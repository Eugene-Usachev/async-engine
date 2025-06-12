//! This module provides an asynchronous `rw_lock` (e.g. [`std::sync::RwLock`]) type [`RWLock`].
//!
//! It allows for asynchronous read or write locking and unlocking, and provides
//! ownership-based locking through [`ReadLockGuard`] and [`WriteLockGuard`].

use crate::runtime::{Call, IsLocal, Task};
use crate::sync::{AsyncRWLock, AsyncReadLockGuard, AsyncWriteLockGuard, LockStatus};
use crate::utils::{
    Backoff, SpinLock, TaskVecFromPool, acquire_task_vec_from_pool, likely, unlikely,
    unwrap_or_bug_hint,
};
use crate::{local_executor, panic_if_local_in_future};
use std::cell::UnsafeCell;
use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed, Release};
use std::task::{Context, Poll};

/// Contains the read and write waiters lists.
#[repr(C)]
struct RWWaitersLists {
    writers_list: SpinLock<TaskVecFromPool>,
    readers_list: SpinLock<TaskVecFromPool>,
}

unsafe impl Send for RWWaitersLists {}
unsafe impl Sync for RWWaitersLists {}

/// `WaitWriteLock` is a [`Future`] that resolves to [`WriteLockGuard`].
#[repr(C)]
struct WaitWriteLock<'rw_lock, T: 'rw_lock + ?Sized> {
    rw_lock: &'rw_lock RWLock<T>,
    was_called: bool,
}

impl<'rw_lock, T: 'rw_lock + ?Sized> WaitWriteLock<'rw_lock, T> {
    /// Creates a new instance of [`WaitWriteLock`].
    fn new(rw_lock: &'rw_lock RWLock<T>) -> Self {
        Self {
            rw_lock,
            was_called: false,
        }
    }
}

impl<'rw_lock, T: 'rw_lock + ?Sized> Future for WaitWriteLock<'rw_lock, T> {
    type Output = WriteLockGuard<'rw_lock, T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        /// # Safety
        ///
        /// Before calling this function, the counter must be incremented by `ONE_WRITER`.
        #[allow(
            clippy::future_not_send,
            reason = "It is not `Send` only when T is not `Send`, it is fine"
        )]
        unsafe fn park_writer_task<'rw_lock, T: 'rw_lock + ?Sized>(
            rw_lock: &'rw_lock RWLock<T>,
            task: Task,
        ) {
            let mut writers = rw_lock.queue.writers_list.lock();

            writers.push(task);

            unsafe {
                local_executor().invoke_call(Call::release_atomic_bool(writers.leak_to_atomic()));
            }
        }

        panic_if_local_in_future!(cx, "RWLock");

        let this = &mut *self;

        if this.was_called {
            debug_assert_eq!(this.rw_lock.get_lock_status(), LockStatus::WriteLocked);

            return Poll::Ready(WriteLockGuard::new(this.rw_lock));
        }

        this.was_called = true;

        let prev = this.rw_lock.state.fetch_add(ONE_WRITER, AcqRel);

        if prev != 0 {
            // We need to park the current task
            // Now the counter is updated, so any unlocker knows about this task

            unsafe { park_writer_task(this.rw_lock, Task::from_context(cx)) };

            return Poll::Pending;
        }

        // We need to update the `state` to `IS_WRITE_MODE_BIT | ONE_WRITER`

        let mut number_of_writers = ONE_WRITER;

        loop {
            // `Acquire` for the failure reduces misses
            // but also decreases performance.
            // The choice can be changed later.
            let res = this.rw_lock.state.compare_exchange(
                number_of_writers,
                IS_WRITE_MODE_BIT | number_of_writers,
                Acquire,
                Acquire,
            );

            match res {
                Ok(_) => return Poll::Ready(WriteLockGuard::new(this.rw_lock)),
                Err(current) => {
                    if current & WRITERS_COUNT_MASK != current {
                        // Another task has already acquired the lock
                        // It doesn't matter if it is a writer or a reader
                        // We need to park the current task
                        // Now the counter is updated, so any unlocker knows about this task

                        unsafe { park_writer_task(this.rw_lock, Task::from_context(cx)) };

                        return Poll::Pending;
                    }

                    // Another writer has tried to acquire the lock,
                    // But he failed.
                    // We need to retry

                    number_of_writers = current;
                }
            }
        }
    }
}

/// `WaitReadLock` is a [`Future`] that resolves to [`ReadLockGuard`].
#[repr(C)]
struct WaitReadLock<'rw_lock, T: 'rw_lock + ?Sized> {
    rw_lock: &'rw_lock RWLock<T>,
    was_called: bool,
}

impl<'rw_lock, T: 'rw_lock + ?Sized> WaitReadLock<'rw_lock, T> {
    /// Creates a new instance of [`WaitReadLock`].
    fn new(rw_lock: &'rw_lock RWLock<T>) -> Self {
        Self {
            rw_lock,
            was_called: false,
        }
    }
}

impl<'rw_lock, T: 'rw_lock + ?Sized> Future for WaitReadLock<'rw_lock, T> {
    type Output = ReadLockGuard<'rw_lock, T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        panic_if_local_in_future!(cx, "RWLock");

        let this = &mut *self;

        if this.was_called {
            debug_assert!(matches!(
                this.rw_lock.get_lock_status(),
                LockStatus::ReadLocked(_)
            ));

            return Poll::Ready(ReadLockGuard::new(this.rw_lock));
        }

        this.was_called = true;

        let prev = this.rw_lock.state.fetch_add(ONE_READER, AcqRel);

        if unlikely(prev & IS_WRITE_MODE_BIT != 0) {
            // We need to park the current task
            // Now the counter is updated, so any unlocker knows about this task

            let mut readers = this.rw_lock.queue.readers_list.lock();

            readers.push(unsafe { Task::from_context(cx) });

            unsafe {
                local_executor().invoke_call(Call::release_atomic_bool(readers.leak_to_atomic()));

                return Poll::Pending;
            }
        }

        Poll::Ready(ReadLockGuard::new(this.rw_lock))
    }
}

// region guards

/// RAII structure used to release the shared read access of a lock when
/// dropped.
///
/// This structure is created by the [`RWLock::read`](crate::sync::RWLock::read)
/// and [`RWLock::try_read`](crate::sync::RWLock::try_read).
pub struct ReadLockGuard<'rw_lock, T: ?Sized> {
    rw_lock: &'rw_lock RWLock<T>,
}

impl<'rw_lock, T: ?Sized> ReadLockGuard<'rw_lock, T> {
    /// Creates a new `ReadLockGuard`.
    #[inline]
    fn new(rw_lock: &'rw_lock RWLock<T>) -> Self {
        Self { rw_lock }
    }
}

impl<'rw_lock, T: ?Sized> AsyncReadLockGuard<'rw_lock, T> for ReadLockGuard<'rw_lock, T> {
    type RWLock = RWLock<T>;

    fn rw_lock(&self) -> &'rw_lock Self::RWLock {
        self.rw_lock
    }

    #[inline]
    unsafe fn leak(self) -> &'rw_lock Self::RWLock {
        ManuallyDrop::new(self).rw_lock
    }
}

impl<T: ?Sized> Deref for ReadLockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.rw_lock.value.get() }
    }
}

impl<T: ?Sized> Drop for ReadLockGuard<'_, T> {
    fn drop(&mut self) {
        unsafe {
            self.rw_lock.read_unlock();
        }
    }
}

unsafe impl<T: ?Sized + Send + Sync> Sync for ReadLockGuard<'_, T> {}
unsafe impl<T: ?Sized + Send> Send for ReadLockGuard<'_, T> {}

/// RAII structure used to release the exclusive write access of a lock when
/// dropped.
///
/// This structure is created by the [`RWLock::write`](crate::sync::RWLock::write)
/// and [`RWLock::try_write`](crate::sync::RWLock::try_write).
pub struct WriteLockGuard<'rw_lock, T: ?Sized> {
    rw_lock: &'rw_lock RWLock<T>,
}

impl<'rw_lock, T: ?Sized> WriteLockGuard<'rw_lock, T> {
    /// Creates a new `WriteLockGuard`.
    #[inline]
    fn new(rw_lock: &'rw_lock RWLock<T>) -> Self {
        Self { rw_lock }
    }
}

impl<'rw_lock, T: ?Sized> AsyncWriteLockGuard<'rw_lock, T> for WriteLockGuard<'rw_lock, T> {
    type RWLock = RWLock<T>;

    fn rw_lock(&self) -> &'rw_lock Self::RWLock {
        self.rw_lock
    }

    #[inline]
    unsafe fn leak(self) -> &'rw_lock Self::RWLock {
        ManuallyDrop::new(self).rw_lock
    }
}

impl<T: ?Sized> Deref for WriteLockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.rw_lock.value.get() }
    }
}

impl<T: ?Sized> DerefMut for WriteLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.rw_lock.value.get() }
    }
}

impl<T: ?Sized> Drop for WriteLockGuard<'_, T> {
    fn drop(&mut self) {
        unsafe {
            self.rw_lock.write_unlock();
        }
    }
}

unsafe impl<T: ?Sized + Send + Sync> Sync for WriteLockGuard<'_, T> {}
unsafe impl<T: ?Sized + Send> Send for WriteLockGuard<'_, T> {}

// endregion

const READERS_COUNT_BITS: u64 = 30;
const WRITERS_COUNT_BITS: u64 = 30;
const MAX_READERS_COUNT: u64 = 1 << READERS_COUNT_BITS;

// Reader count occupies bits from 0 to READERS_COUNT_BITS - 1
const READERS_COUNT_MASK: u64 = (1 << READERS_COUNT_BITS) - 1;

// Writer count occupies bits from READERS_COUNT_BITS to WRITERS_COUNT_BITS + READERS_COUNT_BITS - 1
const WRITERS_COUNT_SHIFT: u64 = READERS_COUNT_BITS;
const WRITERS_COUNT_MASK: u64 = ((1 << WRITERS_COUNT_BITS) - 1) << WRITERS_COUNT_SHIFT;

const ONE_READER: u64 = 1;
const ONE_WRITER: u64 = 1 << WRITERS_COUNT_SHIFT;

const IS_WRITE_MODE_BIT: u64 = 1 << (READERS_COUNT_BITS + WRITERS_COUNT_BITS);

/// An asynchronous version of a [`reader-writer lock`](std::sync::RwLock).
///
/// This type of lock allows a number of readers or at most one writer at any
/// point in time. The write portion of this lock typically allows modification
/// of the underlying data (exclusive access), and the read portion of this lock
/// typically allows for read-only access (shared access).
///
/// In comparison, a [`AsyncMutex`](crate::sync::AsyncMutex)
/// does not distinguish between readers or writers
/// that acquire the lock, therefore blocking any tasks waiting for the lock to
/// become available. An `RWLock` will allow any number of readers to acquire the
/// lock as long as a writer is not holding the lock.
///
/// The type parameter `T` represents the data that this lock protects. It is
/// required that `T` satisfies [`Sync`] to allow concurrent access through readers. The RAII guards
/// returned from the locking methods implement [`Deref`] (and [`DerefMut`]
/// for the `write` methods) to allow access to the content of the lock.
///
/// # The difference between `RWLock` and [`LocalRWLock`](crate::sync::LocalRWLock)
///
/// The `RWLock` works with `shared tasks` and can be shared between threads.
///
/// Read [`Executor`](crate::Executor) for more details.
///
/// # Example
///
/// ```rust
/// use std::collections::HashMap;
/// use std::rc::Rc;
/// use orengine::sync::{AsyncRWLock, RWLock};
///
/// # async fn write_to_the_dump_file(key: usize, value: usize) {}
///
/// async fn dump_storage(storage: Rc<RWLock<HashMap<usize, usize>>>) {
///     let mut read_guard = storage.read().await;
///
///     for (key, value) in read_guard.iter() {
///         write_to_the_dump_file(*key, *value).await;
///     }
///
///     // read lock is released when `guard` goes out of scope
/// }
/// ```
#[repr(C)]
pub struct RWLock<T: ?Sized> {
    /// State contains the mode bit, the number of readers and the special number of writers.
    ///
    /// [`IS_WRITE_MODE_BIT`] is always valid.
    ///
    /// The number of writers can be got by
    /// `(state & WRITERS_COUNT_MASK) >> WRITERS_COUNT_SHIFT`.
    ///
    /// The number of readers can be got by `state & READERS_COUNT_MASK`.
    state: AtomicU64,
    queue: RWWaitersLists,
    value: UnsafeCell<T>,
}

impl<T: ?Sized> IsLocal for RWLock<T> {
    const IS_LOCAL: bool = false;
}

impl<T: ?Sized> RWLock<T> {
    /// Creates a new [`RWLock`].
    pub fn new(value: T) -> Self
    where
        T: Sized,
    {
        if !cfg!(target_has_atomic = "64") {
            unreachable!();
        }

        Self {
            state: AtomicU64::new(0),
            queue: RWWaitersLists {
                readers_list: SpinLock::new(acquire_task_vec_from_pool()),
                writers_list: SpinLock::new(acquire_task_vec_from_pool()),
            },
            value: UnsafeCell::new(value),
        }
    }

    /// Wakes up a writer.
    ///
    /// # Panics
    ///
    /// If it doesn't find a writer to wake up.
    /// Or if it can't upgrade the lock to write mode because it is unsafe (other readers exist).
    fn wake_up_writer<const IS_CALLED_FROM_READ_UNLOCK: bool>(
        &self,
        current: u64,
    ) -> Result<(), u64> {
        if IS_CALLED_FROM_READ_UNLOCK {
            debug_assert_eq!(current & IS_WRITE_MODE_BIT, 0);

            // Upgrades the lock to write mode

            let res =
                self.state
                    .compare_exchange(current, IS_WRITE_MODE_BIT | current, Release, Acquire);

            if unlikely(res.is_err()) {
                return Err(res.unwrap_err());
            }
        }

        let mut writer_task_ = self.queue.writers_list.lock().pop();

        if unlikely(writer_task_.is_none()) {
            let backoff = Backoff::new();

            loop {
                backoff.spin();

                writer_task_ = self.queue.writers_list.lock().pop();

                if writer_task_.is_some() {
                    break;
                }
            }
        }

        local_executor().spawn_shared_task(unwrap_or_bug_hint(writer_task_));

        Ok(())
    }

    /// Wakes up all readers.
    ///
    /// # Panics
    ///
    /// If it doesn't find a reader to wake up.
    /// Or if some writers are waiting.
    fn wake_up_readers(&self, current: u64) -> Result<(), u64> {
        debug_assert_eq!(current & IS_WRITE_MODE_BIT, IS_WRITE_MODE_BIT);
        debug_assert_eq!((current & WRITERS_COUNT_MASK) >> WRITERS_COUNT_SHIFT, 0);
        debug_assert!(current & READERS_COUNT_MASK > 0);

        let mut to_wake = current & !IS_WRITE_MODE_BIT;

        let res = self
            .state
            .compare_exchange(current, to_wake, Release, Acquire);

        if unlikely(res.is_err()) {
            return Err(res.unwrap_err());
        }

        let backoff = Backoff::new();
        let mut readers_list = self.queue.readers_list.lock();

        #[allow(
            clippy::cast_possible_truncation,
            reason = "Number of readers is less than u32::MAX"
        )]
        while readers_list.len() != to_wake as usize {
            drop(readers_list);

            backoff.spin();

            readers_list = self.queue.readers_list.lock();
        }

        while to_wake > 0 {
            let reader_task = unwrap_or_bug_hint(readers_list.pop());

            local_executor().spawn_shared_task(reader_task);

            to_wake -= 1;
        }

        Ok(())
    }
}

impl<T: ?Sized> AsyncRWLock<T> for RWLock<T> {
    type ReadLockGuard<'rw_lock>
        = ReadLockGuard<'rw_lock, T>
    where
        T: 'rw_lock,
        Self: 'rw_lock;
    type WriteLockGuard<'rw_lock>
        = WriteLockGuard<'rw_lock, T>
    where
        T: 'rw_lock,
        Self: 'rw_lock;

    #[inline]
    fn get_lock_status(&self) -> LockStatus {
        let state = self.state.load(Acquire);

        match state {
            0 => LockStatus::Unlocked,
            n if n & IS_WRITE_MODE_BIT == 0 => {
                LockStatus::ReadLocked((n & READERS_COUNT_MASK) as usize)
            }
            _ => LockStatus::WriteLocked,
        }
    }

    #[allow(
        clippy::future_not_send,
        reason = "It is not `Send` only when T is not `Send`, it is fine"
    )]
    fn write<'rw_lock>(&'rw_lock self) -> impl Future<Output = Self::WriteLockGuard<'rw_lock>>
    where
        T: 'rw_lock,
    {
        WaitWriteLock::new(self)
    }

    #[allow(
        clippy::future_not_send,
        reason = "It is not `Send` only when T is not `Send`, it is fine"
    )]
    fn read<'rw_lock>(&'rw_lock self) -> impl Future<Output = Self::ReadLockGuard<'rw_lock>>
    where
        T: 'rw_lock,
    {
        WaitReadLock::new(self)
    }

    #[inline]
    fn try_write(&self) -> Option<Self::WriteLockGuard<'_>> {
        let res = self
            .state
            .compare_exchange(0, IS_WRITE_MODE_BIT | ONE_WRITER, Acquire, Relaxed);

        if res.is_ok() {
            Some(WriteLockGuard::new(self))
        } else {
            None
        }
    }

    #[inline]
    fn try_read(&self) -> Option<Self::ReadLockGuard<'_>> {
        let mut prev = self.state.load(Acquire);

        loop {
            if unlikely(prev & IS_WRITE_MODE_BIT != 0) {
                return None;
            }

            debug_assert!(prev <= MAX_READERS_COUNT);

            // `Acquire` for the failure reduces misses
            // but also decreases performance.
            // The choice can be changed later.
            let res = self
                .state
                .compare_exchange(prev, prev + ONE_READER, Acquire, Acquire);

            match res {
                Ok(_) => return Some(ReadLockGuard::new(self)),
                Err(e) => prev = e,
            }
        }
    }

    #[inline]
    fn get_mut(&mut self) -> &mut T {
        self.value.get_mut()
    }

    #[inline]
    unsafe fn read_unlock(&self) {
        let prev = self.state.fetch_sub(ONE_READER, AcqRel);
        let mut current = prev - ONE_READER;

        debug_assert_eq!(prev & IS_WRITE_MODE_BIT, 0);
        debug_assert!(prev & READERS_COUNT_MASK > 0);

        loop {
            let number_of_waiting_writers = (current & WRITERS_COUNT_MASK) >> WRITERS_COUNT_SHIFT;
            let number_of_readers = current & READERS_COUNT_MASK;

            if likely(number_of_waiting_writers == 0 || number_of_readers != 0) {
                break;
            }

            match self.wake_up_writer::<true>(current) {
                Ok(()) => break,
                Err(real_current) => current = real_current,
            }
        }
    }

    #[inline]
    unsafe fn write_unlock(&self) {
        let prev = self.state.fetch_sub(ONE_WRITER, AcqRel);
        let mut current = prev - ONE_WRITER;

        debug_assert_eq!(prev & IS_WRITE_MODE_BIT, IS_WRITE_MODE_BIT);
        debug_assert!(prev & WRITERS_COUNT_MASK > 0);

        loop {
            let number_of_waiting_writers = (current & WRITERS_COUNT_MASK) >> WRITERS_COUNT_SHIFT;

            if number_of_waiting_writers > 0 {
                match self.wake_up_writer::<false>(current) {
                    Ok(()) => return,
                    Err(real_current) => {
                        current = real_current;

                        continue;
                    }
                }
            } else if current & READERS_COUNT_MASK > 0 {
                match self.wake_up_readers(current) {
                    Ok(()) => {
                        return;
                    }
                    Err(real_current) => {
                        current = real_current;

                        continue;
                    }
                }
            }

            debug_assert_eq!(current, IS_WRITE_MODE_BIT);

            let res = self.state.compare_exchange(current, 0, Release, Acquire);

            match res {
                Ok(_) => return,
                Err(real_current) => {
                    current = real_current;

                    continue;
                }
            }
        }
    }

    #[inline]
    unsafe fn get_read_locked(&self) -> Self::ReadLockGuard<'_> {
        #[cfg(debug_assertions)]
        {
            let current = self.state.load(Acquire);

            assert_ne!(current, 0, "RWLock is unlocked");
            assert_eq!(current & IS_WRITE_MODE_BIT, 0, "RWLock is locked for write");
        }

        ReadLockGuard::new(self)
    }

    #[inline]
    unsafe fn get_write_locked(&self) -> Self::WriteLockGuard<'_> {
        #[cfg(debug_assertions)]
        {
            let current = self.state.load(Acquire);

            assert_ne!(current, 0, "RWLock is unlocked");
            assert_eq!(current & IS_WRITE_MODE_BIT, 1, "RWLock is locked for read");
        }

        WriteLockGuard::new(self)
    }
}

unsafe impl<T: ?Sized + Send + Sync> Sync for RWLock<T> {}
unsafe impl<T: ?Sized + Send> Send for RWLock<T> {}

/// ```compile_fail
/// use orengine::sync::{RWLock, AsyncRWLock};
/// use orengine::yield_now;
///
/// fn check_send<T: Send>(value: T) -> T { value }
///
/// struct NonSend {
///     value: i32,
///     // impl !Send
///     no_send_marker: std::marker::PhantomData<*const ()>,
/// }
///
/// async fn test() {
///     let mutex = RWLock::new(NonSend {
///         value: 0,
///         no_send_marker: std::marker::PhantomData,
///     });
///
///     let guard = check_send(mutex.read()).await;
///     yield_now().await;
///     assert_eq!(guard.value, 0);
///     drop(guard);
/// }
/// ```
///
/// ```rust
/// use orengine::sync::{RWLock, AsyncRWLock};
/// use orengine::yield_now;
///
/// fn check_send<T: Send>(value: T) -> T { value }
///
/// // impl Send
/// struct CanSend {
///     value: i32,
/// }
///
/// async fn test() {
///     let mutex = RWLock::new(CanSend {
///         value: 0,
///     });
///
///     let guard = check_send(mutex.read()).await;
///     yield_now().await;
///     assert_eq!(guard.value, 0);
///     drop(guard);
/// }
/// ```
///
/// ```compile_fail
/// use orengine::sync::{RWLock, AsyncRWLock};
/// use orengine::yield_now;
///
/// fn check_send<T: Send>(value: T) -> T { value }
///
/// struct NonSend {
///     value: i32,
///     // impl !Send
///     no_send_marker: std::marker::PhantomData<*const ()>,
/// }
///
/// async fn test() {
///     let mutex = RWLock::new(NonSend {
///         value: 0,
///         no_send_marker: std::marker::PhantomData,
///     });
///
///     let guard = check_send(mutex.write()).await;
///     yield_now().await;
///     assert_eq!(guard.value, 0);
///     drop(guard);
/// }
/// ```
///
/// ```rust
/// use orengine::sync::{RWLock, AsyncRWLock};
/// use orengine::yield_now;
///
/// fn check_send<T: Send>(value: T) -> T { value }
///
/// // impl Send
/// struct CanSend {
///     value: i32,
/// }
///
/// async fn test() {
///     let mutex = RWLock::new(CanSend {
///         value: 0,
///     });
///
///     let guard = check_send(mutex.write()).await;
///     yield_now().await;
///     assert_eq!(guard.value, 0);
///     drop(guard);
/// }
/// ```
#[allow(dead_code, reason = "It is used only in compile tests")]
fn test_compile_shared_rw_lock() {}

#[cfg(test)]
mod tests {
    use crate as orengine;
    use crate::sync::{AsyncRWLock, LockStatus, RWLock};
    use crate::{local_executor, yield_now};
    use std::sync::Arc;

    #[orengine::test::test_shared]
    fn test_rw_lock() {
        const NUMBER_OF_READERS: usize = 5;

        let rw_lock = Arc::new(RWLock::new(0));

        for i in 1..=NUMBER_OF_READERS {
            let rw_lock_clone = rw_lock.clone();

            local_executor().exec_shared_future(async move {
                let lock = rw_lock_clone.read().await;

                match rw_lock_clone.get_lock_status() {
                    LockStatus::ReadLocked(n) => assert_eq!(n, i),
                    _ => panic!("Should be read locked!"),
                };

                yield_now().await;

                drop(lock);
            });
        }

        match rw_lock.get_lock_status() {
            LockStatus::ReadLocked(n) => assert_eq!(n, NUMBER_OF_READERS),
            _ => panic!("Should be read locked!"),
        };

        let mut write_lock = rw_lock.write().await;

        assert!(rw_lock.try_write().is_none());

        *write_lock += 1;

        assert_eq!(*write_lock, 1);
        assert!(matches!(rw_lock.get_lock_status(), LockStatus::WriteLocked));
    }

    #[orengine::test::test_shared]
    fn test_try_rw_lock() {
        const NUMBER_OF_READERS: usize = 5;

        let rw_lock = Arc::new(RWLock::new(0));

        for i in 1..=NUMBER_OF_READERS {
            let rw_lock_clone = rw_lock.clone();

            local_executor().exec_shared_future(async move {
                let lock = rw_lock_clone.try_read().expect("Failed to get read lock!");

                match rw_lock_clone.get_lock_status() {
                    LockStatus::ReadLocked(n) => assert_eq!(n, i),
                    _ => panic!("Should be read locked!"),
                }

                yield_now().await;

                drop(lock);
            });
        }

        match rw_lock.get_lock_status() {
            LockStatus::ReadLocked(n) => assert_eq!(n, NUMBER_OF_READERS),
            _ => panic!("Should be read locked!"),
        };

        assert!(
            rw_lock.try_write().is_none(),
            "Successful attempt to acquire write lock when rw_lock locked for read"
        );

        yield_now().await;

        match rw_lock.get_lock_status() {
            LockStatus::Unlocked => (),
            _ => panic!("Should be unlocked!"),
        };

        let mut write_lock = rw_lock.try_write().expect("Failed to get write lock!");

        *write_lock += 1;

        assert_eq!(*write_lock, 1);

        match rw_lock.get_lock_status() {
            LockStatus::WriteLocked => (),
            _ => panic!("Should be write locked!"),
        };

        assert!(
            rw_lock.try_read().is_none(),
            "Successful attempt to acquire read lock when rw_lock locked for write"
        );
        assert!(
            rw_lock.try_write().is_none(),
            "Successful attempt to acquire write lock when rw_lock locked for write"
        );
    }
}

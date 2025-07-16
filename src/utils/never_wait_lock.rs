//! This module contains the [`NeverWaitLock`].

use crate::sync::Unlock;
use crate::utils::short_preempt;
use std::cell::UnsafeCell;
use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};

/// An RAII implementation of a "scoped lock" of a mutex. When this structure is
/// dropped (falls out of scope), the lock will be unlocked.
///
/// The data protected by the mutex can be accessed through this guard via its
/// [`Deref`] and [`DerefMut`] implementations.
///
/// This structure is created by the [`try_lock`](NeverWaitLock::try_lock)
/// method on [`NeverWaitLock`].
pub struct NeverWaitLockGuard<'never_wait_lock, T: ?Sized> {
    never_wait_lock: &'never_wait_lock NeverWaitLock<T>,
}

impl<'never_wait_lock, T: ?Sized> NeverWaitLockGuard<'never_wait_lock, T> {
    /// Creates a new [`NeverWaitLockGuard`].
    #[inline]
    pub(crate) fn new(never_wait_lock: &'never_wait_lock NeverWaitLock<T>) -> Self {
        Self { never_wait_lock }
    }

    /// Returns a reference to the original [`NeverWaitLock`].
    #[inline]
    pub fn never_wait_lock(&self) -> &NeverWaitLock<T> {
        self.never_wait_lock
    }

    /// Unlocks the [`NeverWaitLock`]. Calling `guard.unlock()` is equivalent to
    /// calling `drop(guard)`. This was done to improve readability.
    ///
    /// # Attention
    ///
    /// Even if you don't call `guard.unlock()`,
    /// the [`NeverWaitLock`] will be unlocked after the `guard` is dropped.
    #[inline]
    pub fn unlock(self) {
        drop(self);
    }

    /// Returns a reference to the original [`NeverWaitLock`].
    ///
    /// The lock will never be unlocked.
    ///
    /// # Safety
    ///
    /// The mutex is unlocked by calling [`NeverWaitLock::unlock`] later.
    #[inline]
    pub unsafe fn leak(self) -> *const AtomicBool {
        &ManuallyDrop::new(self).never_wait_lock.is_locked
    }

    /// Returns a reference to the [`AtomicBool`]
    /// associated with the original [`NeverWaitLock`] to
    /// [`call`](crate::Executor::invoke_call)
    /// [`ReleaseAtomicBool`](crate::runtime::Call::ReleaseAtomicBool).
    ///
    /// # Safety
    ///
    /// The mutex is unlocked by calling [`NeverWaitLock::unlock`] later
    /// or by [calling](crate::Executor::invoke_call)
    /// [`ReleaseAtomicBool`](crate::runtime::Call::ReleaseAtomicBool).
    #[inline]
    pub unsafe fn leak_to_atomic(self) -> NonNull<AtomicBool> {
        debug_assert!(self.never_wait_lock.is_locked.load(Acquire));

        unsafe {
            NonNull::new_unchecked(
                (&raw const ManuallyDrop::new(self).never_wait_lock.is_locked).cast_mut(),
            )
        }
    }
}

impl<T: ?Sized> Deref for NeverWaitLockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.never_wait_lock.value.get() }
    }
}

impl<T: ?Sized> DerefMut for NeverWaitLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.never_wait_lock.value.get() }
    }
}

impl<T: ?Sized> Drop for NeverWaitLockGuard<'_, T> {
    fn drop(&mut self) {
        unsafe { self.never_wait_lock.unlock() };
    }
}

/// `NeverWaitLock` is a wrapper around [`AtomicBool`] that never waits for the lock
/// but can lock resources.
pub struct NeverWaitLock<T: ?Sized> {
    is_locked: AtomicBool,
    value: UnsafeCell<T>,
}

impl<T: ?Sized> NeverWaitLock<T> {
    /// Creates a new `NeverWaitLock` with the given value.
    pub const fn new(value: T) -> Self
    where
        T: Sized,
    {
        Self {
            is_locked: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    /// If `NeverWaitLock` is unlocked returns [`NeverWaitLockGuard`], otherwise returns [`None`].
    pub fn try_lock(&self) -> Option<NeverWaitLockGuard<T>> {
        if self
            .is_locked
            .compare_exchange_weak(false, true, Acquire, Relaxed)
            .is_ok()
        {
            Some(NeverWaitLockGuard::new(self))
        } else {
            None
        }
    }

    /// If `NeverWaitLock` is unlocked returns [`NeverWaitLockGuard`], otherwise returns [`None`].
    ///
    /// It preempts the current task on failure, but it can be more useful in cases
    /// where the lock is very likely to be locked for __less than 100 nanoseconds__.
    ///
    /// It is `pub(crate)` because users can misuse it and fill the preempt stack.
    pub(crate) fn try_lock_with_preemption(&self) -> Option<NeverWaitLockGuard<T>> {
        if let Some(guard) = self.try_lock() {
            return Some(guard);
        }

        short_preempt();

        if let Some(guard) = self.try_lock() {
            return Some(guard);
        }

        None
    }

    /// Returns a reference to the underlying data. It is safe because it uses `&mut self`.
    #[inline]
    pub fn get_mut(&mut self) -> &mut T {
        self.value.get_mut()
    }

    /// Returns a reference to the inner value.
    ///
    /// # Safety
    ///
    /// - The `NeverWaitLock` must be locked.
    ///
    /// - And only the current task has ownership of this `NeverWaitLock`.
    #[inline]
    #[allow(
        clippy::mut_from_ref,
        reason = "The caller guarantees safety using this code"
    )]
    pub unsafe fn get_locked(&self) -> &mut T {
        debug_assert!(self.is_locked.load(Acquire));
        unsafe { &mut *self.value.get() }
    }
}

impl<T: ?Sized> Unlock for NeverWaitLock<T> {
    #[inline]
    unsafe fn unlock(&self) {
        debug_assert!(self.is_locked.load(Acquire));

        self.is_locked.store(false, Release);
    }
}

unsafe impl<T: ?Sized + Send + Sync> Sync for NeverWaitLock<T> {}
unsafe impl<T: ?Sized + Send> Send for NeverWaitLock<T> {}

#[cfg(test)]
mod tests {
    use crate as orengine;
    use crate::sync::{AsyncWaitGroup, WaitGroup};
    use crate::test::sched_future;
    use crate::utils::never_wait_lock::NeverWaitLock;
    use std::sync::Arc;

    #[orengine::test::test_shared]
    fn test_never_wait_lock_lock() {
        let mutex = Arc::new(NeverWaitLock::new(false));
        let mutex_clone = mutex.clone();
        let lock_wg = Arc::new(WaitGroup::new());
        let lock_wg_clone = lock_wg.clone();
        let unlock_wg = Arc::new(WaitGroup::new());
        let unlock_wg_clone = unlock_wg.clone();
        let second_lock = Arc::new(WaitGroup::new());
        let second_lock_clone = second_lock.clone();

        lock_wg.add(1).await;
        unlock_wg.add(1).await;

        sched_future(async move {
            let mut value = mutex_clone.try_lock().unwrap();

            println!("1");

            lock_wg_clone.done().await;
            unlock_wg_clone.wait().await;

            println!("4");

            *value = true;

            drop(value);

            second_lock_clone.done().await;

            println!("5");
        });

        lock_wg.wait().await;

        println!("2");

        let value = mutex.try_lock();

        println!("3");

        assert!(value.is_none());

        second_lock.inc().await;
        unlock_wg.done().await;

        second_lock.wait().await;

        let value = mutex.try_lock();

        println!("6");

        match value {
            Some(v) => assert!(*v, "not waited"),
            None => panic!("can't acquire lock"),
        }
    }
}

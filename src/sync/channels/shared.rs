use crate::panic_if_local_in_future;
use crate::runtime::call::Call;
use crate::runtime::waiting_task::WaitingTask;
use crate::runtime::{IsLocal, Task, TaskWithDeadline, local_executor};
use crate::sync::channels::select::SelectNonBlockingBranchResult;
use crate::sync::channels::state::{CallState, CallStatePtr};
use crate::sync::channels::waiting_task::waiting_select_task_deque::WaitingTaskSharedDequeGuard;
use crate::sync::channels::waiting_task::{PopIfAcquiredResult, TaskInSelectBranch};
use crate::sync::channels::{SelectReceiver, SelectSender};
use crate::sync::mutexes::naive_shared::NaiveMutex;
use crate::sync::{
    AsyncChannel, AsyncMutex, AsyncReceiver, AsyncSender, RecvErr, RecvTimeoutErr, SendErr,
    SendTimeoutErr, TryRecvErr, TrySendErr, Unlock,
};
use crate::utils::{OrengineInstant, Ptr, unwrap_or_bug_hint};
use crate::utils::{unlikely, unreachable_hint};
use std::collections::VecDeque;
use std::future::Future;
use std::mem::ManuallyDrop;
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::ptr::{NonNull, copy_nonoverlapping};
use std::task::{Context, Poll};
use std::{mem, ptr};

/// This is the internal data structure for the [`channel`](Channel).
/// It holds the actual storage for the values and manages the queue of senders and receivers.
#[repr(C)]
struct Inner<T> {
    storage: VecDeque<T>,
    capacity: usize,
    is_closed: bool,
    deque: WaitingTaskSharedDequeGuard<T>,
}

unsafe impl<T: Send> Sync for Inner<T> {}
#[allow(
    clippy::non_send_fields_in_send_ty,
    reason = "We guarantee that `Inner<T>` is `Send`"
)]
unsafe impl<T: Send> Send for Inner<T> {}

// region futures

/// Returns `Poll::Pending` and releases the lock by invoking [`Call::release_atomic_bool`].
/// [`release_atomic_bool`](crate::Executor::release_atomic_bool).
macro_rules! return_pending_and_release_lock {
    ($ex:expr, $lock:expr) => {
        unsafe { $ex.invoke_call(Call::release_atomic_bool($lock.leak_to_atomic())) };

        return Poll::Pending;
    };
}

/// Returns `Poll::Pending` if the mutex is not acquired, otherwise returns lock.
macro_rules! acquire_lock {
    ($mutex:expr, $task:expr) => {
        match $mutex.try_lock() {
            Some(lock) => lock,
            None => {
                unsafe { local_executor().spawn_task_at_end_of_shared_tasks_queue($task) };

                return Poll::Pending;
            }
        }
    };
}

/// This struct represents a future that waits for a value to be sent
/// into the [`channel`](Channel).
///
/// When the future is polled, it either sends the value immediately (if there is capacity) or
/// gets parked in the list of waiting senders.
///
/// # Panics or memory leaks
///
/// If [`WaitSend::poll`] is not called.
#[repr(C)]
pub struct WaitSend<'future, T> {
    inner: &'future NaiveMutex<Inner<T>>,
    value: ManuallyDrop<T>,
    call_state: CallState,
    #[cfg(debug_assertions)]
    was_awaited: bool,
}

impl<'future, T> WaitSend<'future, T> {
    /// Creates a new [`WaitSend`].
    #[inline]
    fn new(value: T, inner: &'future NaiveMutex<Inner<T>>) -> Self {
        Self {
            inner,
            call_state: CallState::FirstCall,
            value: ManuallyDrop::new(value),
            #[cfg(debug_assertions)]
            was_awaited: false,
        }
    }
}

impl<T> Future for WaitSend<'_, T> {
    type Output = Result<(), SendErr<T>>;

    #[inline]
    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        #[cfg(debug_assertions)]
        {
            this.was_awaited = true;
        }
        panic_if_local_in_future!(cx, "Channel");

        match this.call_state {
            CallState::FirstCall => {
                let mut inner_lock = acquire_lock!(this.inner, Task::from_context(cx));
                if unlikely(inner_lock.is_closed) {
                    return Poll::Ready(Err(SendErr::Closed(unsafe {
                        ManuallyDrop::take(&mut this.value)
                    })));
                }

                let was_written =
                    inner_lock
                        .deque
                        .try_pop_front_receiver_and_call(|call_state, slot| unsafe {
                            this.inner.unlock(); // Release the lock here to improve performance

                            copy_nonoverlapping(&*this.value, slot.as_ptr(), 1);
                            call_state.write(CallState::WokenToReturnReady);
                        });
                if was_written {
                    mem::forget(inner_lock); // Was released above

                    return Poll::Ready(Ok(()));
                }

                let len = inner_lock.storage.len();
                if unlikely(len >= inner_lock.capacity) {
                    inner_lock.deque.push_back_sender(WaitingTask::common(
                        unsafe { Task::from_context(cx) },
                        CallStatePtr::new(&mut this.call_state),
                        NonNull::from(&mut *this.value),
                    ));

                    return_pending_and_release_lock!(local_executor(), inner_lock);
                }

                unsafe {
                    inner_lock
                        .storage
                        .push_back(ManuallyDrop::take(&mut this.value));
                }

                Poll::Ready(Ok(()))
            }
            CallState::WokenToReturnReady => Poll::Ready(Ok(())),
            CallState::WokenByClose => Poll::Ready(Err(SendErr::Closed(unsafe {
                ManuallyDrop::take(&mut this.value)
            }))),
            CallState::WokenByDeadline => unreachable_hint(),
        }
    }
}

unsafe impl<T: Send> Send for WaitSend<'_, T> {}
impl<T: UnwindSafe> UnwindSafe for WaitSend<'_, T> {}
impl<T: RefUnwindSafe> RefUnwindSafe for WaitSend<'_, T> {}

#[cfg(debug_assertions)]
impl<T> Drop for WaitSend<'_, T> {
    fn drop(&mut self) {
        assert!(
            self.was_awaited,
            "`WaitSend` was not awaited. This will cause a memory leak."
        );
    }
}

/// This struct represents a future that waits for a value to be sent
/// into the [`channel`](Channel) with a deadline.
///
/// When the future is polled, it either sends the value immediately (if there is capacity) or
/// gets parked in the list of waiting senders.
///
/// # Panics or memory leaks
///
/// If [`WaitSend::poll`] is not called.
#[repr(C)]
pub struct WaitSendWithDeadline<'future, T> {
    inner: &'future NaiveMutex<Inner<T>>,
    value: ManuallyDrop<T>,
    call_state: CallState,
    deadline: OrengineInstant,
    #[cfg(debug_assertions)]
    was_awaited: bool,
}

impl<'future, T> WaitSendWithDeadline<'future, T> {
    /// Creates a new [`WaitSendWithDeadline`].
    #[inline]
    fn new(value: T, inner: &'future NaiveMutex<Inner<T>>, deadline: OrengineInstant) -> Self {
        Self {
            inner,
            call_state: CallState::FirstCall,
            value: ManuallyDrop::new(value),
            deadline,
            #[cfg(debug_assertions)]
            was_awaited: false,
        }
    }
}

impl<T> Future for WaitSendWithDeadline<'_, T> {
    type Output = Result<(), SendTimeoutErr<T>>;

    #[inline]
    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        #[cfg(debug_assertions)]
        {
            this.was_awaited = true;
        }
        panic_if_local_in_future!(cx, "Channel");

        match this.call_state {
            CallState::FirstCall => {
                let mut inner_lock = acquire_lock!(this.inner, Task::from_context(cx));
                if unlikely(inner_lock.is_closed) {
                    return Poll::Ready(Err(SendTimeoutErr::Closed(unsafe {
                        ManuallyDrop::take(&mut this.value)
                    })));
                }

                let was_written =
                    inner_lock
                        .deque
                        .try_pop_front_receiver_and_call(|call_state, slot| unsafe {
                            this.inner.unlock(); // Release the lock here to improve performance

                            copy_nonoverlapping(&*this.value, slot.as_ptr(), 1);
                            call_state.write(CallState::WokenToReturnReady);
                        });
                if was_written {
                    mem::forget(inner_lock); // Was released above

                    return Poll::Ready(Ok(()));
                }

                let len = inner_lock.storage.len();
                let call_state_ptr = CallStatePtr::new(&mut this.call_state);

                if unlikely(len >= inner_lock.capacity) {
                    inner_lock
                        .deque
                        .push_back_sender(WaitingTask::common_with_deadline(
                            TaskWithDeadline::create_new_and_register(
                                unsafe { Task::from_context(cx) },
                                call_state_ptr,
                                this.deadline,
                            ),
                            call_state_ptr,
                            NonNull::from(&mut *this.value),
                        ));

                    return_pending_and_release_lock!(local_executor(), inner_lock);
                }

                unsafe {
                    inner_lock
                        .storage
                        .push_back(ManuallyDrop::take(&mut this.value));
                }

                Poll::Ready(Ok(()))
            }
            CallState::WokenToReturnReady => Poll::Ready(Ok(())),
            CallState::WokenByClose => Poll::Ready(Err(SendTimeoutErr::Closed(unsafe {
                ManuallyDrop::take(&mut this.value)
            }))),
            CallState::WokenByDeadline => Poll::Ready(Err(SendTimeoutErr::Timeout(unsafe {
                ManuallyDrop::take(&mut this.value)
            }))),
        }
    }
}

unsafe impl<T: Send> Send for WaitSendWithDeadline<'_, T> {}
impl<T: UnwindSafe> UnwindSafe for WaitSendWithDeadline<'_, T> {}
impl<T: RefUnwindSafe> RefUnwindSafe for WaitSendWithDeadline<'_, T> {}

#[cfg(debug_assertions)]
impl<T> Drop for WaitSendWithDeadline<'_, T> {
    fn drop(&mut self) {
        assert!(
            self.was_awaited,
            "`WaitSend` was not awaited. This will cause a memory leak."
        );
    }
}

/// This struct represents a future that waits for a value to be
/// received from the [`channel`](Channel).
///
/// When the future is polled, it either receives the value immediately (if available) or
/// gets parked in the list of waiting receivers.
#[repr(C)]
pub struct WaitRecv<'future, T> {
    inner: &'future NaiveMutex<Inner<T>>,
    slot: *mut T,
    call_state: CallState,
}

impl<'future, T> WaitRecv<'future, T> {
    /// Creates a new [`WaitRecv`].
    #[inline]
    fn new(inner: &'future NaiveMutex<Inner<T>>, slot: *mut T) -> Self {
        Self {
            inner,
            call_state: CallState::FirstCall,
            slot,
        }
    }
}

impl<T> Future for WaitRecv<'_, T> {
    type Output = Result<(), RecvErr>;

    #[inline]
    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        panic_if_local_in_future!(cx, "Channel");

        match this.call_state {
            CallState::FirstCall => {
                let mut inner_lock = acquire_lock!(this.inner, Task::from_context(cx));
                if unlikely(inner_lock.is_closed) {
                    return Poll::Ready(Err(RecvErr::Closed));
                }

                if unlikely(inner_lock.storage.is_empty()) {
                    let was_written = inner_lock.deque.try_pop_front_sender_and_call(
                        |call_state, value| unsafe {
                            this.inner.unlock(); // Release the lock here to improve performance

                            copy_nonoverlapping(value.as_ptr(), this.slot, 1);
                            call_state.write(CallState::WokenToReturnReady);
                        },
                    );
                    if was_written {
                        mem::forget(inner_lock); // Was released above

                        return Poll::Ready(Ok(()));
                    }

                    inner_lock.deque.push_back_receiver(WaitingTask::common(
                        unsafe { Task::from_context(cx) },
                        CallStatePtr::new(&mut this.call_state),
                        NonNull::from(unsafe { &mut *this.slot }),
                    ));

                    return_pending_and_release_lock!(local_executor(), inner_lock);
                }

                unsafe {
                    this.slot
                        .write(unwrap_or_bug_hint(inner_lock.storage.pop_front()));
                }

                let storage_ref = &mut inner_lock.get_mut().storage;

                let was_written =
                    inner_lock
                        .deque
                        .try_pop_front_sender_and_call(|call_state, value| unsafe {
                            storage_ref.push_back(value.read());

                            this.inner.unlock(); // Release the lock here to improve performance

                            call_state.write(CallState::WokenToReturnReady);
                        });
                if was_written {
                    mem::forget(inner_lock); // Was released above
                }

                Poll::Ready(Ok(()))
            }

            CallState::WokenToReturnReady => Poll::Ready(Ok(())),

            CallState::WokenByClose => Poll::Ready(Err(RecvErr::Closed)),

            CallState::WokenByDeadline => unreachable_hint(),
        }
    }
}

unsafe impl<T: Send> Send for WaitRecv<'_, T> {}
impl<T: UnwindSafe> UnwindSafe for WaitRecv<'_, T> {}
impl<T: RefUnwindSafe> RefUnwindSafe for WaitRecv<'_, T> {}

/// This struct represents a future that waits for a value to be
/// received from the [`channel`](Channel) with a deadline.
///
/// When the future is polled, it either receives the value immediately (if available) or
/// gets parked in the list of waiting receivers.
#[repr(C)]
pub struct WaitRecvWithDeadline<'future, T> {
    inner: &'future NaiveMutex<Inner<T>>,
    slot: *mut T,
    call_state: CallState,
    deadline: OrengineInstant,
}

impl<'future, T> WaitRecvWithDeadline<'future, T> {
    /// Creates a new [`WaitRecvWithDeadline`].
    #[inline]
    fn new(inner: &'future NaiveMutex<Inner<T>>, slot: *mut T, deadline: OrengineInstant) -> Self {
        Self {
            inner,
            call_state: CallState::FirstCall,
            slot,
            deadline,
        }
    }
}

impl<T> Future for WaitRecvWithDeadline<'_, T> {
    type Output = Result<(), RecvTimeoutErr>;

    #[inline]
    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        panic_if_local_in_future!(cx, "Channel");

        match this.call_state {
            CallState::FirstCall => {
                let mut inner_lock = acquire_lock!(this.inner, Task::from_context(cx));
                if unlikely(inner_lock.is_closed) {
                    return Poll::Ready(Err(RecvTimeoutErr::Closed));
                }

                if unlikely(inner_lock.storage.is_empty()) {
                    let was_written = inner_lock.deque.try_pop_front_sender_and_call(
                        |call_state, value| unsafe {
                            this.inner.unlock(); // Release the lock here to improve performance

                            copy_nonoverlapping(value.as_ptr(), this.slot, 1);
                            call_state.write(CallState::WokenToReturnReady);
                        },
                    );
                    if was_written {
                        mem::forget(inner_lock); // Was released above

                        return Poll::Ready(Ok(()));
                    }

                    let call_state_ptr = CallStatePtr::new(&mut this.call_state);

                    inner_lock
                        .deque
                        .push_back_receiver(WaitingTask::common_with_deadline(
                            TaskWithDeadline::create_new_and_register(
                                unsafe { Task::from_context(cx) },
                                call_state_ptr,
                                this.deadline,
                            ),
                            call_state_ptr,
                            NonNull::from(unsafe { &mut *this.slot }),
                        ));

                    return_pending_and_release_lock!(local_executor(), inner_lock);
                }

                unsafe {
                    this.slot
                        .write(unwrap_or_bug_hint(inner_lock.storage.pop_front()));
                }

                let storage_ref = &mut inner_lock.get_mut().storage;

                let was_written =
                    inner_lock
                        .deque
                        .try_pop_front_sender_and_call(|call_state, value| unsafe {
                            storage_ref.push_back(value.read());

                            this.inner.unlock(); // Release the lock here to improve performance

                            call_state.write(CallState::WokenToReturnReady);
                        });
                if was_written {
                    mem::forget(inner_lock); // Was released above
                }

                Poll::Ready(Ok(()))
            }

            CallState::WokenToReturnReady => Poll::Ready(Ok(())),

            CallState::WokenByClose => Poll::Ready(Err(RecvTimeoutErr::Closed)),

            CallState::WokenByDeadline => Poll::Ready(Err(RecvTimeoutErr::Timeout)),
        }
    }
}

unsafe impl<T: Send> Send for WaitRecvWithDeadline<'_, T> {}
impl<T: UnwindSafe> UnwindSafe for WaitRecvWithDeadline<'_, T> {}
impl<T: RefUnwindSafe> RefUnwindSafe for WaitRecvWithDeadline<'_, T> {}

// endregion

fn close_with_lock<T>(inner: &mut Inner<T>) {
    inner.is_closed = true;

    inner.deque.clear();
}

/// Closes the [`channel`](Channel) and wakes all senders and receivers.
#[inline]
#[allow(
    clippy::future_not_send,
    reason = "It is not `Send` only when T is not `Send`, it is fine"
)]
async fn close<T>(inner: &NaiveMutex<Inner<T>>) {
    let mut inner_lock = inner.lock().await;

    close_with_lock(&mut inner_lock);
}

macro_rules! generate_recv_in_ptr_and_recv_in_ptr_with_timeout {
    () => {
        /// Asynchronously receives a value from the [`channel`](AsyncChannel) to the provided `slot`.
        ///
        /// If the [`channel`](AsyncChannel) is empty, the receiver waits until a value
        /// is available or the [`channel`](AsyncChannel) is closed.
        ///
        /// Else, the value is immediately received.
        ///
        /// # On close
        ///
        /// Returns `Err(`[`RecvErr::Closed`]`)` if the [`channel`](AsyncChannel) is closed.
        ///
        /// # Attention
        ///
        /// __Doesn't drop__ the previous value in the `slot`.
        ///
        /// # Safety
        ///
        /// - The provided pointer is valid and aligned;
        ///
        /// - The previous value is dropped.
        unsafe fn recv_in_ptr(&self, slot: Ptr<T>) -> impl Future<Output = Result<(), RecvErr>> {
            WaitRecv::new(self.inner(), slot.as_ptr())
        }

        /// Same as [`recv_in_ptr`](Self::recv_in_ptr), but with a deadline.
        unsafe fn recv_in_ptr_with_deadline(
            &self,
            slot: Ptr<T>,
            deadline: impl Into<OrengineInstant>,
        ) -> impl Future<Output = Result<(), RecvTimeoutErr>> {
            WaitRecvWithDeadline::new(self.inner(), slot.as_ptr(), deadline.into())
        }
    };
}

macro_rules! generate_try_send {
    () => {
        fn try_send(&self, value: T) -> Result<(), TrySendErr<T>> {
            match self.inner.try_lock() {
                Some(mut inner_lock) => {
                    if unlikely(inner_lock.is_closed) {
                        return Err(TrySendErr::Closed(value));
                    }

                    let was_written = inner_lock.deque.try_pop_front_receiver_and_call(
                        |call_state, slot| unsafe {
                            self.inner.unlock(); // Release the lock here to improve performance

                            ptr::copy_nonoverlapping(&value, slot.as_ptr(), 1);
                            call_state.write(CallState::WokenToReturnReady);
                        },
                    );
                    if was_written {
                        mem::forget(inner_lock); // Was released above
                        mem::forget(value); // We copied it

                        return Ok(());
                    }

                    let len = inner_lock.storage.len();
                    if len >= inner_lock.capacity {
                        return Err(TrySendErr::Full(value));
                    }

                    inner_lock.storage.push_back(value);

                    Ok(())
                }
                None => Err(TrySendErr::Locked(value)),
            }
        }
    };
}

macro_rules! generate_send_or_subscribe {
    () => {
        fn send_or_subscribe(
            &self,
            data: NonNull<Self::Data>,
            state: CallStatePtr,
            task_in_select_branch: TaskInSelectBranch,
        ) -> SelectNonBlockingBranchResult {
            let mut inner_lock = {
                let backoff = $crate::utils::Backoff::new();

                loop {
                    let Some(inner_lock) = self.inner.try_lock() else {
                        backoff.spin();

                        continue;
                    };

                    break inner_lock;
                }
            };

            if unlikely(inner_lock.is_closed) {
                return if let Some(task) = task_in_select_branch.acquire_once() {
                    state.set_to_closed();

                    local_executor().spawn_shared_task(task);

                    SelectNonBlockingBranchResult::Success
                } else {
                    SelectNonBlockingBranchResult::AlreadyAcquired
                };
            }

            let result = inner_lock
                .deque
                .try_pop_front_receiver_and_call_if_acquired(
                    |call_state, slot| unsafe {
                        self.inner.unlock(); // Release the lock here to improve performance

                        let data = data.as_ref();

                        copy_nonoverlapping(data, slot.as_ptr(), 1);

                        call_state.write(CallState::WokenToReturnReady);
                    },
                    task_in_select_branch,
                );

            match result {
                PopIfAcquiredResult::Ok => {
                    mem::forget(inner_lock); // Was released above

                    SelectNonBlockingBranchResult::Success
                }

                PopIfAcquiredResult::AlreadyAcquired => {
                    SelectNonBlockingBranchResult::AlreadyAcquired
                }

                PopIfAcquiredResult::NoData(task_in_select_branch) => {
                    let len = inner_lock.storage.len();
                    if unlikely(len >= inner_lock.capacity) {
                        inner_lock.deque.push_back_sender(WaitingTask::in_selector(
                            task_in_select_branch,
                            state,
                            data,
                        ));

                        return SelectNonBlockingBranchResult::NotReady;
                    }

                    match task_in_select_branch.acquire_once() {
                        Some(task) => {
                            inner_lock.storage.push_back(unsafe { data.read() });

                            drop(inner_lock);

                            local_executor().spawn_shared_task(task);

                            SelectNonBlockingBranchResult::Success
                        }
                        None => SelectNonBlockingBranchResult::AlreadyAcquired,
                    }
                }

                _ => unreachable_hint(),
            }
        }
    };
}

macro_rules! generate_try_recv_in {
    () => {
        /// Tries to receive a value from the [`channel`](AsyncChannel) to the provided `slot`.
        ///
        /// If the [`channel`](AsyncChannel) is empty, the receiver
        /// returns `Err(`[`TryRecvErr::Empty`]`)`.
        ///
        /// If the [`channel`](AsyncChannel) is locked, the receiver
        /// returns `Err(`[`TryRecvErr::Locked`]`)`.
        ///
        /// If the [`channel`](AsyncChannel) is closed, the receiver
        /// returns `Err(`[`TryRecvErr::Closed`]`)`.
        ///
        /// Else, the value is immediately received.
        ///
        /// # The difference between `try_recv_in_ptr` and `recv_in_ptr`
        ///
        /// `try_recv_in_ptr` doesn't block the current task.
        ///
        /// # Attention
        ///
        /// __Doesn't drop__ the previous value in the `slot`.
        ///
        /// # Safety
        ///
        /// - The provided pointer is valid and aligned;
        ///
        /// - The previous value is dropped.
        unsafe fn try_recv_in_ptr(&self, slot: Ptr<T>) -> Result<(), TryRecvErr> {
            match self.inner.try_lock() {
                Some(mut inner_lock) => {
                    if unlikely(inner_lock.is_closed) {
                        return Err(TryRecvErr::Closed);
                    }

                    if unlikely(inner_lock.storage.len() == 0) {
                        let was_written = inner_lock.deque.try_pop_front_sender_and_call(
                            |call_state, value| unsafe {
                                self.inner.unlock(); // Release the lock here to improve performance

                                copy_nonoverlapping(value.as_ptr(), slot.as_ptr(), 1);
                                call_state.write(CallState::WokenToReturnReady);
                            },
                        );
                        if was_written {
                            mem::forget(inner_lock); // Was released above

                            return Ok(());
                        }

                        return Err(TryRecvErr::Empty);
                    }

                    unsafe {
                        slot.write(unwrap_or_bug_hint(inner_lock.storage.pop_front()));
                    }

                    let storage_ref = &mut inner_lock.get_mut().storage;

                    let was_written = inner_lock.deque.try_pop_front_sender_and_call(
                        |call_state, value| unsafe {
                            storage_ref.push_back(value.read());

                            self.inner.unlock(); // Release the lock here to improve performance

                            call_state.write(CallState::WokenToReturnReady);
                        },
                    );
                    if was_written {
                        mem::forget(inner_lock); // Was released above
                    }

                    Ok(())
                }
                None => Err(TryRecvErr::Locked),
            }
        }
    };
}

macro_rules! generate_recv_or_subscribe {
    () => {
        fn recv_or_subscribe(
            &self,
            slot: NonNull<Self::Data>,
            state: CallStatePtr,
            task_in_select_branch: TaskInSelectBranch,
        ) -> SelectNonBlockingBranchResult {
            let mut inner_lock = {
                let backoff = $crate::utils::Backoff::new();

                loop {
                    let Some(inner_lock) = self.inner.try_lock() else {
                        backoff.spin();

                        continue;
                    };

                    break inner_lock;
                }
            };

            if unlikely(inner_lock.is_closed) {
                return match task_in_select_branch.acquire_once() {
                    Some(task) => {
                        state.set_to_closed();

                        local_executor().spawn_shared_task(task);

                        SelectNonBlockingBranchResult::Success
                    }
                    None => SelectNonBlockingBranchResult::AlreadyAcquired,
                };
            }

            if unlikely(inner_lock.storage.len() == 0) {
                let result = inner_lock.deque.try_pop_front_sender_and_call_if_acquired(
                    |call_state, value| unsafe {
                        self.inner.unlock(); // Release the lock here to improve performance

                        copy_nonoverlapping(value.as_ptr(), slot.as_ptr(), 1);

                        call_state.write(CallState::WokenToReturnReady);
                    },
                    task_in_select_branch,
                );

                match result {
                    PopIfAcquiredResult::Ok => {
                        mem::forget(inner_lock); // Was released above

                        return SelectNonBlockingBranchResult::Success;
                    }

                    PopIfAcquiredResult::AlreadyAcquired => {
                        return SelectNonBlockingBranchResult::AlreadyAcquired;
                    }

                    PopIfAcquiredResult::NoData(task_in_select_branch) => {
                        inner_lock
                            .deque
                            .push_back_receiver(WaitingTask::in_selector(
                                task_in_select_branch,
                                state,
                                slot,
                            ));

                        return SelectNonBlockingBranchResult::NotReady;
                    }

                    _ => unreachable_hint(),
                }
            }

            match task_in_select_branch.acquire_once() {
                Some(task) => {
                    unsafe { slot.write(unwrap_or_bug_hint(inner_lock.storage.pop_front())) };

                    let storage_ref = &mut inner_lock.get_mut().storage;

                    let was_written = inner_lock.deque.try_pop_front_sender_and_call(
                        |call_state, value| unsafe {
                            storage_ref.push_back(value.read());

                            self.inner.unlock(); // Release the lock here to improve performance

                            call_state.write(CallState::WokenToReturnReady);
                        },
                    );
                    if !was_written {
                        drop(inner_lock);
                    } else {
                        mem::forget(inner_lock); // Was released above
                    }

                    local_executor().spawn_shared_task(task);

                    SelectNonBlockingBranchResult::Success
                }
                None => SelectNonBlockingBranchResult::AlreadyAcquired,
            }
        }
    };
}

// region channel

/// The `Channel` provides an asynchronous communication channel between tasks.
///
/// It supports both [`bounded`](Channel::bounded) and [`unbounded`](Channel::unbounded)
/// channels for sending and receiving values.
///
/// When the [`channel`](Channel) is not empty, values are received immediately, else
/// the reception operation is waiting until a value is available or
/// the [`channel`](Channel) is closed.
///
/// When the channel is not full, values are sent immediately, else
/// the sending operation is waiting until capacity is available or
/// the [`channel`](Channel) is closed.
///
/// # The difference between `Channel` and [`LocalChannel`](crate::sync::LocalChannel)
///
/// The `Channel` works with `shared tasks` and can be shared between threads.
///
/// Read [`Executor`](crate::Executor) for more details.
///
/// # Example
///
/// ```rust
/// use orengine::sync::{AsyncChannel, AsyncReceiver, AsyncSender};
///
///  async fn foo() {
///     let channel = orengine::sync::Channel::bounded(1); // capacity = 1
///
///     channel.send(1).await.unwrap();
///     let res = channel.recv().await.unwrap();
///     assert_eq!(res, 1);
/// }
/// ```
pub struct Channel<T> {
    inner: NaiveMutex<Inner<T>>,
}

impl<T> Channel<T> {
    /// Returns a reference to the inner [`NaiveMutex`].
    fn inner(&self) -> &NaiveMutex<Inner<T>> {
        &self.inner
    }

    generate_recv_in_ptr_and_recv_in_ptr_with_timeout!();

    generate_try_recv_in!();

    /// Returns current len, number of receivers and number of senders.
    ///
    /// It is async because it needs to acquire the lock.
    #[inline]
    pub async fn fullness_state(&self) -> (usize, usize, usize) {
        let inner = self.inner.lock().await;
        let number_of_senders_or_receivers = inner.deque.number_of_senders_or_receivers();
        let len = number_of_senders_or_receivers.unsigned_abs();

        if number_of_senders_or_receivers > 0 {
            (len, len, 0)
        } else {
            (len, 0, len)
        }
    }
}

impl<T> AsyncChannel<T> for Channel<T> {
    /// Creates a bounded [`channel`](Channel) with a given capacity.
    ///
    /// A bounded channel limits the number of items that can be stored before sending blocks.
    /// Once the [`channel`](Channel) reaches its capacity,
    /// senders will block until space becomes available.
    ///
    /// # Example
    ///
    /// ```rust
    /// use orengine::sync::{AsyncChannel, AsyncSender};
    ///
    ///  async fn foo() {
    ///     let channel = orengine::sync::Channel::bounded(1);
    ///
    ///     channel.send(1).await.unwrap(); // not blocked
    ///     channel.send(2).await.unwrap(); // blocked because the channel is full
    /// }
    /// ```
    fn bounded(capacity: usize) -> Self {
        Self {
            inner: NaiveMutex::new(Inner {
                storage: VecDeque::with_capacity(capacity),
                capacity,
                is_closed: false,
                deque: WaitingTaskSharedDequeGuard::new(),
            }),
        }
    }

    fn unbounded() -> Self {
        Self {
            inner: NaiveMutex::new(Inner {
                storage: VecDeque::with_capacity(0),
                capacity: usize::MAX,
                is_closed: false,
                deque: WaitingTaskSharedDequeGuard::new(),
            }),
        }
    }

    #[allow(
        clippy::future_not_send,
        reason = "It is not `Send` only when T is not `Send`, it is fine"
    )]
    fn close(&self) -> impl Future<Output = ()> {
        close(&self.inner)
    }
}

impl<T> IsLocal for Channel<T> {
    const IS_LOCAL: bool = false;
}

impl<T> AsyncSender<T> for Channel<T> {
    #[allow(
        clippy::future_not_send,
        reason = "It is not `Send` only when T is not `Send`, it is fine"
    )]
    fn send(&self, value: T) -> impl Future<Output = Result<(), SendErr<T>>> {
        WaitSend::new(value, &self.inner)
    }

    #[allow(
        clippy::future_not_send,
        reason = "It is not `Send` only when T is not `Send`, it is fine"
    )]
    fn send_deadline(
        &self,
        value: T,
        deadline: impl Into<OrengineInstant>,
    ) -> impl Future<Output = Result<(), SendTimeoutErr<T>>> {
        WaitSendWithDeadline::new(value, &self.inner, deadline.into())
    }

    generate_try_send!();

    #[allow(
        clippy::future_not_send,
        reason = "It is not `Send` only when T is not `Send`, it is fine"
    )]
    async fn sender_close(&self) {
        close(&self.inner).await;
    }
}

impl<T> AsyncReceiver<T> for Channel<T> {
    crate::sync::channels::macros::impl_recv_from_recv_in_ptr!();

    crate::sync::channels::macros::impl_recv_with_timeout_from_recv_in_ptr_with_deadline!();

    crate::sync::channels::macros::impl_try_recv_from_recv_in_ptr!();

    #[allow(
        clippy::future_not_send,
        reason = "It is not `Send` only when T is not `Send`, it is fine"
    )]
    fn receiver_close(&self) -> impl Future<Output = ()> {
        close(&self.inner)
    }
}

impl<T> SelectSender for Channel<T> {
    type Data = T;

    generate_send_or_subscribe!();
}

impl<T> SelectReceiver for Channel<T> {
    type Data = T;

    generate_recv_or_subscribe!();
}

unsafe impl<T: Send> Sync for Channel<T> {}
unsafe impl<T: Send> Send for Channel<T> {}
impl<T: UnwindSafe> UnwindSafe for Channel<T> {}
impl<T: RefUnwindSafe> RefUnwindSafe for Channel<T> {}

impl<T> Drop for Channel<T> {
    fn drop(&mut self) {
        let inner = &mut *self.inner.get_mut();

        if unlikely(!inner.is_closed) {
            close_with_lock(inner);
        }
    }
}

// endregion

/// ```fail_compile
/// use std::marker::PhantomData;
/// use orengine::sync::{AsyncChannel, AsyncSender, Channel};
/// use orengine::yield_now;
///
/// fn check_send<T: Send>(value: T) -> T { value }
///
/// struct NonSend {
///     value: i32,
///     // impl !Send
///     no_send_marker: PhantomData<*const ()>,
/// }
///
/// async fn test() {
///     let channel = Channel::bounded(1);
///
///     check_send(channel.send(NonSend { value: 1, no_send_marker: PhantomData })).await;
/// }
/// ```
///
/// ```fail_compile
/// use std::marker::PhantomData;
/// use orengine::sync::{AsyncChannel, AsyncReceiver, Channel};
/// use orengine::yield_now;
///
/// fn check_send<T: Send>(value: T) -> T { value }
///
/// struct NonSend {
///     value: i32,
///     // impl !Send
///     no_send_marker: PhantomData<*const ()>,
/// }
///
/// async fn test() {
///     let channel = Channel::<NonSend>::bounded(1);
///
///     check_send(channel.recv().await);
/// }
/// ```
///
/// ```rust
/// use std::marker::PhantomData;
/// use orengine::sync::{AsyncChannel, AsyncSender, Channel};
/// use orengine::yield_now;
///
/// fn check_send<T: Send>(value: T) -> T { value }
///
/// async fn test() {
///     let channel = Channel::bounded(1);
///
///     check_send(channel.send(1)).await;
/// }
/// ```
///
/// ```rust
/// use std::marker::PhantomData;
/// use orengine::sync::{AsyncChannel, AsyncReceiver, Channel};
/// use orengine::yield_now;
///
/// fn check_send<T: Send>(value: T) -> T { value }
///
/// async fn test() {
///     let channel = Channel::<usize>::bounded(1);
///
///     check_send(channel.recv().await);
/// }
/// ```
#[allow(dead_code, reason = "It is used only in compile tests")]
fn test_compile_shared_channel() {}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use crate as orengine;
    use crate::sleep;
    use crate::sync::{
        AsyncChannel, AsyncReceiver, AsyncSender, AsyncWaitGroup, Channel, RecvErr, RecvTimeoutErr,
        SendErr, SendTimeoutErr, TryRecvErr, TrySendErr, WaitGroup,
    };
    use crate::test::sched_future_to_another_thread;
    use crate::utils::droppable_element::DroppableElement;
    use crate::utils::{Ptr, SpinLock};

    #[orengine::test::test_shared]
    fn test_zero_capacity_shared_channel() {
        let ch = Arc::new(Channel::bounded(0));
        let ch_clone = ch.clone();

        sched_future_to_another_thread(async move {
            ch_clone.send(1).await.unwrap();
            ch_clone.send(2).await.unwrap();
            ch_clone.receiver_close().await;
        });

        let res = ch.recv().await.unwrap();
        assert_eq!(res, 1);

        sleep(Duration::from_millis(1)).await;

        let res = ch.recv().await.unwrap();
        assert_eq!(res, 2);

        match ch.send(2).await.expect_err("should be closed") {
            SendErr::Closed(value) => assert_eq!(value, 2),
        };
    }

    #[orengine::test::test_shared]
    fn test_shared_channel_try() {
        let ch = Channel::bounded(1);

        assert!(matches!(
            ch.try_recv().expect_err("should be empty"),
            TryRecvErr::Empty
        ),);
        assert!(ch.try_send(1).is_ok(), "should be empty");
        assert_eq!(
            ch.try_recv().expect("should be not empty"),
            1,
            "should be not empty"
        );
        assert!(ch.try_send(2).is_ok(), "should be empty");
        match ch.try_send(3).expect_err("should be full") {
            TrySendErr::Full(value) => {
                assert_eq!(value, 3);
            }
            TrySendErr::Locked(_) => {
                panic!("should not be locked")
            }
            TrySendErr::Closed(_) => {
                panic!("should not be closed")
            }
        }

        ch.close().await;

        assert!(
            matches!(
                ch.try_recv().expect_err("should be closed"),
                TryRecvErr::Closed
            ),
            "should be closed"
        );
        match ch.try_send(4).expect_err("should be closed") {
            TrySendErr::Full(_) => {
                panic!("should be not full")
            }
            TrySendErr::Locked(_) => {
                panic!("should not be locked")
            }
            TrySendErr::Closed(value) => {
                assert_eq!(value, 4);
            }
        }
    }

    const N: usize = 10_025;

    #[orengine::test::test_shared]
    fn test_shared_channel() {
        let ch = Arc::new(Channel::bounded(N));
        let wg = Arc::new(WaitGroup::new());
        let ch_clone = ch.clone();
        let wg_clone = wg.clone();

        wg.add(N);

        sched_future_to_another_thread(async move {
            for i in 0..N {
                ch_clone.send(i).await.unwrap();
            }

            wg_clone.wait().await;
            ch_clone.receiver_close().await;
        });

        for i in 0..N {
            let res = ch.recv().await.unwrap();
            assert_eq!(res, i);
            wg.done();
        }

        assert!(
            matches!(
                ch.recv().await.expect_err("should be closed"),
                RecvErr::Closed
            ),
            "should be closed"
        );
    }

    #[orengine::test::test_shared]
    fn test_shared_channel_wait_recv() {
        let ch = Arc::new(Channel::bounded(1));
        let ch_clone = ch.clone();

        sched_future_to_another_thread(async move {
            sleep(Duration::from_millis(1)).await;
            ch_clone.send(1).await.unwrap();
        });

        let res = ch.recv().await.unwrap();
        assert_eq!(res, 1);
    }

    #[orengine::test::test_shared]
    fn test_shared_channel_wait_send() {
        let ch = Arc::new(Channel::bounded(1));
        let ch_clone = ch.clone();

        sched_future_to_another_thread(async move {
            ch_clone.send(1).await.unwrap();
            ch_clone.send(2).await.unwrap();

            sleep(Duration::from_millis(1)).await;

            ch_clone.receiver_close().await;
        });

        sleep(Duration::from_millis(1)).await;

        let res = ch.recv().await.unwrap();
        assert_eq!(res, 1);
        let res = ch.recv().await.unwrap();
        assert_eq!(res, 2);

        let _ = ch.send(3).await;
        match ch.send(4).await.expect_err("should be closed") {
            SendErr::Closed(value) => assert_eq!(value, 4),
        };
    }

    #[orengine::test::test_shared]
    fn test_unbounded_shared_channel() {
        let ch = Arc::new(Channel::unbounded());
        let wg = Arc::new(WaitGroup::new());
        let ch_clone = ch.clone();
        let wg_clone = wg.clone();

        wg.inc();
        sched_future_to_another_thread(async move {
            for i in 0..N {
                ch_clone.send(i).await.unwrap();
            }

            wg_clone.wait().await;

            ch_clone.receiver_close().await;
        });

        for i in 0..N {
            let res = ch.recv().await.unwrap();
            assert_eq!(res, i);
        }

        wg.done();

        assert!(
            matches!(
                ch.recv().await.expect_err("should be closed"),
                RecvErr::Closed
            ),
            "should be closed"
        );
    }

    #[orengine::test::test_shared]
    fn test_drop_shared_channel() {
        let dropped = Arc::new(SpinLock::new(Vec::new()));
        let channel = Channel::bounded(1);

        let _ = channel
            .send(DroppableElement::new(1, dropped.clone()))
            .await;
        let mut prev_elem = DroppableElement::new(2, dropped.clone());

        drop(prev_elem);

        prev_elem = channel.recv().await.unwrap();

        assert_eq!(prev_elem.value, 1);
        assert_eq!(dropped.lock().as_slice(), [2]);

        let _ = channel
            .send(DroppableElement::new(3, dropped.clone()))
            .await;
        unsafe { channel.recv_in_ptr(Ptr::from(&mut prev_elem)).await }.unwrap();
        assert_eq!(prev_elem.value, 3);
        assert_eq!(dropped.lock().as_slice(), [2]);

        channel.receiver_close().await;
        match channel
            .send(DroppableElement::new(5, dropped.clone()))
            .await
            .expect_err("should be closed")
        {
            SendErr::Closed(elem) => {
                assert_eq!(elem.value, 5);
                assert_eq!(dropped.lock().as_slice(), [2]);
            }
        }
        assert_eq!(dropped.lock().as_slice(), [2, 5]);
    }

    #[orengine::test::test_shared]
    fn test_shared_channel_timeout() {
        let chan = Channel::bounded(1);

        let failed_recv_res = chan.recv_timeout(Duration::from_micros(100)).await;

        assert!(matches!(failed_recv_res, Err(RecvTimeoutErr::Timeout)));

        chan.send_timeout(1, Duration::from_micros(100))
            .await
            .unwrap();

        let recv_res = chan.recv_timeout(Duration::from_micros(100)).await;

        assert!(matches!(recv_res, Ok(1)));

        chan.send_timeout(2, Duration::from_micros(100))
            .await
            .unwrap();

        let failed_send_res = chan.send_timeout(3, Duration::from_micros(100)).await;

        assert!(matches!(failed_send_res, Err(SendTimeoutErr::Timeout(3))));
    }
}

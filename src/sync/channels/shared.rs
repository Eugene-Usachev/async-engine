use crate::panic_if_local_in_future;
use crate::runtime::call::Call;
use crate::runtime::{local_executor, IsLocal, Task};
use crate::sync::channels::select::SelectNonBlockingBranchResult;
use crate::sync::channels::states::{PtrToCallState, RecvCallState, SendCallState};
use crate::sync::channels::waiting_task::waiting_task::WaitingTask;
use crate::sync::channels::waiting_task::waiting_task_deque::WaitingTaskSharedDequeGuard;
use crate::sync::channels::waiting_task::{PopIfAcquiredResult, TaskInSelectBranch};
use crate::sync::channels::{SelectReceiver, SelectSender};
use crate::sync::mutexes::naive_shared::NaiveMutex;
use crate::sync::{
    AsyncChannel, AsyncMutex, AsyncReceiver, AsyncSender, RecvErr, SendErr, TryRecvErr, TrySendErr,
};
use std::collections::VecDeque;
use std::future::Future;
use std::mem::ManuallyDrop;
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::ptr::{copy_nonoverlapping, NonNull};
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

/// Returns `Poll::Pending` and releases the lock by invokes
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
    call_state: SendCallState,
    #[cfg(debug_assertions)]
    was_awaited: bool,
}

impl<'future, T> WaitSend<'future, T> {
    /// Creates a new [`WaitSend`].
    #[inline]
    fn new(value: T, inner: &'future NaiveMutex<Inner<T>>) -> Self {
        Self {
            inner,
            call_state: SendCallState::FirstCall,
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
            SendCallState::FirstCall => {
                let mut inner_lock = acquire_lock!(this.inner, unsafe { Task::from_context(cx) });
                if inner_lock.is_closed {
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
                            call_state.write(RecvCallState::WokenToReturnReady);
                        });
                if was_written {
                    mem::forget(inner_lock); // Was released above

                    return Poll::Ready(Ok(()));
                }

                let len = inner_lock.storage.len();
                if len >= inner_lock.capacity {
                    inner_lock.deque.push_back_sender(WaitingTask::common(
                        unsafe { Task::from_context(cx) },
                        PtrToCallState::from(&mut this.call_state),
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
            SendCallState::WokenToReturnReady => Poll::Ready(Ok(())),
            SendCallState::WokenByClose => Poll::Ready(Err(SendErr::Closed(unsafe {
                ManuallyDrop::take(&mut this.value)
            }))),
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

/// This struct represents a future that waits for a value to be
/// received from the [`channel`](Channel).
///
/// When the future is polled, it either receives the value immediately (if available) or
/// gets parked in the list of waiting receivers.
#[repr(C)]
pub struct WaitRecv<'future, T> {
    inner: &'future NaiveMutex<Inner<T>>,
    slot: *mut T,
    call_state: RecvCallState,
}

impl<'future, T> WaitRecv<'future, T> {
    /// Creates a new [`WaitRecv`].
    #[inline]
    fn new(inner: &'future NaiveMutex<Inner<T>>, slot: *mut T) -> Self {
        Self {
            inner,
            call_state: RecvCallState::FirstCall,
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
            RecvCallState::FirstCall => {
                let mut inner_lock = acquire_lock!(this.inner, Task::from_context(cx));
                if inner_lock.is_closed {
                    return Poll::Ready(Err(RecvErr::Closed));
                }

                if inner_lock.storage.is_empty() {
                    let was_written = inner_lock.deque.try_pop_front_sender_and_call(
                        |call_state, value| unsafe {
                            this.inner.unlock(); // Release the lock here to improve performance

                            copy_nonoverlapping(value.as_ptr(), this.slot, 1);
                            call_state.write(SendCallState::WokenToReturnReady);
                        },
                    );
                    if was_written {
                        mem::forget(inner_lock); // Was released above

                        return Poll::Ready(Ok(()));
                    }

                    inner_lock.deque.push_back_receiver(WaitingTask::common(
                        unsafe { Task::from_context(cx) },
                        PtrToCallState::from(&mut this.call_state),
                        NonNull::from(unsafe { &mut *this.slot }),
                    ));

                    return_pending_and_release_lock!(local_executor(), inner_lock);
                }

                unsafe {
                    this.slot
                        .write(inner_lock.storage.pop_front().unwrap_unchecked());
                }

                let storage_ref = &mut inner_lock.get_mut().storage;

                let was_written =
                    inner_lock
                        .deque
                        .try_pop_front_sender_and_call(|call_state, value| unsafe {
                            storage_ref.push_back(value.read());

                            this.inner.unlock(); // Release the lock here to improve performance

                            call_state.write(SendCallState::WokenToReturnReady);
                        });
                if was_written {
                    mem::forget(inner_lock); // Was released above
                }

                Poll::Ready(Ok(()))
            }

            RecvCallState::WokenToReturnReady => Poll::Ready(Ok(())),

            RecvCallState::WokenByClose => Poll::Ready(Err(RecvErr::Closed)),
        }
    }
}

unsafe impl<T: Send> Send for WaitRecv<'_, T> {}
impl<T: UnwindSafe> UnwindSafe for WaitRecv<'_, T> {}
impl<T: RefUnwindSafe> RefUnwindSafe for WaitRecv<'_, T> {}

// endregion

/// Closes the [`channel`](Channel) and wakes all senders and receivers.
#[inline]
#[allow(
    clippy::future_not_send,
    reason = "It is not `Send` only when T is not `Send`, it is fine"
)]
async fn close<T>(inner: &NaiveMutex<Inner<T>>) {
    let mut inner_lock = inner.lock().await;

    inner_lock.is_closed = true;

    inner_lock.deque.clear_with(
        |call_state, _| unsafe { call_state.write(RecvCallState::WokenByClose) },
        |call_state, _| unsafe { call_state.write(SendCallState::WokenByClose) },
    );
}

macro_rules! generate_try_send {
    () => {
        fn try_send(&self, value: T) -> Result<(), TrySendErr<T>> {
            match self.inner.try_lock() {
                Some(mut inner_lock) => {
                    if inner_lock.is_closed {
                        return Err(TrySendErr::Closed(value));
                    }

                    let was_written = inner_lock.deque.try_pop_front_receiver_and_call(
                        |call_state, slot| unsafe {
                            self.inner.unlock(); // Release the lock here to improve performance

                            ptr::copy_nonoverlapping(&value, slot.as_ptr(), 1);
                            call_state.write(RecvCallState::WokenToReturnReady);
                        },
                    );
                    if was_written {
                        mem::forget(inner_lock); // Was released above

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
            mut state: PtrToCallState,
            mut task_in_select_branch: TaskInSelectBranch,
            _: bool, // always false
        ) -> SelectNonBlockingBranchResult {
            debug_assert!(
                !task_in_select_branch.is_local(),
                "Tried to use `local` task in `select` in a non-local channel."
            );

            let Some(mut inner_lock) = self.inner.try_lock() else {
                return SelectNonBlockingBranchResult::Locked;
            };

            if inner_lock.is_closed {
                return if let Some(task) = task_in_select_branch.acquire_once() {
                    unsafe { state.as_send_and_set_closed() };

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

                        call_state.write(RecvCallState::WokenToReturnReady);
                    },
                    &mut task_in_select_branch,
                );

            match result {
                PopIfAcquiredResult::Ok => {
                    mem::forget(inner_lock); // Was released above

                    return SelectNonBlockingBranchResult::Success;
                }

                PopIfAcquiredResult::NotAcquired => {
                    return SelectNonBlockingBranchResult::AlreadyAcquired
                }

                PopIfAcquiredResult::NoData => {}
            }

            let len = inner_lock.storage.len();
            if len >= inner_lock.capacity {
                inner_lock.deque.push_back_sender(WaitingTask::in_selector(
                    task_in_select_branch,
                    PtrToCallState::from(state),
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
    };
}

macro_rules! generate_try_recv_in {
    () => {
        unsafe fn try_recv_in_ptr(&self, slot: *mut T) -> Result<(), TryRecvErr> {
            match self.inner.try_lock() {
                Some(mut inner_lock) => {
                    if inner_lock.is_closed {
                        return Err(TryRecvErr::Closed);
                    }

                    if inner_lock.storage.len() == 0 {
                        let was_written = inner_lock.deque.try_pop_front_sender_and_call(
                            |call_state, value| unsafe {
                                self.inner.unlock(); // Release the lock here to improve performance

                                copy_nonoverlapping(value.as_ptr(), slot, 1);
                                call_state.write(SendCallState::WokenToReturnReady);
                            },
                        );
                        if was_written {
                            mem::forget(inner_lock); // Was released above

                            return Ok(());
                        }

                        return Err(TryRecvErr::Empty);
                    }

                    unsafe {
                        slot.write(inner_lock.storage.pop_front().unwrap_unchecked());
                    }

                    let storage_ref = &mut inner_lock.get_mut().storage;

                    let was_written = inner_lock.deque.try_pop_front_sender_and_call(
                        |call_state, value| unsafe {
                            storage_ref.push_back(value.read());

                            self.inner.unlock(); // Release the lock here to improve performance

                            call_state.write(SendCallState::WokenToReturnReady);
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
            mut state: PtrToCallState,
            mut task_in_select_branch: TaskInSelectBranch,
            _: bool, // always false
        ) -> SelectNonBlockingBranchResult {
            debug_assert!(
                !task_in_select_branch.is_local(),
                "Tried to use `local` task in `select` in a non-local channel."
            );

            let mut inner_lock = match self.inner.try_lock() {
                Some(inner_lock) => inner_lock,
                None => return SelectNonBlockingBranchResult::Locked,
            };

            if inner_lock.is_closed {
                return match task_in_select_branch.acquire_once() {
                    Some(task) => {
                        unsafe { state.as_recv_and_set_closed() };

                        local_executor().spawn_shared_task(task);

                        SelectNonBlockingBranchResult::Success
                    }
                    None => SelectNonBlockingBranchResult::AlreadyAcquired,
                };
            }

            if inner_lock.storage.len() == 0 {
                let result = inner_lock.deque.try_pop_front_sender_and_call_if_acquired(
                    |call_state, value| unsafe {
                        self.inner.unlock(); // Release the lock here to improve performance

                        copy_nonoverlapping(value.as_ptr(), slot.as_ptr(), 1);

                        call_state.write(SendCallState::WokenToReturnReady);
                    },
                    &mut task_in_select_branch,
                );

                match result {
                    PopIfAcquiredResult::Ok => {
                        mem::forget(inner_lock); // Was released above

                        return SelectNonBlockingBranchResult::Success;
                    }

                    PopIfAcquiredResult::NotAcquired => {
                        return SelectNonBlockingBranchResult::AlreadyAcquired
                    }

                    PopIfAcquiredResult::NoData => {}
                }

                inner_lock
                    .deque
                    .push_back_receiver(WaitingTask::in_selector(
                        task_in_select_branch,
                        PtrToCallState::from(state),
                        slot,
                    ));

                return SelectNonBlockingBranchResult::NotReady;
            }

            match task_in_select_branch.acquire_once() {
                Some(task) => {
                    unsafe { slot.write(inner_lock.storage.pop_front().unwrap_unchecked()) };

                    let storage_ref = &mut inner_lock.get_mut().storage;

                    let was_written = inner_lock.deque.try_pop_front_sender_and_call(
                        |call_state, value| unsafe {
                            storage_ref.push_back(value.read());

                            self.inner.unlock(); // Release the lock here to improve performance

                            call_state.write(SendCallState::WokenToReturnReady);
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

// region sender

/// The `Sender` allows sending values into the [`Channel`].
///
/// When the [`channel`](Channel) is not full, values are sent immediately.
///
/// If the [`channel`](Channel) is full, the sender waits until capacity
/// is available or the [`channel`](Channel) is closed.
///
/// # Example
///
/// ```rust
/// use orengine::sync::{AsyncChannel, AsyncReceiver, AsyncSender};
///
///  async fn foo() {
///     let channel = orengine::sync::Channel::bounded(2); // capacity = 2
///     let (sender, receiver) = channel.split();
///
///     sender.send(1).await.unwrap();
///     let res = receiver.recv().await.unwrap();
///     assert_eq!(res, 1);
/// }
/// ```
pub struct Sender<'channel, T> {
    inner: &'channel NaiveMutex<Inner<T>>,
}

impl<'channel, T> Sender<'channel, T> {
    /// Creates a new [`Sender`].
    #[inline]
    fn new(inner: &'channel NaiveMutex<Inner<T>>) -> Self {
        Self { inner }
    }
}

impl<T> AsyncSender<T> for Sender<'_, T> {
    #[allow(
        clippy::future_not_send,
        reason = "It is not `Send` only when T is not `Send`, it is fine"
    )]
    fn send(&self, value: T) -> impl Future<Output = Result<(), SendErr<T>>> {
        WaitSend::new(value, self.inner)
    }

    generate_try_send!();

    #[allow(
        clippy::future_not_send,
        reason = "It is not `Send` only when T is not `Send`, it is fine"
    )]
    async fn sender_close(&self) {
        close(self.inner).await;
    }
}

impl<T> IsLocal for Sender<'_, T> {
    const IS_LOCAL: bool = false;
}

impl<T> Clone for Sender<'_, T> {
    fn clone(&self) -> Self {
        Sender { inner: self.inner }
    }
}

impl<T> SelectSender for Sender<'_, T> {
    type Data = T;

    generate_send_or_subscribe!();
}

unsafe impl<T: Send> Sync for Sender<'_, T> {}
unsafe impl<T: Send> Send for Sender<'_, T> {}
impl<T: UnwindSafe> UnwindSafe for Sender<'_, T> {}
impl<T: RefUnwindSafe> RefUnwindSafe for Sender<'_, T> {}

// endregion

// region receiver

/// The `Receiver` allows receiving values from the [`Channel`].
///
/// When the [`channel`](Channel) is not empty, values are received immediately.
///
/// If the [`channel`](Channel) is empty, the receiver waits until a value
/// is available or the [`channel`](Channel) is closed.
///
/// # Example
///
/// ```rust
/// use orengine::sync::{AsyncChannel, AsyncReceiver, AsyncSender};
///
/// async fn foo() {
///     let channel = orengine::sync::Channel::bounded(2); // capacity = 2
///     let (sender, receiver) = channel.split();
///
///     sender.send(1).await.unwrap();
///     let res = receiver.recv().await.unwrap();
///     assert_eq!(res, 1);
/// }
pub struct Receiver<'channel, T> {
    inner: &'channel NaiveMutex<Inner<T>>,
}

impl<'channel, T> Receiver<'channel, T> {
    /// Creates a new [`Receiver`].
    #[inline]
    fn new(inner: &'channel NaiveMutex<Inner<T>>) -> Self {
        Self { inner }
    }
}

impl<T> AsyncReceiver<T> for Receiver<'_, T> {
    #[allow(
        clippy::future_not_send,
        reason = "It is not `Send` only when T is not `Send`, it is fine"
    )]
    unsafe fn recv_in_ptr(&self, slot: *mut T) -> impl Future<Output = Result<(), RecvErr>> {
        WaitRecv::new(self.inner, slot)
    }

    generate_try_recv_in!();

    #[allow(
        clippy::future_not_send,
        reason = "It is not `Send` only when T is not `Send`, it is fine"
    )]
    fn receiver_close(&self) -> impl Future<Output = ()> {
        close(self.inner)
    }
}

impl<T> IsLocal for Receiver<'_, T> {
    const IS_LOCAL: bool = false;
}

impl<T> SelectReceiver for Receiver<'_, T> {
    type Data = T;

    generate_recv_or_subscribe!();
}

impl<T> Clone for Receiver<'_, T> {
    fn clone(&self) -> Self {
        Receiver { inner: self.inner }
    }
}

unsafe impl<T: Send> Sync for Receiver<'_, T> {}
unsafe impl<T: Send> Send for Receiver<'_, T> {}
impl<T: UnwindSafe> UnwindSafe for Receiver<'_, T> {}
impl<T: RefUnwindSafe> RefUnwindSafe for Receiver<'_, T> {}

// endregion

// region channel

/// The `Channel` provides an asynchronous communication channel between tasks.
///
/// It supports both [`bounded`](Channel::bounded) and [`unbounded`](Channel::unbounded)
/// channels for sending and receiving values.
///
/// When the [`channel`](Channel) is not empty, values are received immediately else
/// the reception operation is waiting until a value is available or
/// the [`channel`](Channel) is closed.
///
/// When channel is not full, values are sent immediately else
/// the sending operation is waiting until capacity is available or
/// the [`channel`](Channel) is closed.
///
/// # The difference between `Channel` and [`LocalChannel`](crate::sync::LocalChannel)
///
/// The `Channel` works with `shared tasks` and can be shared between threads.
///
/// Read [`Executor`](crate::Executor) for more details.
///
/// # Examples
///
/// ## Don't split
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
///
/// ## Split into receiver and sender
///
/// ```rust
/// use orengine::sync::{AsyncChannel, AsyncReceiver, AsyncSender};
///
///  async fn foo() {
///     let channel = orengine::sync::Channel::bounded(1); // capacity = 1
///     let (sender, receiver) = channel.split();
///
///     sender.send(1).await.unwrap();
///     let res = receiver.recv().await.unwrap();
///     assert_eq!(res, 1);
/// }
/// ```
pub struct Channel<T> {
    inner: NaiveMutex<Inner<T>>,
}

impl<T> AsyncChannel<T> for Channel<T> {
    type Sender<'channel>
        = Sender<'channel, T>
    where
        T: 'channel;
    type Receiver<'channel>
        = Receiver<'channel, T>
    where
        T: 'channel;

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

    fn split(&self) -> (Sender<T>, Receiver<T>) {
        (Sender::new(&self.inner), Receiver::new(&self.inner))
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
    #[allow(
        clippy::future_not_send,
        reason = "It is not `Send` only when T is not `Send`, it is fine"
    )]
    unsafe fn recv_in_ptr(&self, slot: *mut T) -> impl Future<Output = Result<(), RecvErr>> {
        WaitRecv::new(&self.inner, slot)
    }

    generate_try_recv_in!();

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
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering::{Relaxed, SeqCst};
    use std::sync::Arc;
    use std::time::Duration;

    use crate as orengine;
    use crate::sync::{
        AsyncChannel, AsyncReceiver, AsyncSender, AsyncWaitGroup, Channel, RecvErr, SendErr,
        TryRecvErr, TrySendErr, WaitGroup,
    };
    use crate::test::sched_future_to_another_thread;
    use crate::utils::droppable_element::DroppableElement;
    use crate::utils::{get_core_ids, SpinLock};
    use crate::{local_executor, sleep, yield_now};

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
        channel.recv_in(&mut prev_elem).await.unwrap();
        assert_eq!(prev_elem.value, 1);
        assert_eq!(dropped.lock().as_slice(), [2]);

        let _ = channel
            .send(DroppableElement::new(3, dropped.clone()))
            .await;
        unsafe { channel.recv_in_ptr(&mut prev_elem).await }.unwrap();
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
    fn test_drop_shared_channel_split() {
        let channel = Channel::bounded(1);
        let dropped = Arc::new(SpinLock::new(Vec::new()));
        let (sender, receiver) = channel.split();

        let _ = sender.send(DroppableElement::new(1, dropped.clone())).await;
        let mut prev_elem = DroppableElement::new(2, dropped.clone());
        receiver.recv_in(&mut prev_elem).await.unwrap();
        assert_eq!(prev_elem.value, 1);
        assert_eq!(dropped.lock().as_slice(), [2]);

        let _ = sender.send(DroppableElement::new(3, dropped.clone())).await;
        unsafe { receiver.recv_in_ptr(&mut prev_elem).await.unwrap() };
        assert_eq!(prev_elem.value, 3);
        assert_eq!(dropped.lock().as_slice(), [2]);

        sender.sender_close().await;
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

    async fn stress_test(channel: Channel<usize>, count: usize) {
        let channel = Arc::new(channel);
        for _ in 0..20 {
            let wg = Arc::new(WaitGroup::new());
            let sent = Arc::new(AtomicUsize::new(0));
            let received = Arc::new(AtomicUsize::new(0));

            for i in 0..get_core_ids().unwrap().len() * 2 {
                let channel = channel.clone();
                let wg = wg.clone();
                let sent = sent.clone();
                let received = received.clone();
                wg.add(1);

                sched_future_to_another_thread(async move {
                    if i % 2 == 0 {
                        for j in 0..count {
                            channel.send(j).await.unwrap();
                            sent.fetch_add(j, Relaxed);
                        }
                    } else {
                        for _ in 0..count {
                            let res = channel.recv().await.unwrap();
                            received.fetch_add(res, Relaxed);
                        }
                    }

                    wg.done();
                });
            }

            wg.wait().await;
            assert_eq!(sent.load(Relaxed), received.load(Relaxed));
        }
    }

    #[orengine::test::test_shared]
    fn stress_test_bounded_shared_channel() {
        stress_test(Channel::bounded(1024), 100).await;
    }

    #[orengine::test::test_shared]
    fn stress_test_unbounded_shared_channel() {
        stress_test(Channel::unbounded(), 100).await;
    }

    #[orengine::test::test_shared]
    fn stress_test_zero_capacity_shared_channel() {
        stress_test(Channel::bounded(0), 20).await;
    }

    #[allow(clippy::future_not_send, reason = "Because it is test")]
    async fn stress_test_local_channel_try(original_channel: Arc<Channel<usize>>) {
        const PAR: usize = 10;
        const COUNT: usize = 100;

        for _ in 0..100 {
            let original_res = Arc::new(AtomicUsize::new(0));
            let original_wg = Arc::new(WaitGroup::new());

            for i in 0..PAR {
                let wg = original_wg.clone();
                let channel = original_channel.clone();
                wg.inc();

                local_executor().spawn_shared(async move {
                    if i % 2 == 0 {
                        for j in 0..COUNT {
                            loop {
                                match channel.try_send(j) {
                                    Ok(()) => break,
                                    Err(e) => match e {
                                        TrySendErr::Full(_) | TrySendErr::Locked(_) => {
                                            yield_now().await;
                                        }
                                        TrySendErr::Closed(_) => panic!("send failed"),
                                    },
                                }
                            }
                        }
                    } else {
                        for j in 0..COUNT {
                            channel.send(j).await.unwrap();
                        }
                    }

                    wg.done();
                });

                let wg = original_wg.clone();
                let res = original_res.clone();
                let channel = original_channel.clone();
                wg.inc();

                local_executor().spawn_shared(async move {
                    if i % 2 == 0 {
                        for _ in 0..COUNT {
                            loop {
                                match channel.try_recv() {
                                    Ok(v) => {
                                        res.fetch_add(v, SeqCst);
                                        break;
                                    }
                                    Err(e) => match e {
                                        TryRecvErr::Empty | TryRecvErr::Locked => {
                                            yield_now().await;
                                        }
                                        TryRecvErr::Closed => panic!("recv failed"),
                                    },
                                }
                            }
                        }
                    } else {
                        for _ in 0..COUNT {
                            let r = channel.recv().await.unwrap();
                            res.fetch_add(r, SeqCst);
                        }
                    }

                    wg.done();
                });
            }

            original_wg.wait().await;

            assert_eq!(original_res.load(SeqCst), PAR * COUNT * (COUNT - 1) / 2);
        }
    }

    #[orengine::test::test_shared]
    fn stress_test_local_channel_try_unbounded() {
        stress_test_local_channel_try(Arc::new(Channel::unbounded())).await;
    }

    #[orengine::test::test_shared]
    fn stress_test_local_channel_try_bounded() {
        stress_test_local_channel_try(Arc::new(Channel::bounded(1024))).await;
    }
}

use crate::runtime::{IsLocal, Task};
use crate::sync::channels::select::SelectNonBlockingBranchResult;
use crate::sync::channels::state::{CallState, CallStatePtr};
use crate::sync::channels::waiting_task::waiting_task::WaitingTask;
use crate::sync::channels::waiting_task::waiting_task_deque::WaitingTaskLocalDequeGuard;
use crate::sync::channels::waiting_task::{PopIfAcquiredResult, TaskInSelectBranch};
use crate::sync::channels::{SelectReceiver, SelectSender};
use crate::sync::{
    AsyncChannel, AsyncReceiver, AsyncSender, RecvErr, SendErr, TryRecvErr, TrySendErr,
};
use crate::utils::Ptr;
use crate::utils::{unlikely, unreachable_hint};
use crate::{local_executor, panic_if_shared_in_future};
use std::cell::UnsafeCell;
use std::collections::VecDeque;
use std::future::Future;
use std::mem::ManuallyDrop;
use std::ptr;
use std::ptr::NonNull;
use std::task::{Context, Poll};

/// This is the internal data structure for the [`local channel`](LocalChannel).
/// It holds the actual storage for the values and manages the queue of senders and receivers.
#[repr(C)]
struct Inner<T> {
    storage: VecDeque<T>,
    capacity: usize,
    is_closed: bool,
    deque: WaitingTaskLocalDequeGuard<T>,
}

// region futures

/// This struct represents a future that waits for a value to be sent
/// into the [`local channel`](LocalChannel).
///
/// When the future is polled, it either sends the value immediately (if there is capacity) or
/// gets parked in the list of waiting senders.
///
/// # Panics or memory leaks
///
/// If [`WaitLocalSend::poll`] is not called.
#[repr(C)]
pub struct WaitLocalSend<'future, T> {
    inner: &'future mut Inner<T>,
    value: ManuallyDrop<T>,
    call_state: CallState,
    #[cfg(debug_assertions)]
    was_awaited: bool,
}

impl<'future, T> WaitLocalSend<'future, T> {
    /// Creates a new [`WaitLocalSend`].
    #[inline]
    fn new(value: T, inner: &'future mut Inner<T>) -> Self {
        Self {
            inner,
            call_state: CallState::FirstCall,
            value: ManuallyDrop::new(value),
            #[cfg(debug_assertions)]
            was_awaited: false,
        }
    }
}

impl<T> Future for WaitLocalSend<'_, T> {
    type Output = Result<(), SendErr<T>>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        #[cfg(debug_assertions)]
        {
            this.was_awaited = true;
        }
        panic_if_shared_in_future!(cx, "LocalChannel");

        match this.call_state {
            CallState::FirstCall => {
                if unlikely(this.inner.is_closed) {
                    return Poll::Ready(Err(SendErr::Closed(unsafe {
                        ManuallyDrop::take(&mut this.value)
                    })));
                }

                let was_written =
                    this.inner
                        .deque
                        .try_pop_front_receiver_and_call(|call_state, slot| {
                            unsafe {
                                ptr::copy_nonoverlapping(&*this.value, slot.as_ptr(), 1);
                                call_state.write(CallState::WokenToReturnReady);
                            };
                        });
                if was_written {
                    return Poll::Ready(Ok(()));
                }

                let len = this.inner.storage.len();
                if unlikely(len >= this.inner.capacity) {
                    this.inner.deque.push_back_sender(WaitingTask::common(
                        unsafe { Task::from_context(cx) },
                        CallStatePtr::new(&mut this.call_state),
                        NonNull::from(&mut *this.value),
                    ));

                    return Poll::Pending;
                }

                unsafe {
                    this.inner
                        .storage
                        .push_back(ManuallyDrop::take(&mut this.value));
                }
                Poll::Ready(Ok(()))
            }
            CallState::WokenToReturnReady => Poll::Ready(Ok(())),
            CallState::WokenByClose => Poll::Ready(Err(SendErr::Closed(unsafe {
                ManuallyDrop::take(&mut this.value)
            }))),
        }
    }
}

#[cfg(debug_assertions)]
impl<T> Drop for WaitLocalSend<'_, T> {
    fn drop(&mut self) {
        assert!(
            self.was_awaited,
            "WaitLocalSend was not awaited. This will cause a memory leak."
        );
    }
}

/// This struct represents a future that waits for a value to be
/// received from the [`local channel`](LocalChannel).
///
/// When the future is polled, it either receives the value immediately (if available) or
/// gets parked in the list of waiting receivers.
#[repr(C)]
pub struct WaitLocalRecv<'future, T> {
    inner: &'future mut Inner<T>,
    slot: *mut T,
    call_state: CallState,
}

impl<'future, T> WaitLocalRecv<'future, T> {
    /// Creates a new [`WaitLocalRecv`].
    #[inline]
    fn new(inner: &'future mut Inner<T>, slot: *mut T) -> Self {
        Self {
            inner,
            call_state: CallState::FirstCall,
            slot,
        }
    }
}

impl<T> Future for WaitLocalRecv<'_, T> {
    type Output = Result<(), RecvErr>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };

        panic_if_shared_in_future!(cx, "LocalChannel");

        match this.call_state {
            CallState::FirstCall => {
                if unlikely(this.inner.is_closed) {
                    return Poll::Ready(Err(RecvErr::Closed));
                }

                if unlikely(this.inner.storage.is_empty()) {
                    let was_written =
                        this.inner
                            .deque
                            .try_pop_front_sender_and_call(|call_state, value| {
                                unsafe {
                                    ptr::copy_nonoverlapping(value.as_ptr(), this.slot, 1);
                                    call_state.write(CallState::WokenToReturnReady);
                                };
                            });
                    if was_written {
                        return Poll::Ready(Ok(()));
                    }

                    this.inner.deque.push_back_receiver(WaitingTask::common(
                        unsafe { Task::from_context(cx) },
                        CallStatePtr::new(&mut this.call_state),
                        NonNull::from(unsafe { &mut *this.slot }),
                    ));

                    return Poll::Pending;
                }

                unsafe {
                    this.slot
                        .write(this.inner.storage.pop_front().unwrap_unchecked());
                }

                this.inner
                    .deque
                    .try_pop_front_sender_and_call(|call_state, value| unsafe {
                        call_state.write(CallState::WokenToReturnReady);

                        this.inner.storage.push_back(value.read());
                    });

                Poll::Ready(Ok(()))
            }

            CallState::WokenToReturnReady => Poll::Ready(Ok(())),

            CallState::WokenByClose => Poll::Ready(Err(RecvErr::Closed)),
        }
    }
}

// endregion

/// Closes the [`local channel`](LocalChannel) and wakes all senders and receivers.
#[inline]
fn close<T>(inner: &mut Inner<T>) {
    inner.is_closed = true;

    inner.deque.clear();
}

macro_rules! generate_try_send {
    () => {
        fn try_send(&self, value: T) -> Result<(), TrySendErr<T>> {
            let inner = unsafe { &mut *self.inner.get() };
            if unlikely(inner.is_closed) {
                return Err(TrySendErr::Closed(value));
            }

            let was_written = inner
                .deque
                .try_pop_front_receiver_and_call(|call_state, slot| {
                    unsafe {
                        ptr::copy_nonoverlapping(&value, slot.as_ptr(), 1);
                        call_state.write(CallState::WokenToReturnReady);
                    };
                });
            if was_written {
                return Ok(());
            }

            let len = inner.storage.len();
            if len >= inner.capacity {
                return Err(TrySendErr::Full(value));
            }

            inner.storage.push_back(value);

            Ok(())
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
            let inner = unsafe { &mut *self.inner.get() };

            if unlikely(inner.is_closed) {
                return match task_in_select_branch.acquire_once() {
                    // It all is `local`, then other thread can't acquire the task. So, in select
                    // we can definitely acquire it.
                    Some(task) => {
                        state.set_to_closed();

                        local_executor().exec_task(task);

                        SelectNonBlockingBranchResult::Success
                    }
                    None => unreachable_hint(),
                };
            }

            let result = inner.deque.try_pop_front_receiver_and_call_if_acquired(
                |call_state, slot| {
                    unsafe {
                        ptr::copy_nonoverlapping(data.as_ptr(), slot.as_ptr(), 1);

                        call_state.write(CallState::WokenToReturnReady);
                    };
                },
                task_in_select_branch,
            );

            match result {
                PopIfAcquiredResult::NoData(task_in_select_branch) => {
                    let len = inner.storage.len();
                    if unlikely(len >= inner.capacity) {
                        inner.deque.push_back_sender(WaitingTask::in_selector(
                            task_in_select_branch,
                            state,
                            data,
                        ));

                        return SelectNonBlockingBranchResult::NotReady;
                    }

                    match task_in_select_branch.acquire_once() {
                        // It all is `local`, then other thread can't acquire the task. So, in select
                        // we can definitely acquire it.
                        Some(task) => {
                            inner.storage.push_back(unsafe { data.read() });

                            local_executor().exec_task(task);

                            SelectNonBlockingBranchResult::Success
                        }
                        None => unreachable_hint(),
                    }
                }

                PopIfAcquiredResult::Ok => SelectNonBlockingBranchResult::Success,

                _ => unreachable_hint(),
            }
        }
    };
}

macro_rules! generate_try_recv_in_ptr {
    () => {
        unsafe fn try_recv_in_ptr(&self, slot: Ptr<T>) -> Result<(), TryRecvErr> {
            let inner = unsafe { &mut *self.inner.get() };
            if unlikely(inner.is_closed) {
                return Err(TryRecvErr::Closed);
            }

            if unlikely(inner.storage.len() == 0) {
                let was_written = inner
                    .deque
                    .try_pop_front_sender_and_call(|call_state, value| {
                        unsafe {
                            ptr::copy_nonoverlapping(value.as_ptr(), slot.as_ptr(), 1);
                            call_state.write(CallState::WokenToReturnReady);
                        };
                    });
                if was_written {
                    return Ok(());
                }

                return Err(TryRecvErr::Empty);
            }

            unsafe {
                slot.write(inner.storage.pop_front().unwrap_unchecked());
            };

            inner
                .deque
                .try_pop_front_sender_and_call(|call_state, value| unsafe {
                    call_state.write(CallState::WokenToReturnReady);
                    inner.storage.push_back(value.read());
                });

            Ok(())
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
            let inner = unsafe { &mut *self.inner.get() };
            if unlikely(inner.is_closed) {
                return match task_in_select_branch.acquire_once() {
                    Some(task) => {
                        state.set_to_closed();

                        local_executor().exec_task(task);

                        SelectNonBlockingBranchResult::Success
                    }
                    None => unreachable_hint(),
                };
            }

            if unlikely(inner.storage.len() == 0) {
                let result = inner.deque.try_pop_front_sender_and_call_if_acquired(
                    |call_state, value| {
                        unsafe {
                            ptr::copy_nonoverlapping(value.as_ptr(), slot.as_ptr(), 1);

                            call_state.write(CallState::WokenToReturnReady);
                        };
                    },
                    task_in_select_branch,
                );

                return match result {
                    PopIfAcquiredResult::Ok => SelectNonBlockingBranchResult::Success,

                    PopIfAcquiredResult::NoData(task_in_select_branch) => {
                        inner.deque.push_back_receiver(WaitingTask::in_selector(
                            task_in_select_branch,
                            state,
                            slot,
                        ));

                        SelectNonBlockingBranchResult::NotReady
                    }

                    _ => unreachable_hint(),
                };
            }

            match task_in_select_branch.acquire_once() {
                Some(task) => {
                    unsafe { slot.write(inner.storage.pop_front().unwrap_unchecked()) };

                    local_executor().exec_task(task);

                    inner
                        .deque
                        .try_pop_front_sender_and_call(|call_state, value| unsafe {
                            call_state.write(CallState::WokenToReturnReady);

                            inner.storage.push_back(value.read());
                        });

                    SelectNonBlockingBranchResult::Success
                }
                None => unreachable_hint(),
            }
        }
    };
}

// region sender

/// The `LocalSender` allows sending values into the [`LocalChannel`].
///
/// When the [`local channel`](LocalChannel) is not full, values are sent immediately.
///
/// If the [`local channel`](LocalChannel) is full, the sender waits until capacity
/// is available or the [`local channel`](LocalChannel) is closed.
///
/// # Example
///
/// ```rust
/// use orengine::sync::{AsyncChannel, AsyncReceiver, AsyncSender};
///
/// async fn foo() {
///     let channel = orengine::sync::LocalChannel::bounded(2); // capacity = 2
///     let (sender, receiver) = channel.split();
///
///     sender.send(1).await.unwrap();
///     let res = receiver.recv().await.unwrap();
///     assert_eq!(res, 1);
/// }
/// ```
pub struct LocalSender<'channel, T> {
    inner: &'channel UnsafeCell<Inner<T>>,
    // impl !Send
    no_send_marker: std::marker::PhantomData<*const ()>,
}

impl<'channel, T> LocalSender<'channel, T> {
    /// Creates a new [`LocalSender`].
    #[inline]
    fn new(inner: &'channel UnsafeCell<Inner<T>>) -> Self {
        Self {
            inner,
            no_send_marker: std::marker::PhantomData,
        }
    }
}

impl<T> IsLocal for LocalSender<'_, T> {
    const IS_LOCAL: bool = true;
}

impl<T> AsyncSender<T> for LocalSender<'_, T> {
    #[allow(clippy::future_not_send, reason = "Because it is `local`")]
    fn send(&self, value: T) -> impl Future<Output = Result<(), SendErr<T>>> {
        WaitLocalSend::new(value, unsafe { &mut *self.inner.get() })
    }

    generate_try_send!();

    async fn sender_close(&self) {
        let inner = unsafe { &mut *self.inner.get() };
        close(inner);
    }
}

impl<T> SelectSender for LocalSender<'_, T> {
    type Data = T;

    generate_send_or_subscribe!();
}

impl<T> Clone for LocalSender<'_, T> {
    fn clone(&self) -> Self {
        LocalSender {
            inner: self.inner,
            no_send_marker: std::marker::PhantomData,
        }
    }
}

unsafe impl<T> Sync for LocalSender<'_, T> {}

// endregion

// region receiver

/// The `LocalReceiver` allows receiving values from the [`LocalChannel`].
///
/// When the [`local channel`](LocalChannel) is not empty, values are received immediately.
///
/// If the [`local channel`](LocalChannel) is empty, the receiver waits until a value
/// is available or the [`local channel`](LocalChannel) is closed.
///
/// # Example
///
/// ```rust
/// use orengine::sync::{AsyncChannel, AsyncReceiver, AsyncSender};
///
/// async fn foo() {
///     let channel = orengine::sync::LocalChannel::bounded(2); // capacity = 2
///     let (sender, receiver) = channel.split();
///
///     sender.send(1).await.unwrap();
///     let res = receiver.recv().await.unwrap();
///     assert_eq!(res, 1);
/// }
/// ```
pub struct LocalReceiver<'channel, T> {
    inner: &'channel UnsafeCell<Inner<T>>,
    // impl !Send
    no_send_marker: std::marker::PhantomData<*const ()>,
}

impl<'channel, T> LocalReceiver<'channel, T> {
    /// Creates a new [`LocalReceiver`].
    #[inline]
    fn new(inner: &'channel UnsafeCell<Inner<T>>) -> Self {
        Self {
            inner,
            no_send_marker: std::marker::PhantomData,
        }
    }
}

impl<T> AsyncReceiver<T> for LocalReceiver<'_, T> {
    #[allow(clippy::future_not_send, reason = "Because it is `local`")]
    unsafe fn recv_in_ptr(&self, slot: Ptr<T>) -> impl Future<Output = Result<(), RecvErr>> {
        WaitLocalRecv::new(unsafe { &mut *self.inner.get() }, slot.as_ptr())
    }

    generate_try_recv_in_ptr!();

    async fn receiver_close(&self) {
        let inner = unsafe { &mut *self.inner.get() };
        close(inner);
    }
}

impl<T> IsLocal for LocalReceiver<'_, T> {
    const IS_LOCAL: bool = true;
}

impl<T> SelectReceiver for LocalReceiver<'_, T> {
    type Data = T;

    generate_recv_or_subscribe!();
}

impl<T> Clone for LocalReceiver<'_, T> {
    fn clone(&self) -> Self {
        LocalReceiver {
            inner: self.inner,
            no_send_marker: std::marker::PhantomData,
        }
    }
}

unsafe impl<T> Sync for LocalReceiver<'_, T> {}

// endregion

// region channel

/// The `LocalChannel` provides an asynchronous communication channel between
/// tasks running on the same thread.
///
/// It supports both [`bounded`](LocalChannel::bounded) and [`unbounded`](LocalChannel::unbounded)
/// channels for sending and receiving values.
///
/// When the [`local channel`](LocalChannel) is not empty, values are received immediately else
/// the reception operation is waiting until a value is available or
/// the [`local channel`](LocalChannel) is closed.
///
/// When channel is not full, values are sent immediately else
/// the sending operation is waiting until capacity is available or
/// the [`local channel`](LocalChannel) is closed.
///
/// # The difference between `LocalChannel` and [`Channel`](crate::sync::Channel)
///
/// The `LocalChannel` works with `local tasks`.
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
/// async fn foo() {
///     let channel = orengine::sync::LocalChannel::bounded(1); // capacity = 1
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
/// async fn foo() {
///     let channel = orengine::sync::LocalChannel::bounded(1); // capacity = 1
///     let (sender, receiver) = channel.split();
///
///     sender.send(1).await.unwrap();
///     let res = receiver.recv().await.unwrap();
///     assert_eq!(res, 1);
/// }
/// ```
pub struct LocalChannel<T> {
    inner: UnsafeCell<Inner<T>>,
    // impl !Send
    no_send_marker: std::marker::PhantomData<*const ()>,
}

impl<T> LocalChannel<T> {
    /// Returns current len, number of receivers and number of senders.
    #[inline]
    pub fn fullness_state(&self) -> (usize, usize, usize) {
        let inner = unsafe { &mut *self.inner.get() };
        let number_of_senders_or_receivers = inner.deque.number_of_senders_or_receivers();
        let len = number_of_senders_or_receivers.unsigned_abs();

        if number_of_senders_or_receivers > 0 {
            (len, len, 0)
        } else {
            (len, 0, len)
        }
    }
}

impl<T> AsyncChannel<T> for LocalChannel<T> {
    type Sender<'channel>
        = LocalSender<'channel, T>
    where
        Self: 'channel;
    type Receiver<'channel>
        = LocalReceiver<'channel, T>
    where
        Self: 'channel;

    #[inline]
    fn bounded(capacity: usize) -> Self {
        Self {
            inner: UnsafeCell::new(Inner {
                storage: VecDeque::with_capacity(capacity),
                capacity,
                is_closed: false,
                deque: WaitingTaskLocalDequeGuard::new(),
            }),
            no_send_marker: std::marker::PhantomData,
        }
    }

    #[inline]
    fn unbounded() -> Self {
        Self {
            inner: UnsafeCell::new(Inner {
                storage: VecDeque::with_capacity(0),
                capacity: usize::MAX,
                is_closed: false,
                deque: WaitingTaskLocalDequeGuard::new(),
            }),
            no_send_marker: std::marker::PhantomData,
        }
    }

    #[inline]
    fn split(&self) -> (Self::Sender<'_>, Self::Receiver<'_>) {
        (
            LocalSender::new(&self.inner),
            LocalReceiver::new(&self.inner),
        )
    }

    async fn close(&self) {
        let inner = unsafe { &mut *self.inner.get() };
        close(inner);
    }
}

impl<T> IsLocal for LocalChannel<T> {
    const IS_LOCAL: bool = true;
}

impl<T> AsyncReceiver<T> for LocalChannel<T> {
    #[allow(clippy::future_not_send, reason = "Because it is `local`")]
    unsafe fn recv_in_ptr(&self, slot: Ptr<T>) -> impl Future<Output = Result<(), RecvErr>> {
        WaitLocalRecv::new(unsafe { &mut *self.inner.get() }, slot.as_ptr())
    }

    generate_try_recv_in_ptr!();

    async fn receiver_close(&self) {
        let inner = unsafe { &mut *self.inner.get() };
        close(inner);
    }
}

impl<T> AsyncSender<T> for LocalChannel<T> {
    #[allow(clippy::future_not_send, reason = "Because it is `local`")]
    fn send(&self, value: T) -> impl Future<Output = Result<(), SendErr<T>>> {
        WaitLocalSend::new(value, unsafe { &mut *self.inner.get() })
    }

    generate_try_send!();

    async fn sender_close(&self) {
        let inner = unsafe { &mut *self.inner.get() };
        close(inner);
    }
}

impl<T> SelectReceiver for LocalChannel<T> {
    type Data = T;

    generate_recv_or_subscribe!();
}

impl<T> SelectSender for LocalChannel<T> {
    type Data = T;

    generate_send_or_subscribe!();
}

unsafe impl<T> Sync for LocalChannel<T> {}

impl<T> Drop for LocalChannel<T> {
    fn drop(&mut self) {
        let inner = unsafe { &mut *self.inner.get() };

        if unlikely(!inner.is_closed) {
            close(inner);
        }
    }
}

// endregion

/// ```fail_compile
/// use std::marker::PhantomData;
/// use orengine::sync::{AsyncChannel, AsyncSender, LocalChannel};
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
///     let channel = LocalChannel::bounded(1);
///
///     check_send(channel.send(NonSend { value: 1, no_send_marker: PhantomData })).await;
/// }
/// ```
///
/// ```fail_compile
/// use std::marker::PhantomData;
/// use orengine::sync::{AsyncChannel, AsyncReceiver, LocalChannel};
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
///     let channel = LocalChannel::<NonSend>::bounded(1);
///
///     check_send(channel.recv().await);
/// }
/// ```
///
/// ```fail_compile
/// use std::marker::PhantomData;
/// use orengine::sync::{AsyncChannel, AsyncSender, LocalChannel};
/// use orengine::yield_now;
///
/// fn check_send<T: Send>(value: T) -> T { value }
///
/// async fn test() {
///     let channel = LocalChannel::bounded(1);
///
///     check_send(channel.send(1)).await;
/// }
/// ```
///
/// ```fail_compile
/// use std::marker::PhantomData;
/// use orengine::sync::{AsyncChannel, AsyncReceiver, LocalChannel};
/// use orengine::yield_now;
///
/// fn check_send<T: Send>(value: T) -> T { value }
///
/// async fn test() {
///     let channel = LocalChannel::<usize>::bounded(1);
///
///     check_send(channel.recv().await);
/// }
/// ```
#[allow(dead_code, reason = "It is used only in compile tests")]
fn test_compile_local_channel() {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as orengine;
    use crate::sync::{RecvErr, TryRecvErr};
    use crate::utils::droppable_element::DroppableElement;
    use crate::utils::SpinLock;
    use crate::yield_now;
    use std::rc::Rc;
    use std::sync::Arc;

    #[orengine::test::test_local]
    fn test_local_zero_capacity() {
        let chan = Rc::new(LocalChannel::bounded(0));
        let chan_clone = chan.clone();

        local_executor().spawn_local(async move {
            chan_clone.send(1).await.unwrap();

            yield_now().await;

            chan_clone.send(2).await.unwrap();
            chan_clone.close().await;
        });

        let res = chan.recv().await.unwrap();
        assert_eq!(res, 1);
        let res = chan.recv().await.unwrap();
        assert_eq!(res, 2);

        match chan.send(2).await.expect_err("should be closed") {
            SendErr::Closed(value) => assert_eq!(value, 2),
        };
    }

    #[orengine::test::test_local]
    fn test_local_unbounded() {
        let chan = Rc::new(LocalChannel::unbounded());
        let chan_clone = chan.clone();

        local_executor().spawn_local(async move {
            chan_clone.send(1).await.unwrap();

            yield_now().await;

            for i in 2..100 {
                chan_clone.send(i).await.unwrap();
            }

            yield_now().await; // Drops streak (maximum 63 executions)

            chan_clone.close().await;
        });

        for i in 1..100 {
            let res = chan.recv().await.unwrap();
            assert_eq!(res, i);
        }

        assert!(
            matches!(
                chan.recv().await.expect_err("should be closed"),
                RecvErr::Closed
            ),
            "should be closed"
        );
    }

    #[orengine::test::test_local]
    fn test_local_channel_try() {
        let chan = LocalChannel::bounded(1);

        assert!(
            matches!(
                chan.try_recv().expect_err("should be closed"),
                TryRecvErr::Empty
            ),
            "should be empty"
        );
        assert!(chan.try_send(1).is_ok(), "should be empty");
        assert!(
            matches!(chan.try_recv().expect("should be not empty"), 1),
            "should be not empty"
        );
        assert!(chan.try_send(2).is_ok(), "should be empty");
        match chan.try_send(3).expect_err("should be full") {
            TrySendErr::Full(value) => {
                assert_eq!(value, 3);
            }
            TrySendErr::Locked(_) => {
                panic!("unreachable")
            }
            TrySendErr::Closed(_) => {
                panic!("should not be closed")
            }
        }

        chan.close().await;

        assert!(
            matches!(
                chan.try_recv().expect_err("should be closed"),
                TryRecvErr::Closed
            ),
            "should be closed"
        );
        match chan.try_send(4).expect_err("should be closed") {
            TrySendErr::Full(_) => {
                panic!("should be not full")
            }
            TrySendErr::Locked(_) => {
                panic!("unreachable")
            }
            TrySendErr::Closed(value) => {
                assert_eq!(value, 4);
            }
        }
    }

    const N: usize = 125;

    // case 1 - send N and recv N. No wait
    // case 2 - send N and recv (N + 1). Wait for recv
    // case 3 - send (N + 1) and recv N. Wait for send
    // case 4 - send (N + 1) and recv (N + 1). Wait for send and wait for recv

    #[orengine::test::test_local]
    fn test_local_channel_case1() {
        let chan = Rc::new(LocalChannel::bounded(N));
        let chan_clone = chan.clone();

        local_executor().spawn_local(async move {
            for i in 0..N {
                chan_clone.send(i).await.unwrap();
            }

            yield_now().await;

            chan_clone.close().await;
        });

        for i in 0..N {
            let res = chan.recv().await.unwrap();
            assert_eq!(res, i);
        }

        assert!(
            matches!(
                chan.recv().await.expect_err("should be closed"),
                RecvErr::Closed
            ),
            "should be closed"
        );
    }

    #[orengine::test::test_local]
    fn test_local_channel_case2() {
        let chan = Rc::new(LocalChannel::bounded(N));
        let chan_clone = chan.clone();

        local_executor().spawn_local(async move {
            for i in 0..=N {
                let res = chan_clone.recv().await.unwrap();
                assert_eq!(res, i);
            }

            chan_clone.close().await;
        });

        for i in 0..N {
            chan.send(i).await.unwrap();
        }

        yield_now().await;

        chan.send(N).await.unwrap();
    }

    #[orengine::test::test_local]
    fn test_local_channel_case3() {
        let chan = Rc::new(LocalChannel::bounded(N));
        let chan_clone = chan.clone();

        local_executor().spawn_local(async move {
            for i in 0..N {
                let res = chan_clone.recv().await.unwrap();
                assert_eq!(res, i);
            }

            yield_now().await;

            let res = chan_clone.recv().await.unwrap();
            assert_eq!(res, N);
        });

        for i in 0..=N {
            chan.send(i).await.unwrap();
        }
    }

    #[orengine::test::test_local]
    fn test_local_channel_case4() {
        let chan = Rc::new(LocalChannel::bounded(N));
        let chan_clone = chan.clone();

        local_executor().spawn_local(async move {
            for i in 0..=N {
                let res = chan_clone.recv().await.unwrap();
                assert_eq!(res, i);
            }
        });

        for i in 0..=N {
            chan.send(i).await.unwrap();
        }
    }

    #[orengine::test::test_local]
    fn test_local_channel_split() {
        let chan = LocalChannel::bounded(N);
        let (tx, rx) = chan.split();

        local_executor().spawn_local(async {
            for i in 0..=N * 2 {
                let res = rx.recv().await.unwrap();
                assert_eq!(res, i);
            }
        });

        for i in 0..=N * 3 {
            tx.send(i).await.unwrap();
        }
    }

    #[orengine::test::test_local]
    fn test_drop_local_channel() {
        let dropped = Arc::new(SpinLock::new(Vec::new()));
        let channel = LocalChannel::bounded(1);

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
        unsafe {
            channel
                .recv_in_ptr(Ptr::from(&mut prev_elem))
                .await
                .unwrap();
        };
        assert_eq!(prev_elem.value, 3);
        assert_eq!(dropped.lock().as_slice(), [2]);

        channel.close().await;

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

    #[orengine::test::test_local]
    fn test_drop_local_channel_split() {
        let channel = LocalChannel::bounded(1);
        let dropped = Arc::new(SpinLock::new(Vec::new()));
        let (sender, receiver) = channel.split();

        let _ = sender.send(DroppableElement::new(1, dropped.clone())).await;
        let mut prev_elem = DroppableElement::new(2, dropped.clone());
        receiver.recv_in(&mut prev_elem).await.unwrap();
        assert_eq!(prev_elem.value, 1);
        assert_eq!(dropped.lock().as_slice(), [2]);

        let _ = sender.send(DroppableElement::new(3, dropped.clone())).await;
        unsafe {
            receiver
                .recv_in_ptr(Ptr::from(&mut prev_elem))
                .await
                .unwrap();
        };
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
}

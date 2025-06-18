//! This module contains the [`AsyncSender`], [`AsyncReceiver`] and [`AsyncChannel`] traits.
use crate::local_executor;
use crate::runtime::IsLocal;
use crate::sync::channels::{RecvErr, SendErr, TryRecvErr, TrySendErr};
use crate::sync::{RecvTimeoutErr, SendTimeoutErr};
use crate::utils::OrengineInstant;
use std::future::Future;
use std::ops::Deref;
use std::time::Duration;

/// The `AsyncSender` allows sending values into the [`channel`](AsyncChannel).
///
/// It provides blocking [`send`](AsyncSender::send) and non-blocking
/// [`try_send`](AsyncSender::try_send) methods.
///
/// # Example
///
/// ```rust
/// use orengine::sync::{AsyncChannel, AsyncReceiver, AsyncSender};
///
///  async fn foo() {
///     let channel = orengine::sync::Channel::bounded(2); // capacity = 2
///
///     channel.send(1).await.unwrap();
///
///     let res = channel.recv().await.unwrap();
///
///     assert_eq!(res, 1);
/// }
/// ```
pub trait AsyncSender<T>: IsLocal {
    /// Sends a value into the [`channel`](AsyncChannel).
    ///
    /// Wait until the [`channel`](AsyncChannel) is available or
    /// the [`channel`](AsyncChannel) is closed.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::sync::Arc;
    /// use std::time::Duration;
    /// use orengine::{local_executor, sleep};
    /// use orengine::sync::{AsyncChannel, AsyncReceiver, AsyncSender};
    ///
    ///  async fn foo() {
    ///     let channel = Arc::new(orengine::sync::Channel::bounded(1));
    ///     let channel_clone = channel.clone();
    ///     let start = std::time::Instant::now();
    ///
    ///     local_executor().spawn_local(async move {
    ///         sleep(Duration::from_millis(100)).await;
    ///
    ///         channel_clone.recv().await.unwrap();
    ///     });
    ///
    ///     channel.send(1).await.unwrap();
    ///     assert!(start.elapsed() < Duration::from_millis(100));
    ///
    ///     channel.send(2).await.unwrap(); // blocks, because the channel is full
    ///     assert!(start.elapsed() >= Duration::from_millis(100));
    /// }
    /// ```
    fn send(&self, value: T) -> impl Future<Output = Result<(), SendErr<T>>>;

    /// Sends a value into the [`channel`](AsyncChannel).
    ///
    /// Wait until the [`channel`](AsyncChannel) is available or
    /// the [`channel`](AsyncChannel) is closed or the provided deadline is reached.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::time::Duration;
    /// use orengine::{local_executor, sleep};
    /// use orengine::sync::{AsyncChannel, AsyncReceiver, AsyncSender, SendTimeoutErr};
    ///
    /// # async fn foo() {
    /// let channel = orengine::sync::Channel::bounded(1);
    /// channel.send_deadline(
    ///     0,
    ///     local_executor().start_round_time_for_deadlines() + Duration::from_millis(100)
    /// ).await.unwrap();
    ///
    /// let res: Result<(), SendTimeoutErr<usize>> = channel.send_deadline(
    ///     1,
    ///     local_executor().start_round_time_for_deadlines() + Duration::from_millis(100)
    /// ).await;
    ///
    /// assert!(matches!(res, Err(SendTimeoutErr::Timeout(1))));
    /// # }
    /// ```
    fn send_deadline(
        &self,
        value: T,
        deadline: impl Into<OrengineInstant>,
    ) -> impl Future<Output = Result<(), SendTimeoutErr<T>>>;

    /// Sends a value into the [`channel`](AsyncChannel).
    ///
    /// Wait until the [`channel`](AsyncChannel) is available or
    /// the [`channel`](AsyncChannel) is closed or the provided deadline is reached.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::time::Duration;
    /// use orengine::{local_executor, sleep};
    /// use orengine::sync::{AsyncChannel, AsyncReceiver, AsyncSender, SendTimeoutErr};
    ///
    /// # async fn foo() {
    /// let channel = orengine::sync::Channel::bounded(1);
    /// channel.send_timeout(
    ///     0,
    ///     Duration::from_millis(100)
    /// ).await.unwrap();
    ///
    /// let res: Result<(), SendTimeoutErr<usize>> = channel.send_timeout(
    ///     1,
    ///     Duration::from_millis(100)
    /// ).await;
    ///
    /// assert!(matches!(res, Err(SendTimeoutErr::Timeout(1))));
    /// # }
    /// ```
    fn send_timeout(
        &self,
        value: T,
        timeout: Duration,
    ) -> impl Future<Output = Result<(), SendTimeoutErr<T>>> {
        self.send_deadline(
            value,
            local_executor().start_round_time_for_deadlines() + timeout,
        )
    }

    /// Tries to send a value into the [`channel`](AsyncChannel).
    ///
    /// If the [`channel`](AsyncChannel) is full, returns [`TrySendErr::Full`].
    ///
    /// If the [`channel`](AsyncChannel) is locked, it returns [`TrySendErr::Locked`].
    ///
    /// If the [`channel`](AsyncChannel) is closed, it returns [`TrySendErr::Closed`].
    ///
    /// Else, the value is immediately sent.
    ///
    /// You can find an example in [`AsyncSender::try_send`].
    ///
    /// # Example
    ///
    /// ```rust
    /// use orengine::sync::{AsyncChannel, AsyncSender, TrySendErr};
    ///
    /// # async fn foo() {
    /// let channel = orengine::sync::Channel::bounded(1);
    ///
    /// assert!(channel.try_send(1).is_ok());
    /// assert!(matches!(channel.try_send(2).unwrap_err(), TrySendErr::Full(_)));
    ///
    /// channel.close().await;
    ///
    /// assert!(matches!(channel.try_send(3).unwrap_err(), TrySendErr::Closed(_)));
    /// # }
    /// ```
    fn try_send(&self, value: T) -> Result<(), TrySendErr<T>>;

    /// Closes the [`channel`](AsyncChannel) associated with this sender.
    fn sender_close(&self) -> impl Future<Output = ()>;
}

/// The `AsyncReceiver` allows receiving values from the [`channel`](AsyncChannel).
///
/// It provides blocking [`recv`](AsyncReceiver::recv) and non-blocking
/// [`try_recv`](AsyncReceiver::try_recv) methods.
///
/// # Example
///
/// ```rust
/// use orengine::sync::{AsyncChannel, AsyncReceiver, AsyncSender};
///
/// async fn foo() {
///     let channel = orengine::sync::Channel::bounded(2); // capacity = 2
///
///     channel.send(1).await.unwrap();
///
///     let res = channel.recv().await.unwrap();
///
///     assert_eq!(res, 1);
/// }
/// ```
pub trait AsyncReceiver<T>: IsLocal {
    /// Asynchronously receives a value from the [`channel`](AsyncChannel).
    ///
    /// If the [`channel`](AsyncChannel) is empty, the receiver waits until a value
    /// is available or the [`channel`](AsyncChannel) is closed.
    ///
    /// Else, the value is immediately received.
    ///
    /// # On close
    ///
    /// Returns [`RecvErr::Closed`] if the [`channel`](AsyncChannel) is closed.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use orengine::sync::{AsyncReceiver, RecvErr};
    ///
    /// # type Payload = i32;
    ///
    /// // Must be dropped.
    /// struct Msg { value: Box<Payload> }
    ///
    /// # fn process_msg(msg: &Msg) {}
    ///
    /// async fn handle_messages<R: AsyncReceiver<Msg>>(receiver: R) {
    ///
    ///     loop {
    ///         match receiver.recv().await {
    ///             Ok(msg) => {
    ///                 process_msg(&msg);
    ///             }
    ///             Err(RecvErr::Closed) => return
    ///         }
    ///     }
    /// }
    /// ```
    fn recv(&self) -> impl Future<Output = Result<T, RecvErr>>;

    /// Asynchronously receives a value from the [`channel`](AsyncChannel).
    ///
    /// If the [`channel`](AsyncChannel) is empty, the receiver waits until a value
    /// is available or the [`channel`](AsyncChannel) is closed, or the deadline is reached.
    ///
    /// Else, the value is immediately received.
    ///
    /// # On close
    ///
    /// Returns [`RecvErr::Closed`] if the [`channel`](AsyncChannel) is closed.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::time::Duration;
    /// use orengine::local_executor;
    /// use orengine::sync::{AsyncChannel, AsyncReceiver, AsyncSender};
    ///
    /// # async fn foo() {
    /// let channel = orengine::sync::Channel::bounded(2);
    ///
    /// channel.send(1).await.unwrap();
    ///
    /// channel.recv_deadline(local_executor().start_round_time_for_deadlines() + Duration::from_secs(1)).await.unwrap();
    ///
    /// let res = channel.recv_deadline(local_executor().start_round_time_for_deadlines() + Duration::from_secs(1)).await;
    ///
    /// assert!(matches!(res, Err(orengine::sync::RecvTimeoutErr::Timeout)));
    /// # }
    /// ```
    fn recv_deadline(
        &self,
        deadline: impl Into<OrengineInstant>,
    ) -> impl Future<Output = Result<T, RecvTimeoutErr>>;

    /// Asynchronously receives a value from the [`channel`](AsyncChannel).
    ///
    /// If the [`channel`](AsyncChannel) is empty, the receiver waits until a value
    /// is available or the [`channel`](AsyncChannel) is closed, or the deadline is reached.
    ///
    /// Else, the value is immediately received.
    ///
    /// # On close
    ///
    /// Returns [`RecvErr::Closed`] if the [`channel`](AsyncChannel) is closed.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::time::Duration;
    /// use orengine::local_executor;
    /// use orengine::sync::{AsyncChannel, AsyncReceiver, AsyncSender};
    ///
    /// # async fn foo() {
    /// let channel = orengine::sync::Channel::bounded(2);
    ///
    /// channel.send(1).await.unwrap();
    ///
    /// channel.recv_timeout(Duration::from_secs(1)).await.unwrap();
    ///
    /// let res = channel.recv_timeout(Duration::from_secs(1)).await;
    ///
    /// assert!(matches!(res, Err(orengine::sync::RecvTimeoutErr::Timeout)));
    /// # }
    /// ```
    fn recv_timeout(&self, timeout: Duration) -> impl Future<Output = Result<T, RecvTimeoutErr>> {
        self.recv_deadline(local_executor().start_round_time_for_deadlines() + timeout)
    }

    /// Tries to receive a value from the [`channel`](AsyncChannel).
    ///
    /// If the [`channel`](AsyncChannel) is empty,
    /// returns `Err(`[`TryRecvErr::Empty`]`)`.
    ///
    /// If the [`channel`](AsyncChannel) is locked,
    /// returns `Err(`[`TryRecvErr::Locked`]`)`.
    ///
    /// If the [`channel`](AsyncChannel) is closed, it
    /// returns `Err(`[`TryRecvErr::Closed`]`)`.
    ///
    /// Else, the value is immediately received.
    ///
    /// # Example
    ///
    /// ```rust
    /// use orengine::sync::{AsyncReceiver, TryRecvErr};
    ///
    /// type Payload = i32;
    ///
    /// // Must be dropped.
    /// struct Msg { value: Box<Payload> }
    ///
    /// # fn process_msg(msg: &Msg) {}
    ///
    /// fn handle_new_messages<R: AsyncReceiver<Msg>>(receiver: R) -> Result<usize, ()> {
    ///     let mut processed = 0;
    ///
    ///     loop {
    ///         match receiver.try_recv() {
    ///             Ok(msg) => {
    ///                 process_msg(&msg);
    ///
    ///                 processed += 1;
    ///             }
    ///             Err(e) => return match e {
    ///                 TryRecvErr::Empty | TryRecvErr::Locked => Ok(processed),
    ///                 TryRecvErr::Closed => Err(())
    ///             }
    ///         }
    ///     }
    /// }
    /// ```
    fn try_recv(&self) -> Result<T, TryRecvErr>;

    /// Closes the [`channel`](AsyncChannel) associated with this receiver.
    fn receiver_close(&self) -> impl Future<Output = ()>;
}

/// The `Channel` provides an asynchronous communication channel between tasks.
///
/// It supports both [`bounded`](AsyncChannel::bounded) and [`unbounded`](AsyncChannel::unbounded)
/// channels for sending and receiving values.
///
/// If communication occurs between `local` tasks (read about `local` tasks in
/// [`Executor`](crate::Executor)), use [`LocalChannel`](crate::sync::LocalChannel).
///
/// Else use [`Channel`](crate::sync::Channel).
///
/// # Examples
///
/// ## No splitting
///
/// ```rust
/// use orengine::sync::{AsyncChannel, AsyncReceiver, AsyncSender};
///
/// async fn foo() {
///     let channel = orengine::sync::Channel::bounded(1); // capacity = 1
///
///     channel.send(1).await.unwrap();
///
///     let res = channel.recv().await.unwrap();
///
///     assert_eq!(res, 1);
/// }
/// ```
///
/// ## Splitting
///
/// You can split the channel by using only one trait of [`AsyncSender`] and [`AsyncReceiver`].
///
/// ```rust
/// use orengine::sync::{AsyncChannel, AsyncReceiver, AsyncSender};
/// use orengine::local_executor;
///
/// use std::sync::Arc;
///
/// async fn return_value(sender: impl AsyncSender<u32>) {
///     sender.send(1).await.unwrap();
/// }
///
/// async fn print_value(receiver: impl AsyncReceiver<u32>) {
///     let res = receiver.recv().await.unwrap();
///
///     assert_eq!(res, 1);
/// }
///
/// async fn foo() {
///     let channel = Arc::new(orengine::sync::Channel::bounded(1)); // capacity = 1
///
///     local_executor().spawn_shared(return_value(channel.clone()));
///     local_executor().spawn_shared(print_value(channel));
/// }
/// ```
pub trait AsyncChannel<T>: AsyncSender<T> + AsyncReceiver<T> {
    /// Creates a bounded [`channel`](AsyncChannel) with a given capacity.
    ///
    /// A bounded channel limits the number of items that can be stored before sending blocks.
    /// Once the [`channel`](AsyncChannel) reaches its capacity,
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
    fn bounded(capacity: usize) -> Self;

    /// Creates an unbounded [`channel`](AsyncChannel).
    ///
    /// An unbounded channel allows senders to send an unlimited number of values.
    ///
    /// # Example
    ///
    /// ```rust
    /// use orengine::sync::{AsyncChannel, AsyncSender};
    ///
    ///  async fn foo() {
    ///     let channel = orengine::sync::Channel::unbounded();
    ///
    ///     channel.send(1).await.unwrap(); // not blocked
    ///     channel.send(2).await.unwrap(); // not blocked
    /// }
    /// ```
    fn unbounded() -> Self;

    /// Closes the [`channel`](AsyncChannel).
    fn close(&self) -> impl Future<Output = ()>;
}

impl<T, G: AsyncSender<T>, H: Deref<Target = G> + IsLocal> AsyncSender<T> for H {
    #[allow(
        clippy::future_not_send,
        reason = "It is not Send when T or H is not Send, it is fine"
    )]
    #[inline]
    async fn send(&self, value: T) -> Result<(), SendErr<T>> {
        (**self).send(value).await
    }

    #[allow(
        clippy::future_not_send,
        reason = "It is not Send when T or H is not Send, it is fine"
    )]
    #[inline]
    async fn send_deadline(
        &self,
        value: T,
        deadline: impl Into<OrengineInstant>,
    ) -> Result<(), SendTimeoutErr<T>> {
        (**self).send_deadline(value, deadline).await
    }

    #[inline]
    fn try_send(&self, value: T) -> Result<(), TrySendErr<T>> {
        (**self).try_send(value)
    }

    #[allow(
        clippy::future_not_send,
        reason = "It is not Send when T or H is not Send, it is fine"
    )]
    #[inline]
    async fn sender_close(&self) {
        (**self).sender_close().await;
    }
}

impl<T, G: AsyncReceiver<T>, H: Deref<Target = G> + IsLocal> AsyncReceiver<T> for H {
    #[allow(
        clippy::future_not_send,
        reason = "It is not Send when T or H is not Send, it is fine"
    )]
    #[inline]
    async fn recv(&self) -> Result<T, RecvErr> {
        (**self).recv().await
    }

    #[allow(
        clippy::future_not_send,
        reason = "It is not Send when T or H is not Send, it is fine"
    )]
    #[inline]
    async fn recv_deadline(
        &self,
        deadline: impl Into<OrengineInstant>,
    ) -> Result<T, RecvTimeoutErr> {
        (**self).recv_deadline(deadline).await
    }

    #[inline]
    fn try_recv(&self) -> Result<T, TryRecvErr> {
        (**self).try_recv()
    }

    #[allow(
        clippy::future_not_send,
        reason = "It is not Send when T or H is not Send, it is fine"
    )]
    #[inline]
    async fn receiver_close(&self) {
        (**self).receiver_close().await;
    }
}

pub(crate) mod macros {
    macro_rules! impl_recv_from_recv_in_ptr {
        () => {
            #[allow(
                clippy::future_not_send,
                reason = "It is not Send when T is not Send, it is fine"
            )]
            #[inline]
            async fn recv(&self) -> Result<T, $crate::sync::RecvErr> {
                let mut res_slot = std::mem::MaybeUninit::uninit();

                unsafe { self.recv_in_ptr(Ptr::from(res_slot.as_mut_ptr())).await? };

                Ok(unsafe { res_slot.assume_init() })
            }
        };
    }

    macro_rules! impl_recv_with_timeout_from_recv_in_ptr_with_deadline {
        () => {
            #[allow(
                clippy::future_not_send,
                reason = "It is not Send when T is not Send, it is fine"
            )]
            #[inline]
            async fn recv_deadline(
                &self,
                deadline: impl Into<$crate::utils::OrengineInstant>,
            ) -> Result<T, $crate::sync::RecvTimeoutErr> {
                let mut res_slot = std::mem::MaybeUninit::uninit();

                unsafe {
                    self.recv_in_ptr_with_deadline(Ptr::from(res_slot.as_mut_ptr()), deadline)
                        .await?
                };

                Ok(unsafe { res_slot.assume_init() })
            }
        };
    }

    macro_rules! impl_try_recv_from_recv_in_ptr {
        () => {
            #[inline]
            fn try_recv(&self) -> Result<T, $crate::sync::TryRecvErr> {
                let mut res_slot = std::mem::MaybeUninit::uninit();

                unsafe { self.try_recv_in_ptr(Ptr::from(res_slot.as_mut_ptr()))? };

                Ok(unsafe { res_slot.assume_init() })
            }
        };
    }

    pub(crate) use {
        impl_recv_from_recv_in_ptr, impl_recv_with_timeout_from_recv_in_ptr_with_deadline,
        impl_try_recv_from_recv_in_ptr,
    };
}

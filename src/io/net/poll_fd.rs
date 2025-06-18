//! This module contains the [`PollRecv`], [`PollRecvWithDeadline`], [`PollSend`]
//! and [`PollSendWithDeadline`] IO operations and the [`AsyncPollSocket`] trait.
use crate::io::io_request_data::{IoRequestData, IoRequestDataPtr};
use crate::io::macros::{poll_for_io_request, poll_for_time_bounded_io_request};
use crate::io::sys::{AsRawSocket, RawSocket};
use crate::io::worker::{local_worker, IoWorker};
use crate::local_executor;
use crate::utils::{unwrap_or_bug_hint, OrengineInstant};

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

macro_rules! generate_poll {
    ($name:ident, $name_with_deadline:ident, $method:expr, $method_with_deadline:expr) => {
        /// `poll_raw_socket` io operation.
        #[repr(C)]
        pub struct $name {
            raw_socket: RawSocket,
            io_request_data: Option<IoRequestData>,
        }

        impl $name {
            /// Creates a new `poll_raw_socket` io operation.
            pub fn new(raw_socket: RawSocket) -> Self {
                Self {
                    raw_socket,
                    io_request_data: None,
                }
            }
        }

        impl Future for $name {
            type Output = std::io::Result<()>;

            fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
                let this = unsafe { self.get_unchecked_mut() };

                poll_for_io_request!(
                    {
                        $method(
                            local_worker(),
                            this.raw_socket,
                            IoRequestDataPtr::new(unwrap_or_bug_hint(
                                this.io_request_data.as_mut(),
                            )),
                        );
                    },
                    this.io_request_data,
                    cx,
                    _ret,
                    ()
                );
            }
        }

        unsafe impl Send for $name {}

        /// `poll_raw_socket` io operation with deadline.
        #[repr(C)]
        pub struct $name_with_deadline {
            deadline: OrengineInstant,
            raw_socket: RawSocket,
            io_request_data: Option<IoRequestData>,
        }

        impl $name_with_deadline {
            /// Creates a new `poll_raw_socket` io operation with deadline.
            pub fn new(raw_socket: RawSocket, deadline: OrengineInstant) -> Self {
                Self {
                    raw_socket,
                    io_request_data: None,
                    deadline,
                }
            }
        }

        impl Future for $name_with_deadline {
            type Output = std::io::Result<()>;

            fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
                let this = unsafe { self.get_unchecked_mut() };
                let worker = local_worker();

                poll_for_time_bounded_io_request!(
                    {
                        $method_with_deadline(
                            worker,
                            this.raw_socket,
                            IoRequestDataPtr::new(unwrap_or_bug_hint(
                                this.io_request_data.as_mut(),
                            )),
                            &mut this.deadline,
                        );
                    },
                    this.io_request_data,
                    worker,
                    &this.deadline,
                    cx,
                    _ret,
                    ()
                );
            }
        }

        unsafe impl Send for $name_with_deadline {}
    };
}

generate_poll!(
    PollRecv,
    PollRecvWithDeadline,
    IoWorker::poll_socket_read,
    IoWorker::poll_socket_read_with_deadline
);
generate_poll!(
    PollSend,
    PollSendWithDeadline,
    IoWorker::poll_socket_write,
    IoWorker::poll_socket_write_with_deadline
);

/// The `AsyncPollSocket` trait provides non-blocking polling methods for readiness in receiving
/// and sending data on file descriptors.
///
/// It enables polling with deadlines, timeouts,
/// and simple polling for both read and write readiness.
///
/// This trait can be implemented for any writable and readable structs
/// that support the [`AsRawSocket`] trait.
pub trait AsyncPollSocket: AsRawSocket {
    /// Returns future that will be resolved when the file descriptor
    /// becomes readable or an error occurs.
    ///
    /// # Usage
    ///
    /// Call this method on the stream before allocating a [`buffer`](crate::io::Buffer)
    /// and receive from the stream.
    /// After the receiving release (drop) the [`buffer`](crate::io::Buffer).
    ///
    /// Asynchronously peeks into the incoming data without consuming it, filling the buffer with
    /// available data. Returns the number of bytes peeked.
    ///
    /// # Example
    ///
    /// ```rust
    /// use orengine::net::TcpStream;
    /// use orengine::io::{full_buffer, AsyncConnectStream, AsyncPollSocket, AsyncRecv};
    ///
    /// # async fn foo() -> std::io::Result<()> {
    /// let mut stream = TcpStream::connect("127.0.0.1:8080").await?;
    /// stream.poll_recv().await?;
    /// let mut buf = full_buffer();
    /// let bytes_peeked = stream.recv(&mut buf).await?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    fn poll_recv(&self) -> PollRecv {
        PollRecv::new(AsRawSocket::as_raw_socket(self))
    }

    /// Returns future that will be resolved when the file descriptor
    /// becomes readable or an error occurs or the deadline is reached.
    ///
    /// If the deadline is exceeded, the method will return an error with
    /// kind [`ErrorKind::TimedOut`](std::io::ErrorKind::TimedOut).
    ///
    /// # Usage
    ///
    /// Call this method on the stream before allocating a [`buffer`](crate::io::Buffer)
    /// and receive from the stream.
    /// After the receiving release (drop) the [`buffer`](crate::io::Buffer).
    ///
    /// Asynchronously peeks into the incoming data with a specified deadline.
    /// Returns the number of bytes peeked.
    ///
    /// If the deadline is exceeded, the method will return an error with
    /// kind [`ErrorKind::TimedOut`](std::io::ErrorKind::TimedOut).
    ///
    /// # Example
    ///
    /// ```rust
    /// use orengine::net::TcpStream;
    /// use orengine::io::{full_buffer, AsyncConnectStream, AsyncPollSocket, AsyncRecv};
    /// use std::time::{Duration, Instant};
    ///
    /// async fn foo() -> std::io::Result<()> {
    /// let mut stream = TcpStream::connect("127.0.0.1:8080").await?;
    /// let deadline = Instant::now() + Duration::from_secs(5);
    /// stream.poll_recv_with_deadline(deadline).await?;
    /// let mut buf = full_buffer();
    ///
    /// let bytes_peeked = stream.recv_with_deadline(&mut buf, deadline).await?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    fn poll_recv_with_deadline(
        &self,
        deadline: impl Into<OrengineInstant>,
    ) -> PollRecvWithDeadline {
        PollRecvWithDeadline::new(AsRawSocket::as_raw_socket(self), deadline.into())
    }

    /// Returns future that will be resolved when the file descriptor
    /// becomes readable or an error occurs or the timeout is exceeded.
    ///
    /// If the deadline is exceeded, the method will return an error with
    /// kind [`ErrorKind::TimedOut`](std::io::ErrorKind::TimedOut).
    ///
    /// # Usage
    ///
    /// Call this method on the stream before allocating a [`buffer`](crate::io::Buffer)
    /// and receive from the stream.
    /// After the receiving release (drop) the [`buffer`](crate::io::Buffer).
    ///
    /// Asynchronously peeks into the incoming data with a specified timeout.
    /// Returns the number of bytes peeked.
    ///
    /// If the deadline is exceeded, the method will return an error with
    /// kind [`ErrorKind::TimedOut`](std::io::ErrorKind::TimedOut).
    ///
    /// # Example
    ///
    /// ```rust
    /// use orengine::net::TcpStream;
    /// use orengine::io::{full_buffer, AsyncConnectStream, AsyncPollSocket, AsyncRecv};
    /// use std::time::Duration;
    ///
    /// async fn foo() -> std::io::Result<()> {
    /// let mut stream = TcpStream::connect("127.0.0.1:8080").await?;
    /// let timeout = Duration::from_secs(5);
    /// stream.poll_recv_with_timeout(timeout).await?;
    /// let mut buf = full_buffer();
    ///
    /// let bytes_peeked = stream.recv_with_timeout(&mut buf, timeout).await?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    fn poll_recv_with_timeout(&self, timeout: Duration) -> PollRecvWithDeadline {
        self.poll_recv_with_deadline(local_executor().start_round_time_for_deadlines() + timeout)
    }

    /// Returns future that will be resolved when the file descriptor
    /// becomes writable or an error occurs.
    ///
    /// # Usage
    ///
    /// Call this method on the stream before allocating a [`buffer`](crate::io::Buffer)
    /// and send to the stream.
    /// After the sending release (drop) the [`buffer`](crate::io::Buffer).
    /// As opposed to [`poll_recv`](Self::poll_recv), it does not have a significant impact
    /// on productivity and efficiency.
    #[inline]
    fn poll_send(&self) -> PollSend {
        PollSend::new(AsRawSocket::as_raw_socket(self))
    }

    /// Returns future that will be resolved when the file descriptor
    /// becomes writable or an error occurs or the deadline is reached.
    ///
    /// If the deadline is exceeded, the method will return an error with
    /// kind [`ErrorKind::TimedOut`](std::io::ErrorKind::TimedOut).
    ///
    /// # Usage
    ///
    /// Call this method on the stream before allocating a [`buffer`](crate::io::Buffer)
    /// and send to the stream.
    /// After the sending release (drop) the [`buffer`](crate::io::Buffer).
    /// As opposed to [`poll_recv_with_deadline`](Self::poll_recv_with_deadline), it does not have a significant impact
    /// on productivity and efficiency.
    #[inline]
    fn poll_send_with_deadline(
        &self,
        deadline: impl Into<OrengineInstant>,
    ) -> PollSendWithDeadline {
        PollSendWithDeadline::new(AsRawSocket::as_raw_socket(self), deadline.into())
    }

    /// Returns future that will be resolved when the file descriptor
    /// becomes writable or an error occurs or the timeout is exceeded.
    ///
    /// If the deadline is exceeded, the method will return an error with
    /// kind [`ErrorKind::TimedOut`](std::io::ErrorKind::TimedOut).
    ///
    /// # Usage
    ///
    /// Call this method on the stream before allocating a [`buffer`](crate::io::Buffer)
    /// and send to the stream.
    /// After the sending release (drop) the [`buffer`](crate::io::Buffer).
    /// As opposed to [`poll_recv_with_timeout`](Self::poll_recv_with_timeout), it does not have a significant impact
    /// on productivity and efficiency.
    #[inline]
    fn poll_send_with_timeout(&self, timeout: Duration) -> PollSendWithDeadline {
        self.poll_send_with_deadline(local_executor().start_round_time_for_deadlines() + timeout)
    }
}

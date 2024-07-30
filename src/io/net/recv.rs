use std::future::Future;
use std::intrinsics::unlikely;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::io::Result;
use std::time::{Duration, Instant};
use io_macros::{poll_for_io_request, poll_for_time_bounded_io_request};
use crate::io::AsyncPollFd;
use crate::io::io_request::{IoRequest};
use crate::io::io_sleeping_task::TimeBoundedIoTask;
use crate::io::sys::{AsFd, Fd};
use crate::io::worker::{IoWorker, local_worker};
use crate::runtime::task::Task;

#[must_use = "Future must be awaited to drive the IO operation"]
pub struct Recv<'a> {
    fd: Fd,
    buf: &'a mut [u8],
    io_request: Option<IoRequest>
}

impl<'a> Recv<'a> {
    pub fn new(fd: Fd, buf: &'a mut [u8]) -> Self {
        Self {
            fd,
            buf,
            io_request: None
        }
    }
}

impl<'a> Future for Recv<'a> {
    type Output = Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let worker = unsafe { local_worker() };
        let ret;

        poll_for_io_request!((
             worker.recv(this.fd, this.buf.as_mut_ptr(), this.buf.len(), this.io_request.as_ref().unwrap_unchecked()),
             ret
        ));
    }
}

#[must_use = "Future must be awaited to drive the IO operation"]
pub struct RecvWithDeadline<'a> {
    fd: Fd,
    buf: &'a mut [u8],
    time_bounded_io_task: TimeBoundedIoTask,
    io_request: Option<IoRequest>
}

impl<'a> RecvWithDeadline<'a> {
    pub fn new(fd: Fd, buf: &'a mut [u8], deadline: Instant) -> Self {
        Self {
            fd,
            buf,
            time_bounded_io_task: TimeBoundedIoTask::new(deadline, 0),
            io_request: None
        }
    }
}

impl<'a> Future for RecvWithDeadline<'a> {
    type Output = Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let worker = unsafe { local_worker() };
        let ret;

        poll_for_time_bounded_io_request!((
             worker.recv(this.fd, this.buf.as_mut_ptr(), this.buf.len(), this.io_request.as_ref().unwrap_unchecked()),
             ret
        ));
    }
}

#[macro_export]
macro_rules! generate_recv {
    () => {
         #[inline(always)]
        pub fn recv<'a>(&mut self, buf: &'a mut [u8]) -> Recv<'a> {
            Recv::new(self.as_raw_fd(), buf)
        }

        #[inline(always)]
        pub fn recv_with_deadline<'a>(&mut self, buf: &'a mut [u8], deadline: Instant) -> RecvWithDeadline<'a> {
            RecvWithDeadline::new(self.as_raw_fd(), buf, deadline)
        }

        #[inline(always)]
        pub fn recv_with_timeout<'a>(&mut self, buf: &'a mut [u8], duration: Duration) -> RecvWithDeadline<'a> {
            self.recv_with_deadline(buf, Instant::now() + duration)
        }
    }
}

#[macro_export]
macro_rules! generate_recv_exact {
    () => {
        #[inline(always)]
        pub async fn recv_exact(&mut self, buf: &mut [u8]) -> Result<()> {
            let mut received = 0;
            let mut last_received;

            while received < buf.len() {
                last_received = self.recv(&mut buf[received..]).await?;
                if unlikely(last_received == 0) {
                    return Err(std::io::Error::from(std::io::ErrorKind::ConnectionAborted));
                }

                received += last_received;
            }

            Ok(())
        }

        #[inline(always)]
        pub async fn recv_exact_with_deadline(&mut self, buf: &mut [u8], deadline: Instant) -> Result<()> {
            let mut received = 0;
            let mut last_received;

            while received < buf.len() {
                last_received = self.recv_with_deadline(&mut buf[received..], deadline).await?;
                if unlikely(last_received == 0) {
                    return Err(std::io::Error::from(std::io::ErrorKind::ConnectionAborted));
                }

                received += last_received;
            }

            Ok(())
        }

        #[inline(always)]
        pub async fn recv_exact_with_timeout(&mut self, buf: &mut [u8], duration: Duration) -> Result<()> {
            self.recv_exact_with_deadline(buf, Instant::now() + duration).await
        }
    }
}
//! This module provides the [`RemoveDir`] io operation.
use crate::io::io_request_data::{IoRequestData, IoRequestDataPtr};
use crate::io::sys::{get_os_path_ptr, OsPath};
use crate::io::worker::{local_worker, IoWorker};
use crate::utils::unwrap_or_bug_hint;

use crate::io::macros::poll_for_io_request;
use std::future::Future;
use std::io::Result;
use std::pin::Pin;
use std::task::{Context, Poll};

/// `remove_dir` io operation from a given path.
#[repr(C)]
pub struct RemoveDir {
    path: OsPath,
    io_request_data: Option<IoRequestData>,
}

impl RemoveDir {
    /// Creates a new `remove_dir` io operation from a given path.
    pub fn new(path: OsPath) -> Self {
        Self {
            path,
            io_request_data: None,
        }
    }
}

impl Future for RemoveDir {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = &mut *self;

        poll_for_io_request!(
            {
                local_worker().remove_dir(
                    get_os_path_ptr(&this.path),
                    IoRequestDataPtr::new(unwrap_or_bug_hint(this.io_request_data.as_mut())),
                );
            },
            this.io_request_data,
            cx,
            _ret,
            ()
        );
    }
}

unsafe impl Send for RemoveDir {}

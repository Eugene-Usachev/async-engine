//! This module provides the [`SyncAll`] io operation.

use std::future::Future;
use std::io::Result;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::io::io_request_data::{IoRequestData, IoRequestDataPtr};
use crate::io::macros::poll_for_io_request;
use crate::io::sys::{AsRawFile, RawFile};
use crate::io::worker::{local_worker, IoWorker};
use crate::utils::unwrap_or_bug_hint;

/// `sync_all` io operation.
#[repr(C)]
pub struct SyncAll {
    raw_file: RawFile,
    io_request_data: Option<IoRequestData>,
}

impl SyncAll {
    /// Creates a new `sync_all` io operation.
    pub fn new(raw_file: RawFile) -> Self {
        Self {
            raw_file,
            io_request_data: None,
        }
    }
}

impl Future for SyncAll {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = &mut *self;

        poll_for_io_request!(
            {
                local_worker().sync_all(
                    this.raw_file,
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

unsafe impl Send for SyncAll {}

/// The [`AsyncSyncAll`] trait provides a [`sync_all`](AsyncSyncAll::sync_all) method
/// to synchronize the data and metadata of a file with the underlying storage device.
///
/// For more details, see [`sync_all`](AsyncSyncAll::sync_all).
pub trait AsyncSyncAll: AsRawFile {
    /// Attempts to sync all OS-internal file content and metadata to disk.
    ///
    /// This function will attempt to ensure that all in-memory data reaches
    /// the filesystem before returning.
    ///
    /// This can be used to handle errors that would otherwise only be caught when
    /// the File is closed, as dropping a File will ignore all errors. Note, however,
    /// that [`sync_all`](AsyncSyncAll::sync_all) is generally more expensive than closing
    /// a file by dropping it because the latter is not required to block until the data
    /// has been written to the filesystem.
    ///
    /// If synchronizing the metadata is not required,
    /// use [`sync_data`](crate::io::AsyncSyncData::sync_data) instead.
    ///
    /// # Example
    ///
    /// ```rust
    /// use orengine::fs::{File, OpenOptions};
    /// use orengine::io::{AsyncRead, AsyncSyncAll, AsyncWrite, buffer};
    ///
    /// # async fn foo() -> std::io::Result<()> {
    /// let options = OpenOptions::new().write(true);
    /// let mut file = File::open("example.txt", &options).await?;
    ///
    /// let mut buf = buffer();
    /// buf.append(b"Hello, World!");
    ///
    /// file.write_all(&buf).await?;
    /// file.sync_all().await?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    fn sync_all(&self) -> impl Future<Output=Result<()>> {
        SyncAll::new(self.as_raw_file())
    }
}

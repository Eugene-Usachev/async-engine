//! This module contains the fallback implementation of the `io` module.
//!
//! It uses the `mio` as a poller.
//! For blocking operations,
//! it uses a thread pool if the ` fallback_thread_pool ` feature is enabled
//! or blocks the current thread otherwise.
//!
//! It uses [`IoCall`](io_call::IoCall) to represent a type of I/O call and its arguments instead
//! of using dynamic functions.
//!
//! This module contains the following submodules:
//! - [`io_call`]: contains the [`IoCall`](io_call::IoCall) type and its implementations.
//! - [`mio_poller`]: contains the [`MioPoller`](mio_poller::MioPoller) type and its implementations.
//! - [`open_options`]: contains the [`OpenOptions`](open_options::OpenOptions) type
//!   and its implementations.
//! - [`operations`]: contains the [`Operations`](operations::Operations) type
//!   and its implementations.
//! - [`os_message_header`]: contains the [`OsMessageHeader`](os_message_header::OsMessageHeader)
//!   type and its implementations.
//! - [`os_path`]: contains the [`OsPath`](os_path::OsPath) type and its implementations.
//! - [`with_thread_pool`]: contains the [`FallbackWorker`] type and its implementations
//!   (with the `fallback_thread_pool` feature).
//! - [`worker`]: contains the [`FallbackWorker`] type and its implementations
//!   (without the `fallback_thread_pool` feature).
pub(crate) mod io_call;
pub(crate) mod mio_poller;
pub(crate) mod open_options;
pub(crate) mod operations;
pub(crate) mod os_message_header;
pub(crate) mod os_path;
#[cfg(feature = "fallback_thread_pool")]
mod with_thread_pool;
#[cfg(not(feature = "fallback_thread_pool"))]
mod worker;

#[cfg(feature = "fallback_thread_pool")]
pub(crate) use with_thread_pool::worker_with_thread_pool::FallbackWorker;

#[cfg(not(feature = "fallback_thread_pool"))]
pub(crate) use worker::FallbackWorker;

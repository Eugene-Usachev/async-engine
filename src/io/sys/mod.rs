//! Helper for work with the system.
#[cfg(unix)]
pub(crate) mod unix;

#[cfg(unix)]
pub(crate) use io_uring::types::OpenHow;
#[cfg(unix)]
pub(crate) use unix::fd::*;
#[cfg(target_os = "linux")]
pub(crate) use unix::os_message_header::*;
#[cfg(target_os = "linux")]
pub(crate) use unix::os_path as OsPath;
#[cfg(target_os = "linux")]
pub(crate) use unix::IOUringWorker as WorkerSys;

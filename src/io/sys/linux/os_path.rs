//! This module contains the bindings for the `Path` struct depending on the OS.
use std::ffi::CString;
use std::io::{self, Error, ErrorKind};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// Synonymous with the OS path type.
pub(crate) type OsPath = CString;

/// Synonymous with the OS path type pointer.
pub(crate) type OsPathPtr = *const libc::c_char;

/// Gets os path from path.
#[inline]
pub(crate) fn get_os_path(path: &Path) -> io::Result<OsPath> {
    match CString::new(path.as_os_str().as_bytes()) {
        Ok(path) => Ok(path),
        Err(err) => Err(Error::new(ErrorKind::InvalidInput, err)),
    }
}

/// Gets a pointer to the OS path.
pub(crate) fn get_os_path_ptr(path: &OsPath) -> OsPathPtr {
    path.as_ptr()
}

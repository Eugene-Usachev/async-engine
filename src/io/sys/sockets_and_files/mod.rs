//! This module contains traits that allow to use `sockets` and `files` on any OS.
//!
//! [`as_borrowed`]: contains [`BorrowedFile`] and [`BorrowedSocket`] traits.
//! [`as_raw`]: contains [`AsFile`] and [`AsSocket`] traits.
//! [`from_raw`]: contains [`FromRawFile`] and [`FromRawSocket`] traits.
//! [`into_raw`]: contains [`IntoRawFile`] and [`IntoRawSocket`] traits.
pub mod as_borrowed;
pub mod as_raw;
pub mod from_raw;
pub mod into_raw;

pub use as_borrowed::*;
pub use as_raw::*;
pub use from_raw::*;
pub use into_raw::*;

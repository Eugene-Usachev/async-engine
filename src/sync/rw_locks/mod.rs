//! This module contains an async read-write lock (e.g. [`std::sync::RwLock`]).
//!
//! You can use [`AsyncRWLock`] as a generic mutual exclusion primitive.
//!
//! In `local` context use [`LocalRWLock`].
//! In `shared` context use [`RWLock`].
mod async_trait;
mod local;
mod lock_status;
#[cfg(not(target_has_atomic = "64"))]
mod naive_shared;
#[cfg(target_has_atomic = "64")]
mod shared;

pub use async_trait::*;
pub use local::*;
pub use lock_status::*;

#[cfg(not(target_has_atomic = "64"))]
pub use naive_shared::{RWLock, ReadLockGuard, WriteLockGuard};

#[cfg(target_has_atomic = "64")]
pub use shared::{RWLock, ReadLockGuard, WriteLockGuard};

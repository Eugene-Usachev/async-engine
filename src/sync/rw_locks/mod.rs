pub mod async_trait;
pub mod local;
pub(crate) mod lock_status;
pub mod naive_shared;
#[cfg(target_has_atomic = "64")]
pub mod shared;

pub use async_trait::*;
pub use local::*;
pub use lock_status::*;

#[cfg(not(target_has_atomic = "64"))]
pub use naive_shared::{RWLock, ReadLockGuard, WriteLockGuard};

#[cfg(target_has_atomic = "64")]
pub use shared::{RWLock, ReadLockGuard, WriteLockGuard};

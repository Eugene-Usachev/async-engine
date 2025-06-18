//! This module contains an async read-write lock (e.g. `WaitGroup` in Go).
//!
//! You can use [`AsyncWaitGroup`] as a generic mutual exclusion primitive.
//!
//! In `local` context use [`LocalWaitGroup`].
//! In `shared` context use [`WaitGroup`].

mod async_trait;
mod local;
mod shared;

pub use async_trait::*;
pub use local::*;
pub use shared::*;

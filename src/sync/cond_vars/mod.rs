//! This module contains an async condition variables (e.g. [`std::sync::Condvar`]).
//!
//! You can use [`AsyncCondVar`] as a generic condition variable.
//!
//! In `local` context use [`LocalCondVar`].
//! In `shared` context use [`CondVar`].
mod async_trait;
mod local;
mod shared;

pub use async_trait::*;
pub use local::*;
pub use shared::*;

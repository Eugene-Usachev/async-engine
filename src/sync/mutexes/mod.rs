//! This module contains an async mutual exclusion primitive (e.g. [`std::sync::Mutex`]).
//!
//! You can use [`AsyncMutex`] as a generic mutual exclusion primitive.
//!
//! In `local` context use [`LocalMutex`].
//! In `shared` context use [`Mutex`] or [`NaiveMutex`].
//!
//! It also contains the [`AsyncSubscribableMutex`] trait that allows to implement
//! [`AsyncCondVar`](crate::sync::AsyncCondVar).
mod async_trait;
mod local;
mod naive_shared;
mod smart_shared;
mod subscribable_trait;

pub use async_trait::*;
pub use local::*;
pub use naive_shared::*;
pub use smart_shared::*;
pub use subscribable_trait::*;

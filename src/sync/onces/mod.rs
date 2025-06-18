//! This module contains an async one-time execution primitive (e.g. [`std::sync::Once`]).
//!
//! You can use [`AsyncOnce`] as a generic one-time execution primitive.
//!
//! In `local` context use [`LocalOnce`].
//! In `shared` context use [`Once`].

mod async_trait;
mod local;
mod shared;
mod state;

pub use async_trait::*;
pub use local::*;
pub use shared::*;
pub use state::*;

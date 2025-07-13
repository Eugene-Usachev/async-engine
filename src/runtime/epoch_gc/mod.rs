//! This module implements epoch garbage collection (a.k.a. Epoch-Based Memory Reclamation).
//!
//! You can use it [`EpochGCLocalManager`] which can be obtained via [`local_epoch_gc`].
//! Read [`EpochGCLocalManager`] for more details.
//!
//! Despite the presence of the word Epoch in the name,
//! the implementation differs slightly from the generally accepted one
//! to show itself more effectively in async runtimes.
mod deferred;
mod global;
mod local_manager;
mod number_of_executors;
#[cfg(test)]
mod test;

pub(super) use deferred::Deferred;
pub(super) use global::GLOBAL_EPOCH_GC;
pub(super) use number_of_executors::NumberOfExecutorsInEpoch;

pub(crate) use local_manager::register_local_epoch_gc;

pub use local_manager::{EpochGCLocalManager, local_epoch_gc};

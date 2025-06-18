//! This module contains code for interaction between executors.
//! It provides only the [`ExecutorIsNotRegisteredErr`] error for users.
mod interactor;
pub(crate) mod sync_batch_optimized_task_queue;

pub use interactor::ExecutorIsNotRegisteredErr;
pub(crate) use interactor::{Interactor, SharedTaskListForSendTo};
pub(crate) use sync_batch_optimized_task_queue::*;

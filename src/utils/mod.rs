//! This module contains some utilities that can't be attached to any other module.
//! Most of them are private,
//! but if you want to use them, you can copy them to your project and create an issue about it.
pub mod core;
#[cfg(test)]
pub(crate) mod droppable_element;
pub(crate) mod each_addr;
pub mod hints;
pub(crate) mod never_wait_lock;
pub mod ptr;
mod sealed;
#[macro_use]
mod task_structures_pool;
mod array_deque;
mod backoff;
mod clear_with;
mod extend_vec_deque_by_task_slice;
mod instant;
mod lock_free_task_stack;
mod paired_with_lock;
mod preempt;
mod progressive_timeout;
pub mod seg_task_queue;
mod sendable_ptr;
mod shuffle;
pub(crate) mod vec_map;

pub use array_deque::ArrayDeque;
pub(crate) use backoff::Backoff;
pub use clear_with::clear_with;
pub use core::*;
pub(crate) use extend_vec_deque_by_task_slice::extend_vec_deque_by_task_slice;
pub use hints::{
    assert_hint, likely, unlikely, unreachable_hint, unwrap_or_bug_hint, unwrap_or_bug_message_hint,
};
pub use instant::OrengineInstant;
pub use lock_free_task_stack::*;
pub use never_wait_lock::NeverWaitLock;
pub(crate) use paired_with_lock::PairedWithLock;
pub use preempt::{long_preempt, short_preempt};
pub(crate) use progressive_timeout::*;
pub use ptr::*;
pub(crate) use sealed::Sealed;
pub use seg_task_queue::SegTaskQueue;
pub use sendable_ptr::*;
pub use shuffle::shuffle;
pub use task_structures_pool::{acquire_task_vec_from_pool, TaskVecFromPool};

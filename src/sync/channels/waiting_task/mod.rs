//! This module contains the [`TaskInSelect`] and [`TaskInSelectBranch`] and some additional
//! functionality for effective implementation of the [`select`](crate::select).
//!
//! It is public only for implementation the [`select`](crate::select).
//! If you don't want to write your own `select` or understand how does `Orengine` works,
//! you don't need to read it.

mod sender_receiver_deque;
pub mod task_in_select;
pub(crate) mod waiting_select_task_deque;

pub(crate) use sender_receiver_deque::SenderReceiverQueueOption;
pub(crate) use task_in_select::PopIfAcquiredResult;
pub use task_in_select::{TaskInSelect, TaskInSelectBranch};

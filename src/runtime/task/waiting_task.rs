//! This module contains the [`WaitingTask`] enum for channels.
use crate::runtime::{Task, TaskWithDeadline};
use crate::sync::channels::CallStatePtr;
use crate::sync::channels::waiting_task::TaskInSelectBranch;
use std::ptr::NonNull;

/// Represents a task waiting on a channel operation.
///
/// This enum is used internally to track tasks that are blocked on some
/// asynchronous event, either as a regular waiting task or as part of a
/// `select` operation over multiple branches.
pub(crate) enum WaitingTask<T> {
    /// A task that is waiting on a single channel operation.
    Common(Task, CallStatePtr, NonNull<T>),

    /// A task that is participating in a `select` operation over multiple branches.
    InSelector(TaskInSelectBranch, CallStatePtr, NonNull<T>),

    /// A task that is waiting on a single channel operation with a deadline.
    CommonWithDeadline(TaskWithDeadline, CallStatePtr, NonNull<T>),
}

impl<T> WaitingTask<T> {
    /// Creates a `Common` waiting task, representing a task blocked on a single channel.
    pub(crate) fn common(task: Task, state: CallStatePtr, slot: NonNull<T>) -> Self {
        Self::Common(task, state, slot)
    }

    /// Creates an `InSelector` waiting task, representing a task participating in a `select` over channels.
    pub(crate) fn in_selector(
        task: TaskInSelectBranch,
        state: CallStatePtr,
        slot: NonNull<T>,
    ) -> Self {
        Self::InSelector(task, state, slot)
    }

    /// Creates a `CommonWithDeadline` waiting task, representing a task blocked
    /// on a single channel with a deadline.
    pub(crate) fn common_with_deadline(
        task: TaskWithDeadline,
        state: CallStatePtr,
        slot: NonNull<T>,
    ) -> Self {
        Self::CommonWithDeadline(task, state, slot)
    }

    /// Returns whether the waiting task can be freed.
    ///
    /// For example, if [`TaskInSelectBranch`] is acquired.
    pub(crate) fn can_be_freed(&self) -> bool {
        match self {
            Self::InSelector(task_in_select_branch, _, _) => task_in_select_branch.is_acquired(),
            Self::Common(_, _, _) => false,
            Self::CommonWithDeadline(task_with_deadline, _, _) => task_with_deadline.was_woken(),
        }
    }
}

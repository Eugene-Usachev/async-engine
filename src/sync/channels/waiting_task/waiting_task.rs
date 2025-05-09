// TODO docs
use crate::runtime::Task;
use crate::sync::channels::state::CallStatePtr;
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
}

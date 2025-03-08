// TODO docs
use crate::runtime::Task;
use crate::sync::channels::state::CallState;
use crate::sync::channels::waiting_task::TaskInSelectBranch;
use std::ptr::NonNull;

pub(crate) enum WaitingTask<T> {
    Common(Task, NonNull<CallState>, NonNull<T>),
    InSelector(TaskInSelectBranch, NonNull<CallState>, NonNull<T>),
}

impl<T> WaitingTask<T> {
    pub(crate) fn common(task: Task, state: NonNull<CallState>, slot: NonNull<T>) -> Self {
        Self::Common(task, state, slot)
    }

    pub(crate) fn in_selector(
        task: TaskInSelectBranch,
        state: NonNull<CallState>,
        slot: NonNull<T>,
    ) -> Self {
        Self::InSelector(task, state, slot)
    }
}

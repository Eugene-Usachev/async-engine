// TODO docs
use crate::runtime::Task;
use crate::sync::channels::state::CallStatePtr;
use crate::sync::channels::waiting_task::TaskInSelectBranch;
use std::ptr::NonNull;

pub(crate) enum WaitingTask<T> {
    Common(Task, CallStatePtr, NonNull<T>),
    InSelector(TaskInSelectBranch, CallStatePtr, NonNull<T>),
}

impl<T> WaitingTask<T> {
    pub(crate) fn common(task: Task, state: CallStatePtr, slot: NonNull<T>) -> Self {
        Self::Common(task, state, slot)
    }

    pub(crate) fn in_selector(
        task: TaskInSelectBranch,
        state: CallStatePtr,
        slot: NonNull<T>,
    ) -> Self {
        Self::InSelector(task, state, slot)
    }
}

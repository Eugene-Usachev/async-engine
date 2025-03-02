// TODO docs
use crate::runtime::Task;
use crate::sync::channels::states::PtrToCallState;
use crate::sync::channels::waiting_task::TaskInSelectBranch;
use std::ptr::NonNull;

pub(crate) enum WaitingTask<T> {
    Common(Task, PtrToCallState, NonNull<T>),
    InSelector(TaskInSelectBranch, PtrToCallState, NonNull<T>),
}

impl<T> WaitingTask<T> {
    pub(crate) fn common(task: Task, state: PtrToCallState, slot: NonNull<T>) -> Self {
        WaitingTask::Common(task, state, slot)
    }

    pub(crate) fn in_selector(
        task: TaskInSelectBranch,
        state: PtrToCallState,
        slot: NonNull<T>,
    ) -> Self {
        WaitingTask::InSelector(task, state, slot)
    }
}

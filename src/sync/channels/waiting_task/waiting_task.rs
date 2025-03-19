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

    pub(crate) fn is_local(&self) -> bool {
        match self {
            Self::Common(task, _, _) => task.is_local(),
            Self::InSelector(task, _, _) => task.is_local(),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_with_usize_for_tests(value: usize) -> Self {
        use crate::runtime::Locality;
        use std::mem;

        Self::Common(
            unsafe { Task::from_future(async {}, Locality::local()) },
            CallStatePtr::new(unsafe {
                mem::transmute::<usize, &mut crate::sync::channels::CallState>(8)
            }),
            unsafe { mem::transmute::<usize, NonNull<T>>(value) },
        )
    }

    #[cfg(test)]
    pub(crate) fn extract_usize_for_tests(&self) -> usize {
        match self {
            WaitingTask::Common(_, _, slot) => slot.as_ptr() as usize,
            WaitingTask::InSelector(_, _, _) => unreachable!(),
        }
    }
}

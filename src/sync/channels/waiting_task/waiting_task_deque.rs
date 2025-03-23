// TODO docs
use crate::local_executor;
use crate::sync::channels::state::CallStatePtr;
use crate::sync::channels::waiting_task::sender_receiver_deque::{
    SenderReceiverQueue, SenderReceiverQueueOption,
};
use crate::sync::channels::waiting_task::waiting_task::WaitingTask;
use crate::sync::channels::waiting_task::{PopIfAcquiredResult, TaskInSelectBranch};
use crate::utils::assert_hint;
use std::cell::UnsafeCell;
use std::mem::ManuallyDrop;
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::ptr;
use std::ptr::NonNull;

thread_local! {
    /// A pool of [`waiting task`](WaitingTask) deques.
    static WAITING_TASK_DEQUE_POOL: UnsafeCell<Vec<SenderReceiverQueue>> = const { UnsafeCell::new(Vec::new()) };
}

/// Acquires a [`WaitingTaskDeque`] from the pool.
fn acquire_waiting_task_deque_from_pool<T>() -> SenderReceiverQueue<T> {
    WAITING_TASK_DEQUE_POOL.with(|pool| {
        unsafe { &mut *pool.get().cast::<Vec<SenderReceiverQueue<T>>>() }
            .pop()
            .map_or_else(SenderReceiverQueue::new, |deque| deque)
    })
}

/// Puts the provided [`WaitingTaskDeque`] back into the pool.
fn put_waiting_task_deque_to_pool<T>(deque: SenderReceiverQueue<T>) {
    WAITING_TASK_DEQUE_POOL
        .with(|pool| unsafe { &mut *pool.get().cast::<Vec<SenderReceiverQueue<T>>>() }.push(deque));
}

macro_rules! generate_struct {
    ($name:ident) => {
        /// A deque of waiting tasks.
        ///
        /// It expects that `State` is [`SendCallState`](CallState)
        /// or [`CallState`](CallState)
        /// and `T` is a type of channel data.
        pub(crate) struct $name<T> {
            queue: ManuallyDrop<SenderReceiverQueue<T>>,
        }

        impl<T> $name<T> {
            pub(crate) fn number_of_senders_or_receivers(&self) -> isize {
                self.queue.number_of_senders_or_receivers()
            }
        }
    };
}

macro_rules! generate_new {
    () => {
        pub(crate) fn new() -> Self {
            Self {
                queue: ManuallyDrop::new(acquire_waiting_task_deque_from_pool()),
            }
        }
    };
}

macro_rules! generate_push_back {
    ($should_be_local:expr) => {
        pub(crate) fn push_back_sender(&mut self, task: WaitingTask<T>) {
            debug_assert_eq!(task.is_local(), $should_be_local);

            self.queue.push_sender(task);
        }

        pub(crate) fn push_back_receiver(&mut self, task: WaitingTask<T>) {
            debug_assert_eq!(task.is_local(), $should_be_local);

            self.queue.push_receiver(task);
        }
    };
}

macro_rules! generate_try_pop_and_call {
    () => {
        /// Tries to pop [`waiting task`](WaitingTask) from the deque and executes it.
        ///
        /// Returns `true` if [`waiting task`](WaitingTask) was popped and executed,
        /// otherwise returns `false`.
        pub(crate) fn try_pop_front_receiver_and_call<SetterFn>(
            &mut self,
            mut setter_fn: SetterFn,
        ) -> bool
        where
            SetterFn: FnMut(CallStatePtr, NonNull<T>),
        {
            while matches!(self.queue.option(), SenderReceiverQueueOption::Receiver) {
                if unsafe { self.try_pop_and_call::<true, _>(&mut setter_fn) } {
                    return true;
                }
            }

            false
        }

        /// Tries to pop [`waiting task`](WaitingTask) from the deque and executes it.
        ///
        /// Returns `true` if [`waiting task`](WaitingTask) was popped and executed,
        /// otherwise returns `false`.
        pub(crate) fn try_pop_front_sender_and_call<SetterFn>(
            &mut self,
            mut setter_fn: SetterFn,
        ) -> bool
        where
            SetterFn: FnMut(CallStatePtr, NonNull<T>),
        {
            while matches!(self.queue.option(), SenderReceiverQueueOption::Sender) {
                if unsafe { self.try_pop_and_call::<false, _>(&mut setter_fn) } {
                    return true;
                }
            }

            false
        }
    };
}

macro_rules! generate_clear {
    () => {
        pub(crate) fn clear(&mut self) {
            let option = self.queue.option();

            match option {
                SenderReceiverQueueOption::Empty => {
                    // nothing to clear
                }
                SenderReceiverQueueOption::Sender => {
                    while self.try_pop_front_sender_and_call(|state_ptr, _| {
                        state_ptr.set_to_closed();
                    }) {}
                }
                SenderReceiverQueueOption::Receiver => {
                    while self.try_pop_front_receiver_and_call(|state_ptr, _| {
                        state_ptr.set_to_closed();
                    }) {}
                }
            }
        }
    };
}

macro_rules! generate_drop {
    () => {
        fn drop(&mut self) {
            debug_assert!(self.queue.is_empty());

            if self.queue.capacity() < 128 {
                put_waiting_task_deque_to_pool(unsafe { ptr::read(ptr::from_ref(&*self.queue)) });

                return;
            }

            unsafe { ManuallyDrop::drop(&mut self.queue) };
        }
    };
}

macro_rules! generate_process_pop_if_acquired_result {
    ($this:expr, $is_receiver_pop:expr, $res:expr, $other_task_in_select_branch:expr, $call_state:expr, $slot:expr) => {
        match $res {
            PopIfAcquiredResult::Ok => {
                return PopIfAcquiredResult::Ok;
            }
            PopIfAcquiredResult::NoData => {}
            PopIfAcquiredResult::NotAcquired => {
                if $is_receiver_pop {
                    $this.queue.push_receiver(WaitingTask::InSelector(
                        $other_task_in_select_branch,
                        $call_state,
                        $slot,
                    ));
                } else {
                    $this.queue.push_sender(WaitingTask::InSelector(
                        $other_task_in_select_branch,
                        $call_state,
                        $slot,
                    ));
                }

                return PopIfAcquiredResult::NotAcquired;
            }
        }
    };
}

generate_struct!(WaitingTaskLocalDequeGuard);

impl<T> WaitingTaskLocalDequeGuard<T> {
    generate_new!();

    generate_push_back!(true);

    /// Pops a [`waiting task`](WaitingTask) from the deque, next calls provided function,
    /// and after it execute the task.
    ///
    /// Return `false` if next task can not be executed. Otherwise, returns `true`.
    ///
    /// `setter_fn` is a function that must write/read data to/from receiver/sender.
    #[must_use]
    unsafe fn try_pop_and_call<const IS_RECEIVER_POP: bool, SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
    ) -> bool
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        assert_hint(
            !self.queue.is_empty(),
            "deque must not be empty in `try_pop_and_call`",
        );

        let data = if IS_RECEIVER_POP {
            unsafe { self.queue.pop_receiver().unwrap_unchecked() }
        } else {
            unsafe { self.queue.pop_sender().unwrap_unchecked() }
        };

        match data {
            WaitingTask::Common(task, call_state, slot) => {
                setter_fn(call_state, slot);

                local_executor().exec_task(task);

                true
            }

            WaitingTask::InSelector(mut task_in_select, call_state, slot) => {
                unsafe { task_in_select.acquire_once_local() }.is_some_and(|task| {
                    setter_fn(call_state, slot);

                    local_executor().exec_task(task);

                    true
                })
            }
        }
    }

    generate_try_pop_and_call!();

    /// Pops a [`waiting task`](WaitingTask) from the deque if [`TaskInSelectBranch`] was acquired,
    /// next calls provided function, and after it execute the task.
    ///
    /// # Arguments
    ///
    /// * `setter_fn` is a function that must write/read data to/from receiver/sender.
    ///
    /// * `task_in_select_branch` is a mutable reference to [`TaskInSelectBranch`].
    #[must_use]
    fn try_pop_and_call_if_acquired<const IS_RECEIVER_POP: bool, SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
        task_in_select_branch: &mut TaskInSelectBranch,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        while (IS_RECEIVER_POP
            && matches!(self.queue.option(), SenderReceiverQueueOption::Receiver))
            || (!IS_RECEIVER_POP
                && matches!(self.queue.option(), SenderReceiverQueueOption::Sender))
        {
            // TODO acquire once and acquire once in `shared` below

            let data = if IS_RECEIVER_POP {
                unsafe { self.queue.pop_receiver().unwrap_unchecked() }
            } else {
                unsafe { self.queue.pop_sender().unwrap_unchecked() }
            };

            match data {
                WaitingTask::Common(task, call_state, slot) => {
                    unsafe {
                        if let Some(acquired_task) = task_in_select_branch.acquire_once_local() {
                            setter_fn(call_state, slot);

                            local_executor().exec_task(task);
                            local_executor().exec_task(acquired_task);

                            return PopIfAcquiredResult::Ok;
                        }

                        if IS_RECEIVER_POP {
                            self.queue
                                .push_receiver(WaitingTask::Common(task, call_state, slot));
                        } else {
                            self.queue
                                .push_sender(WaitingTask::Common(task, call_state, slot));
                        }

                        return PopIfAcquiredResult::NotAcquired;
                    };
                }

                WaitingTask::InSelector(mut other_task_in_select_branch, call_state, slot) => {
                    generate_process_pop_if_acquired_result!(
                        self,
                        IS_RECEIVER_POP,
                        unsafe {
                            task_in_select_branch.try_acquire_two_local_tasks_in_select(
                                &mut other_task_in_select_branch,
                                &mut setter_fn,
                                call_state,
                                slot,
                            )
                        },
                        other_task_in_select_branch,
                        call_state,
                        slot
                    );
                }
            }
        }

        PopIfAcquiredResult::NoData
    }

    /// Tries to pop [`waiting task`](WaitingTask) from the deque and executes it only
    /// if [`TaskInSelectBranch`] was acquired.
    pub(crate) fn try_pop_front_receiver_and_call_if_acquired<SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
        task_in_select_branch: &mut TaskInSelectBranch,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        self.try_pop_and_call_if_acquired::<true, _>(&mut setter_fn, task_in_select_branch)
    }

    /// Tries to pop [`waiting task`](WaitingTask) from the deque and executes it only
    /// if [`TaskInSelectBranch`] was acquired.
    pub(crate) fn try_pop_front_sender_and_call_if_acquired<SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
        task_in_select_branch: &mut TaskInSelectBranch,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        self.try_pop_and_call_if_acquired::<false, _>(&mut setter_fn, task_in_select_branch)
    }

    generate_clear!();
}

impl<T> Drop for WaitingTaskLocalDequeGuard<T> {
    generate_drop!();
}

generate_struct!(WaitingTaskSharedDequeGuard);

impl<T> WaitingTaskSharedDequeGuard<T> {
    generate_new!();

    generate_push_back!(false);

    /// Pops a [`waiting task`](WaitingTask) from the deque, next calls provided function,
    /// and after it execute the task.
    ///
    /// Return `false` if next task can not be executed. Otherwise, returns `true`.
    ///
    /// * `setter_fn` is a function that must write/read data to/from receiver/sender.
    #[must_use]
    unsafe fn try_pop_and_call<const IS_RECEIVER_POP: bool, SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
    ) -> bool
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        let data = if IS_RECEIVER_POP {
            unsafe { self.queue.pop_receiver().unwrap_unchecked() }
        } else {
            unsafe { self.queue.pop_sender().unwrap_unchecked() }
        };
        match data {
            WaitingTask::Common(task, call_state, slot) => {
                setter_fn(call_state, slot);

                local_executor().exec_task(task); // TODO spawn

                true
            }
            WaitingTask::InSelector(task_in_select, call_state, slot) => {
                task_in_select.acquire_once().is_some_and(|task| {
                    setter_fn(call_state, slot);

                    local_executor().spawn_shared_task(task);

                    true
                })
            }
        }
    }

    /// Pops a [`waiting task`](WaitingTask) from the deque if [`TaskInSelectBranch`] was acquired,
    /// next calls provided function, and after it execute the task.
    ///
    /// # Arguments
    ///
    /// * `setter_fn` is a function that must write/read data to/from receiver/sender.
    ///
    /// * `task_in_select_branch` is a mutable reference to [`TaskInSelectBranch`].
    #[must_use]
    fn try_pop_and_call_if_acquired<const IS_RECEIVER_POP: bool, SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
        task_in_select_branch: &mut TaskInSelectBranch,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        while (IS_RECEIVER_POP
            && matches!(self.queue.option(), SenderReceiverQueueOption::Receiver))
            || (!IS_RECEIVER_POP
                && matches!(self.queue.option(), SenderReceiverQueueOption::Sender))
        {
            let data = if IS_RECEIVER_POP {
                unsafe { self.queue.pop_receiver().unwrap_unchecked() }
            } else {
                unsafe { self.queue.pop_sender().unwrap_unchecked() }
            };

            match data {
                WaitingTask::Common(task, call_state, slot) => {
                    if let Some(acquired_task) = task_in_select_branch.acquire_once() {
                        setter_fn(call_state, slot);

                        local_executor().spawn_shared_task(task);
                        local_executor().spawn_shared_task(acquired_task);

                        return PopIfAcquiredResult::Ok;
                    }

                    if IS_RECEIVER_POP {
                        self.queue
                            .push_receiver(WaitingTask::Common(task, call_state, slot));
                    } else {
                        self.queue
                            .push_sender(WaitingTask::Common(task, call_state, slot));
                    }

                    return PopIfAcquiredResult::NotAcquired;
                }

                WaitingTask::InSelector(other_task_in_select_branch, call_state, slot) => {
                    generate_process_pop_if_acquired_result!(
                        self,
                        IS_RECEIVER_POP,
                        unsafe {
                            task_in_select_branch.try_acquire_two_shared_tasks_in_select(
                                &other_task_in_select_branch,
                                &mut setter_fn,
                                call_state,
                                slot,
                            )
                        },
                        other_task_in_select_branch,
                        call_state,
                        slot
                    );
                }
            }
        }

        PopIfAcquiredResult::NoData
    }

    generate_try_pop_and_call!();

    /// Tries to pop [`waiting task`](WaitingTask) from the deque and executes it only
    /// if [`TaskInSelectBranch`] was acquired.
    pub(crate) fn try_pop_front_receiver_and_call_if_acquired<SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
        task_in_select_branch: &mut TaskInSelectBranch,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        self.try_pop_and_call_if_acquired::<true, _>(&mut setter_fn, task_in_select_branch)
    }

    /// Tries to pop [`waiting task`](WaitingTask) from the deque and executes it only
    /// if [`TaskInSelectBranch`] was acquired.
    pub(crate) fn try_pop_front_sender_and_call_if_acquired<SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
        task_in_select_branch: &mut TaskInSelectBranch,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        self.try_pop_and_call_if_acquired::<false, _>(&mut setter_fn, task_in_select_branch)
    }

    generate_clear!();
}

#[allow(
    clippy::non_send_fields_in_send_ty,
    reason = "We guarantee that it is `Send`"
)]
unsafe impl<T: Send> Send for WaitingTaskSharedDequeGuard<T> {}
unsafe impl<T: Send> Sync for WaitingTaskSharedDequeGuard<T> {}
impl<T: UnwindSafe> UnwindSafe for WaitingTaskSharedDequeGuard<T> {}
impl<T: RefUnwindSafe> RefUnwindSafe for WaitingTaskSharedDequeGuard<T> {}

impl<T> Drop for WaitingTaskSharedDequeGuard<T> {
    generate_drop!();
}

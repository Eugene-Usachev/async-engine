use crate::local_executor;
use crate::runtime::waiting_task::WaitingTask;
use crate::sync::channels::state::CallStatePtr;
use crate::sync::channels::waiting_task::sender_receiver_deque::{
    SenderReceiverQueue, SenderReceiverQueueOption,
};
use crate::sync::channels::waiting_task::{PopIfAcquiredResult, TaskInSelectBranch};
use crate::utils::assert_hint;
use crate::utils::unreachable_hint;
use std::cell::UnsafeCell;
use std::mem::ManuallyDrop;
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::ptr::NonNull;

/// Contains `size_of::<WaitingTask<()>>()`.
const WAITING_TASKS_SIZE: usize = size_of::<WaitingTask<()>>();

/// `WaitingSelectTaskDequePool` is used to reuse [`WaitingTaskLocalDequeGuard`]
/// and [`WaitingTaskSharedDequeGuard`].
struct WaitingSelectTaskDequePool<T = ()> {
    queues: Vec<SenderReceiverQueue<T>>,
    /// All queues with its capacity in bytes.
    bytes_allocated: usize,
}

impl<T> WaitingSelectTaskDequePool<T> {
    /// Creates [`WaitingSelectTaskDequePool`].
    const fn new() -> Self {
        Self {
            queues: Vec::new(),
            bytes_allocated: 0,
        }
    }

    /// Shrinks the pool if it is necessary.
    fn maybe_shrink(&mut self) {
        let average = (self.bytes_allocated
            - self.queues.len() * size_of::<SenderReceiverQueue<()>>())
            / self.queues.len(); // the average is not the median, but it is too expensive to calculate the median.

        self.queues.retain(|queue| {
            if queue.capacity() < average {
                true
            } else {
                self.bytes_allocated -= queue.capacity() * WAITING_TASKS_SIZE;
                self.bytes_allocated -= size_of::<SenderReceiverQueue<()>>();

                false
            }
        });

        if self.bytes_allocated < 48 * 1024 * 1024 {
            // nice shrink

            return;
        }

        // Probably, too many queues.

        self.bytes_allocated = 0;
        let half_of_len = self.queues.len() >> 1;
        let mut i = 0;

        self.queues.retain(|queue| {
            if i > half_of_len {
                return false;
            }

            self.bytes_allocated += queue.capacity() * WAITING_TASKS_SIZE;
            self.bytes_allocated += size_of::<ManuallyDrop<SenderReceiverQueue<()>>>();

            i += 1;

            true
        });
    }

    /// Returns a [`SenderReceiverQueue<T>`] to the pool.
    fn push(&mut self, deque: SenderReceiverQueue<T>) {
        self.bytes_allocated += deque.capacity() * WAITING_TASKS_SIZE;
        self.bytes_allocated += size_of::<ManuallyDrop<SenderReceiverQueue<()>>>();

        self.queues.push(deque);

        if self.bytes_allocated <= 64 * 1024 * 1024 {
            return;
        }

        self.maybe_shrink();
    }

    /// Pops a [`SenderReceiverQueue<T>`] from the pool.
    fn pop(&mut self) -> Option<SenderReceiverQueue<T>> {
        if let Some(queue) = self.queues.pop() {
            self.bytes_allocated -= queue.capacity() * WAITING_TASKS_SIZE;
            self.bytes_allocated -= size_of::<ManuallyDrop<SenderReceiverQueue<()>>>();

            return Some(queue);
        }

        None
    }
}

thread_local! {
    /// A pool of [`waiting task`](WaitingTask) deques.
    static WAITING_TASK_DEQUE_POOL: UnsafeCell<WaitingSelectTaskDequePool<()>> = const { UnsafeCell::new(WaitingSelectTaskDequePool::new()) };
}

/// Acquires a [`WaitingSelectTaskDeque`] from the pool.
fn acquire_waiting_task_deque_from_pool<T>() -> SenderReceiverQueue<T> {
    WAITING_TASK_DEQUE_POOL.with(|pool| {
        unsafe { &mut *pool.get().cast::<WaitingSelectTaskDequePool<T>>() }
            .pop()
            .map_or_else(SenderReceiverQueue::new, |deque| deque)
    })
}

/// Puts the provided [`WaitingSelectTaskDeque`] back into the pool.
fn put_waiting_task_deque_to_pool<T>(deque: SenderReceiverQueue<T>) {
    WAITING_TASK_DEQUE_POOL.with(|pool| {
        let pool = unsafe { &mut *pool.get().cast::<WaitingSelectTaskDequePool<T>>() };

        pool.push(deque);
    });
}

macro_rules! generate_struct {
    ($name:ident) => {
        /// A deque of waiting tasks.
        pub(crate) struct $name<T> {
            queue: ManuallyDrop<SenderReceiverQueue<T>>,
        }

        impl<T> $name<T> {
            /// Return a number of senders or receivers of the underlying queue.
            pub(crate) fn number_of_senders_or_receivers(&self) -> isize {
                self.queue.number_of_senders_or_receivers()
            }
        }
    };
}

macro_rules! generate_new {
    () => {
        /// Creates new object from the [`SenderReceiverQueue`].
        pub(crate) fn new() -> Self {
            Self {
                queue: ManuallyDrop::new(acquire_waiting_task_deque_from_pool()),
            }
        }
    };
}

macro_rules! generate_push_back {
    () => {
        pub(crate) fn push_back_sender(&mut self, task: WaitingTask<T>) {
            self.queue.push_sender(task);
        }

        pub(crate) fn push_back_receiver(&mut self, task: WaitingTask<T>) {
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

            put_waiting_task_deque_to_pool(unsafe { ManuallyDrop::take(&mut self.queue) });
        }
    };
}

macro_rules! generate_process_pop_if_acquired_result {
    ($this:expr, $is_receiver_pop:expr, $res:expr, $other_task_in_select_branch:expr, $call_state:expr, $slot:expr) => {{
        let res = $res;

        match res {
            PopIfAcquiredResult::NoData(this_task_in_select_branch) => this_task_in_select_branch,

            PopIfAcquiredResult::Ok => return PopIfAcquiredResult::Ok,

            PopIfAcquiredResult::NotAcquired(other_task_in_select_branch) => {
                if $is_receiver_pop {
                    $this.queue.push_receiver(WaitingTask::InSelector(
                        other_task_in_select_branch,
                        $call_state,
                        $slot,
                    ));
                } else {
                    $this.queue.push_sender(WaitingTask::InSelector(
                        other_task_in_select_branch,
                        $call_state,
                        $slot,
                    ));
                }

                return PopIfAcquiredResult::AlreadyAcquired;
            }

            _ => unreachable_hint(),
        }
    }};
}

generate_struct!(WaitingTaskLocalDequeGuard);

impl<T> WaitingTaskLocalDequeGuard<T> {
    generate_new!();

    generate_push_back!();

    /// Pops a [`waiting task`](WaitingTask) from the deque, next calls the provided function,
    /// and after it executes the task.
    ///
    /// Return `false` if the next task cannot be executed. Otherwise, returns `true`.
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

            WaitingTask::InSelector(task_in_select, call_state, slot) => {
                assert_hint(
                    task_in_select.is_local(),
                    "task_in_select_branch must be local in WaitingTaskLocalDequeGuard::try_pop_and_call",
                );

                task_in_select.acquire_once().is_some_and(|task| {
                    setter_fn(call_state, slot);

                    local_executor().exec_task(task);

                    true
                })
            }

            WaitingTask::CommonWithDeadline(task_with_deadline, call_state, slot) => {
                assert_hint(
                    task_with_deadline.is_local(),
                    "task_with_deadline must be local in WaitingTaskLocalDequeGuard::try_pop_and_call",
                );

                task_with_deadline.try_wake_with(|| setter_fn(call_state, slot))
            }
        }
    }

    generate_try_pop_and_call!();

    /// Pops a [`waiting task`](WaitingTask) from the deque if [`TaskInSelectBranch`] was acquired,
    /// next calls provided function, and after it executes the task.
    #[must_use]
    fn try_pop_and_call_if_acquired<const IS_RECEIVER_POP: bool, SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
        mut task_in_select_branch: TaskInSelectBranch,
    ) -> PopIfAcquiredResult<TaskInSelectBranch>
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        assert_hint(
            task_in_select_branch.is_local(),
            "task_in_select_branch must be local in WaitingTaskLocalDequeGuard::try_pop_and_call_if_acquired",
        );

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
                    return if let Some(acquired_task) = task_in_select_branch.acquire_once() {
                        setter_fn(call_state, slot);

                        local_executor().exec_task(task);
                        local_executor().exec_task(acquired_task);

                        PopIfAcquiredResult::Ok
                    } else {
                        if IS_RECEIVER_POP {
                            self.queue
                                .push_receiver(WaitingTask::Common(task, call_state, slot));
                        } else {
                            self.queue
                                .push_sender(WaitingTask::Common(task, call_state, slot));
                        }

                        PopIfAcquiredResult::AlreadyAcquired
                    };
                }

                WaitingTask::InSelector(other_task_in_select_branch, call_state, slot) => {
                    task_in_select_branch = generate_process_pop_if_acquired_result!(
                        self,
                        IS_RECEIVER_POP,
                        unsafe {
                            task_in_select_branch.try_acquire_two_tasks_in_select(
                                other_task_in_select_branch,
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

                WaitingTask::CommonWithDeadline(task_with_deadline, call_state, slot) => {
                    task_in_select_branch = {
                        let res = unsafe {
                            task_in_select_branch.try_acquire_with_task_with_deadline_in_select(
                                task_with_deadline,
                                &mut |task_with_deadline| {
                                    assert_hint(
                                        task_with_deadline.is_local(),
                                        "task_with_deadline must be local in WaitingTaskLocalDequeGuard::try_pop_and_call_if_acquired",
                                    );

                                    task_with_deadline.try_wake_with(|| setter_fn(call_state, slot))
                                })
                        };

                        match res {
                            PopIfAcquiredResult::NoData(this_task_in_select_branch) => {
                                this_task_in_select_branch
                            }

                            PopIfAcquiredResult::Ok => return PopIfAcquiredResult::Ok,

                            PopIfAcquiredResult::NotAcquired(task_with_deadline) => {
                                if IS_RECEIVER_POP {
                                    self.queue.push_receiver(WaitingTask::CommonWithDeadline(
                                        task_with_deadline,
                                        call_state,
                                        slot,
                                    ));
                                } else {
                                    self.queue.push_sender(WaitingTask::CommonWithDeadline(
                                        task_with_deadline,
                                        call_state,
                                        slot,
                                    ));
                                }

                                return PopIfAcquiredResult::AlreadyAcquired;
                            }

                            PopIfAcquiredResult::AlreadyAcquired => unreachable_hint(),
                        }
                    };
                }
            }
        }

        PopIfAcquiredResult::NoData(task_in_select_branch)
    }

    /// Tries to pop [`waiting task`](WaitingTask) from the deque and executes it only
    /// if [`TaskInSelectBranch`] was acquired.
    pub(crate) fn try_pop_front_receiver_and_call_if_acquired<SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
        task_in_select_branch: TaskInSelectBranch,
    ) -> PopIfAcquiredResult<TaskInSelectBranch>
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
        task_in_select_branch: TaskInSelectBranch,
    ) -> PopIfAcquiredResult<TaskInSelectBranch>
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

    generate_push_back!();

    /// Pops a [`waiting task`](WaitingTask) from the deque, next calls provided function,
    /// and after it executes the task.
    ///
    /// Return `false` if a next task cannot be executed. Otherwise, returns `true`.
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

                local_executor().spawn_shared_task(task);

                true
            }
            WaitingTask::InSelector(task_in_select, call_state, slot) => {
                assert_hint(
                    !task_in_select.is_local(),
                    "task_in_select_branch must be shared in WaitingTaskSharedDequeGuard::try_pop_and_call",
                );

                task_in_select.acquire_once().is_some_and(|task| {
                    setter_fn(call_state, slot);

                    local_executor().spawn_shared_task(task);

                    true
                })
            }

            WaitingTask::CommonWithDeadline(task_with_deadline, call_state, slot) => {
                assert_hint(
                    !task_with_deadline.is_local(),
                    "task_with_deadline must be shared in WaitingTaskSharedDequeGuard::try_pop_and_call",
                );

                task_with_deadline.try_wake_with(|| setter_fn(call_state, slot))
            }
        }
    }

    /// Pops a [`waiting task`](WaitingTask) from the deque if [`TaskInSelectBranch`] was acquired,
    /// next calls provided function, and after it executes the task.
    #[must_use]
    fn try_pop_and_call_if_acquired<const IS_RECEIVER_POP: bool, SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
        mut task_in_select_branch: TaskInSelectBranch,
    ) -> PopIfAcquiredResult<TaskInSelectBranch>
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

                    return PopIfAcquiredResult::AlreadyAcquired;
                }

                WaitingTask::InSelector(other_task_in_select_branch, call_state, slot) => {
                    task_in_select_branch = generate_process_pop_if_acquired_result!(
                        self,
                        IS_RECEIVER_POP,
                        unsafe {
                            task_in_select_branch.try_acquire_two_tasks_in_select(
                                other_task_in_select_branch,
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

                WaitingTask::CommonWithDeadline(task_with_deadline, call_state, slot) => {
                    task_in_select_branch = {
                        let res = unsafe {
                            task_in_select_branch.try_acquire_with_task_with_deadline_in_select(
                                task_with_deadline,
                                &mut |task_with_deadline| {
                                    assert_hint(
                                        !task_with_deadline.is_local(),
                                        "task_with_deadline must be shared in WaitingTaskSharedDequeGuard::try_pop_and_call_if_acquired",
                                    );

                                    task_with_deadline.try_wake_with(|| setter_fn(call_state, slot))
                                })
                        };

                        match res {
                            PopIfAcquiredResult::NoData(this_task_in_select_branch) => {
                                this_task_in_select_branch
                            }

                            PopIfAcquiredResult::Ok => return PopIfAcquiredResult::Ok,

                            PopIfAcquiredResult::NotAcquired(task_with_deadline) => {
                                if IS_RECEIVER_POP {
                                    self.queue.push_receiver(WaitingTask::CommonWithDeadline(
                                        task_with_deadline,
                                        call_state,
                                        slot,
                                    ));
                                } else {
                                    self.queue.push_sender(WaitingTask::CommonWithDeadline(
                                        task_with_deadline,
                                        call_state,
                                        slot,
                                    ));
                                }

                                return PopIfAcquiredResult::AlreadyAcquired;
                            }

                            PopIfAcquiredResult::AlreadyAcquired => unreachable_hint(),
                        }
                    };
                }
            }
        }

        PopIfAcquiredResult::NoData(task_in_select_branch)
    }

    generate_try_pop_and_call!();

    /// Tries to pop [`waiting task`](WaitingTask) from the deque and executes it only
    /// if [`TaskInSelectBranch`] was acquired.
    pub(crate) fn try_pop_front_receiver_and_call_if_acquired<SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
        task_in_select_branch: TaskInSelectBranch,
    ) -> PopIfAcquiredResult<TaskInSelectBranch>
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
        task_in_select_branch: TaskInSelectBranch,
    ) -> PopIfAcquiredResult<TaskInSelectBranch>
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

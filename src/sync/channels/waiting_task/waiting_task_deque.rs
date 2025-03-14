// TODO docs
use crate::local_executor;
use crate::sync::channels::state::CallStatePtr;
use crate::sync::channels::waiting_task::waiting_task::WaitingTask;
use crate::sync::channels::waiting_task::{PopIfAcquiredResult, TaskInSelectBranch};
use std::cell::UnsafeCell;
use std::collections::VecDeque;
use std::mem::ManuallyDrop;
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::ptr;
use std::ptr::NonNull;

type WaitingTaskDeque<T = ()> = VecDeque<WaitingTask<T>>;

thread_local! {
    /// A pool of [`waiting task`](WaitingTask) deques.
    static WAITING_TASK_DEQUE_POOL: UnsafeCell<Vec<WaitingTaskDeque>> = const { UnsafeCell::new(Vec::new()) };
}

/// Acquires a [`WaitingTaskDeque`] from the pool.
fn acquire_waiting_task_deque_from_pool<T>() -> WaitingTaskDeque<T> {
    WAITING_TASK_DEQUE_POOL.with(|pool| {
        if let Some(deque) = unsafe { &mut *pool.get().cast::<Vec<WaitingTaskDeque<T>>>() }.pop() {
            deque
        } else {
            VecDeque::with_capacity(2)
        }
    })
}

/// Puts the provided [`WaitingTaskDeque`] back into the pool.
fn put_waiting_task_deque_to_pool<T>(deque: WaitingTaskDeque<T>) {
    if deque.capacity() < 128 {
        WAITING_TASK_DEQUE_POOL.with(|pool| {
            unsafe { &mut *pool.get().cast::<Vec<WaitingTaskDeque<T>>>() }.push(deque)
        });
    }
}

macro_rules! generate_struct {
    ($name:ident) => {
        /// A deque of waiting tasks.
        ///
        /// It expects that `State` is [`SendCallState`](CallState)
        /// or [`CallState`](CallState)
        /// and `T` is a type of channel data.
        pub(crate) struct $name<T> {
            deque: ManuallyDrop<WaitingTaskDeque<T>>,
            /// __0__ for none, __>0__ for receivers, __<0__ for senders
            number_of_senders_or_receivers: isize,
            _marker: std::marker::PhantomData<T>,
        }
    };
}

macro_rules! generate_new {
    () => {
        pub(crate) fn new() -> Self {
            Self {
                deque: ManuallyDrop::new(acquire_waiting_task_deque_from_pool()),
                number_of_senders_or_receivers: 0,
                _marker: std::marker::PhantomData,
            }
        }
    };
}

macro_rules! generate_push_back {
    ($should_be_local:expr) => {
        pub(crate) fn push_back_sender(&mut self, task: WaitingTask<T>) {
            debug_assert!(self.number_of_senders_or_receivers < 1);
            debug_assert_eq!(task.is_local(), $should_be_local);

            self.number_of_senders_or_receivers -= 1;

            self.deque.push_back(task);
        }

        pub(crate) fn push_back_receiver(&mut self, task: WaitingTask<T>) {
            debug_assert!(self.number_of_senders_or_receivers > -1);
            debug_assert_eq!(task.is_local(), $should_be_local);

            self.number_of_senders_or_receivers += 1;

            self.deque.push_back(task);
        }
    };
}

macro_rules! generate_pop_shared_task_in_selector {
    ($setter_fn:expr, $call_state:expr, $slot:expr, $task_in_select_branch:expr) => {{
        match $task_in_select_branch.acquire_once() {
            None => false,
            Some(task) => {
                $setter_fn($call_state, $slot);

                local_executor().spawn_shared_task(task);

                true
            }
        }
    }};
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
            debug_assert_eq!(
                self.number_of_senders_or_receivers.unsigned_abs(),
                self.deque.len()
            );

            while self.number_of_senders_or_receivers > 0 {
                self.number_of_senders_or_receivers -= 1;

                if unsafe { self.try_pop_and_call(&mut setter_fn) } {
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
            debug_assert_eq!(
                self.number_of_senders_or_receivers.unsigned_abs(),
                self.deque.len()
            );

            while self.number_of_senders_or_receivers < 0 {
                self.number_of_senders_or_receivers += 1;

                if unsafe { self.try_pop_and_call(&mut setter_fn) } {
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
            if self.number_of_senders_or_receivers == 0 {
                // nothing to clear
            } else if self.number_of_senders_or_receivers > 0 {
                while self.try_pop_front_receiver_and_call(|state_ptr, _| {
                    state_ptr.set_to_closed();
                }) {}
            } else {
                while self.try_pop_front_sender_and_call(|state_ptr, _| {
                    state_ptr.set_to_closed();
                }) {}
            }
        }
    };
}

macro_rules! generate_drop {
    () => {
        fn drop(&mut self) {
            debug_assert!(self.deque.is_empty());
            debug_assert_eq!(self.number_of_senders_or_receivers, 0);

            put_waiting_task_deque_to_pool(unsafe { ptr::read(ptr::from_ref(&*self.deque)) });
        }
    };
}

macro_rules! generate_process_pop_if_acquired_result {
    ($this:expr, $delta:expr, $res:expr, $other_task_in_select_branch:expr, $call_state:expr, $slot:expr) => {
        match $res {
            PopIfAcquiredResult::Ok => {
                $this.number_of_senders_or_receivers += $delta;

                return PopIfAcquiredResult::Ok;
            }
            PopIfAcquiredResult::NoData => {
                $this.number_of_senders_or_receivers += $delta;
            }
            PopIfAcquiredResult::NotAcquired => {
                $this.deque.push_front(WaitingTask::InSelector(
                    $other_task_in_select_branch,
                    $call_state,
                    $slot,
                ));

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
    /// # Arguments
    ///
    /// * `setter_fn` is a function that must write/read data to/from receiver/sender.
    ///
    /// # Safety
    ///
    /// `self.number_of_senders_or_receivers` must be not zero and the deque must not be empty.
    #[must_use]
    unsafe fn try_pop_and_call<SetterFn>(&mut self, mut setter_fn: SetterFn) -> bool
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        let data = unsafe { self.deque.pop_front().unwrap_unchecked() };
        match data {
            WaitingTask::Common(task, call_state, slot) => {
                setter_fn(call_state, slot);

                if task.is_local() {
                    local_executor().exec_task(task);
                } else {
                    local_executor().spawn_shared_task(task);
                }

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
    /// Accepts `is_all_local` to decide whether to use [`TaskInSelectBranch::acquire_once`]
    /// or [`TaskInSelectBranch::acquire_once_local`].
    ///
    /// # Arguments
    ///
    /// * `setter_fn` is a function that must write/read data to/from receiver/sender.
    ///
    /// * `task_in_select_branch` is a mutable reference to [`TaskInSelectBranch`].
    ///
    /// # Safety
    ///
    /// * `self.number_of_senders_or_receivers` must be not zero and the deque must not be empty;
    ///
    /// * called in `select`.
    #[must_use]
    unsafe fn try_pop_and_call_if_acquired<const DELTA: isize, SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
        task_in_select_branch: &mut TaskInSelectBranch,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        while self.number_of_senders_or_receivers != 0 {
            // TODO acquire once and acquire once in `shared` below

            let data = unsafe { self.deque.pop_front().unwrap_unchecked() };

            match data {
                WaitingTask::Common(task, call_state, slot) => {
                    unsafe {
                        if let Some(acquired_task) = task_in_select_branch.acquire_once_local() {
                            self.number_of_senders_or_receivers += DELTA;

                            setter_fn(call_state, slot);

                            local_executor().exec_task(task);
                            local_executor().exec_task(acquired_task);

                            return PopIfAcquiredResult::Ok;
                        }

                        self.deque
                            .push_front(WaitingTask::Common(task, call_state, slot));

                        return PopIfAcquiredResult::NotAcquired;
                    };
                }

                WaitingTask::InSelector(mut other_task_in_select_branch, call_state, slot) => {
                    generate_process_pop_if_acquired_result!(
                        self,
                        DELTA,
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
    ///
    /// Accepts `is_all_local` to decide whether to use `local` or `shared` methods.
    pub(crate) fn try_pop_front_receiver_and_call_if_acquired<SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
        task_in_select_branch: &mut TaskInSelectBranch,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        unsafe { self.try_pop_and_call_if_acquired::<-1, _>(&mut setter_fn, task_in_select_branch) }
    }

    /// Tries to pop [`waiting task`](WaitingTask) from the deque and executes it only
    /// if [`TaskInSelectBranch`] was acquired.
    ///
    /// Accepts `is_all_local` to decide whether to use [`TaskInSelectBranch::acquire_once`]
    /// or [`TaskInSelectBranch::acquire_once_local`].
    pub(crate) fn try_pop_front_sender_and_call_if_acquired<SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
        task_in_select_branch: &mut TaskInSelectBranch,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        unsafe { self.try_pop_and_call_if_acquired::<1, _>(&mut setter_fn, task_in_select_branch) }
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
    /// # Arguments
    ///
    /// * `setter_fn` is a function that must write/read data to/from receiver/sender.
    ///
    /// # Safety
    ///
    /// `self.number_of_senders_or_receivers` must be not zero and the deque must not be empty.
    #[must_use]
    unsafe fn try_pop_and_call<SetterFn>(&mut self, mut setter_fn: SetterFn) -> bool
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        let data = unsafe { self.deque.pop_front().unwrap_unchecked() };
        match data {
            WaitingTask::Common(task, call_state, slot) => {
                setter_fn(call_state, slot);

                local_executor().spawn_shared_task(task);

                true
            }
            WaitingTask::InSelector(task_in_select, call_state, slot) => {
                generate_pop_shared_task_in_selector!(setter_fn, call_state, slot, task_in_select)
            }
        }
    }

    /// Pops a [`waiting task`](WaitingTask) from the deque if [`TaskInSelectBranch`] was acquired,
    /// next calls provided function, and after it execute the task.
    ///
    /// Accepts `IS_ALL_LOCAL` to decide whether to use [`TaskInSelectBranch::acquire_once`]
    /// or [`TaskInSelectBranch::acquire_once_local`].
    ///
    /// # Arguments
    ///
    /// * `setter_fn` is a function that must write/read data to/from receiver/sender.
    ///
    /// * `task_in_select_branch` is a mutable reference to [`TaskInSelectBranch`].
    ///
    /// # Safety
    ///
    /// * `self.number_of_senders_or_receivers` must be not zero and the deque must not be empty;
    ///
    /// * called in `select`.
    #[must_use]
    unsafe fn try_pop_and_call_if_acquired<const DELTA: isize, SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
        task_in_select_branch: &mut TaskInSelectBranch,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        while self.number_of_senders_or_receivers != 0 {
            let data = unsafe { self.deque.pop_front().unwrap_unchecked() };

            match data {
                WaitingTask::Common(task, call_state, slot) => {
                    if let Some(acquired_task) = task_in_select_branch.acquire_once() {
                        self.number_of_senders_or_receivers += DELTA;

                        setter_fn(call_state, slot);

                        local_executor().spawn_shared_task(task);
                        local_executor().spawn_shared_task(acquired_task);

                        return PopIfAcquiredResult::Ok;
                    }

                    self.deque
                        .push_front(WaitingTask::Common(task, call_state, slot));

                    return PopIfAcquiredResult::NotAcquired;
                }

                WaitingTask::InSelector(other_task_in_select_branch, call_state, slot) => {
                    generate_process_pop_if_acquired_result!(
                        self,
                        DELTA,
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
    ///
    /// Accepts `IS_ALL_LOCAL` to decide whether to use `local` or `shared` methods.
    pub(crate) fn try_pop_front_receiver_and_call_if_acquired<SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
        task_in_select_branch: &mut TaskInSelectBranch,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        unsafe { self.try_pop_and_call_if_acquired::<-1, _>(&mut setter_fn, task_in_select_branch) }
    }

    /// Tries to pop [`waiting task`](WaitingTask) from the deque and executes it only
    /// if [`TaskInSelectBranch`] was acquired.
    ///
    /// Accepts `IS_ALL_LOCAL` to decide whether to use [`TaskInSelectBranch::acquire_once`]
    /// or [`TaskInSelectBranch::acquire_once_local`].
    pub(crate) fn try_pop_front_sender_and_call_if_acquired<SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
        task_in_select_branch: &mut TaskInSelectBranch,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        unsafe { self.try_pop_and_call_if_acquired::<1, _>(&mut setter_fn, task_in_select_branch) }
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

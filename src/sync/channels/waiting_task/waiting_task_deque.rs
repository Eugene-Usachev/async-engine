// TODO docs
use crate::local_executor;
use crate::sync::channels::states::{RecvCallState, SendCallState};
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
        /// It expects that `State` is [`SendCallState`](SendCallState)
        /// or [`RecvCallState`](RecvCallState)
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
    () => {
        pub(crate) fn push_back_sender(&mut self, task: WaitingTask<T>) {
            debug_assert!(self.number_of_senders_or_receivers < 1);

            self.number_of_senders_or_receivers -= 1;

            self.deque.push_back(task);
        }

        pub(crate) fn push_back_receiver(&mut self, task: WaitingTask<T>) {
            debug_assert!(self.number_of_senders_or_receivers > -1);

            self.number_of_senders_or_receivers += 1;

            self.deque.push_back(task);
        }
    };
}

macro_rules! generate_pop_shared_task_in_selector {
    ($setter_fn:expr, $call_state:expr, $slot:expr, $task_in_select_branch:expr) => {{
        if $task_in_select_branch.is_local() {
            unsafe {
                match $task_in_select_branch.acquire_once_local() {
                    None => false,
                    Some(task) => {
                        $setter_fn($call_state.cast(), $slot);

                        local_executor().exec_task(task);

                        true
                    }
                }
            }
        } else {
            match $task_in_select_branch.acquire_once() {
                None => false,
                Some(task) => {
                    $setter_fn($call_state.cast(), $slot);

                    local_executor().spawn_shared_task(task);

                    true
                }
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
            SetterFn: FnMut(NonNull<RecvCallState>, NonNull<T>),
        {
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
            SetterFn: FnMut(NonNull<SendCallState>, NonNull<T>),
        {
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
        pub(crate) fn clear_with<SetterForRecv, SetterForSend>(
            &mut self,
            recv_setter: SetterForRecv,
            send_setter: SetterForSend,
        ) where
            SetterForRecv: Fn(NonNull<RecvCallState>, NonNull<T>),
            SetterForSend: Fn(NonNull<SendCallState>, NonNull<T>),
        {
            if self.number_of_senders_or_receivers == 0 {
                // nothing to clear
            } else if self.number_of_senders_or_receivers > 0 {
                while self.try_pop_front_receiver_and_call(&recv_setter) {}
            } else {
                while self.try_pop_front_sender_and_call(&send_setter) {}
            }
        }
    };
}

macro_rules! generate_drop {
    () => {
        fn drop(&mut self) {
            put_waiting_task_deque_to_pool(unsafe { ptr::read(ptr::from_ref(&*self.deque)) });
        }
    };
}

generate_struct!(WaitingTaskLocalDequeGuard);

impl<T> WaitingTaskLocalDequeGuard<T> {
    generate_new!();

    generate_push_back!();

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
    unsafe fn try_pop_and_call<State, SetterFn>(&mut self, mut setter_fn: SetterFn) -> bool
    where
        SetterFn: FnMut(NonNull<State>, NonNull<T>),
    {
        let data = unsafe { self.deque.pop_front().unwrap_unchecked() };
        match data {
            WaitingTask::Common(task, call_state, slot) => {
                setter_fn(call_state.cast(), slot);

                if task.is_local() {
                    local_executor().exec_task(task);
                } else {
                    local_executor().spawn_shared_task(task);
                }

                true
            }

            WaitingTask::InSelector(mut task_in_select, call_state, slot) => {
                if task_in_select.is_local() {
                    match task_in_select.acquire_once_local() {
                        None => false,

                        Some(task) => {
                            setter_fn(call_state.cast(), slot);

                            if task.is_local() {
                                local_executor().exec_task(task);
                            } else {
                                local_executor().spawn_shared_task(task);
                            }

                            true
                        }
                    }
                } else {
                    generate_pop_shared_task_in_selector!(
                        setter_fn,
                        call_state,
                        slot,
                        task_in_select
                    )
                }
            }
        }
    }

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
    unsafe fn try_pop_and_call_if_acquired<State, SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
        task_in_select_branch: &mut TaskInSelectBranch,
        is_all_local: bool,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(NonNull<State>, NonNull<T>),
    {
        let data = unsafe { self.deque.pop_front().unwrap_unchecked() };
        match data {
            WaitingTask::Common(task, call_state, slot) => {
                if is_all_local {
                    unsafe {
                        if let Some(acquired_task) = task_in_select_branch.acquire_once_local() {
                            setter_fn(call_state.cast(), slot);

                            if task.is_local() {
                                local_executor().exec_task(task);
                            } else {
                                local_executor().spawn_shared_task(task);
                            }

                            local_executor().exec_task(acquired_task);

                            PopIfAcquiredResult::Ok
                        } else {
                            self.deque
                                .push_front(WaitingTask::Common(task, call_state, slot));

                            PopIfAcquiredResult::NotAcquired
                        }
                    }
                } else if let Some(acquired_task) = task_in_select_branch.acquire_once() {
                    setter_fn(call_state.cast(), slot);

                    if task.is_local() {
                        local_executor().exec_task(task);
                    } else {
                        local_executor().spawn_shared_task(task);
                    }

                    local_executor().spawn_shared_task(acquired_task);

                    PopIfAcquiredResult::Ok
                } else {
                    self.deque
                        .push_front(WaitingTask::Common(task, call_state, slot));

                    PopIfAcquiredResult::NotAcquired
                }
            }

            WaitingTask::InSelector(mut other_task_in_select_branch, call_state, slot) => {
                if is_all_local {
                    if let Some(acquired_task) = task_in_select_branch.acquire_once_local() {
                        if other_task_in_select_branch.is_local() {
                            if let Some(other_acquired_task) =
                                other_task_in_select_branch.acquire_once_local()
                            {
                                setter_fn(call_state.cast(), slot);

                                local_executor().exec_task(other_acquired_task);

                                local_executor().exec_task(acquired_task);

                                PopIfAcquiredResult::Ok
                            } else {
                                // Other task have been acquired before. We can forget about it

                                PopIfAcquiredResult::NoData
                            }
                        } else if let Some(other_acquired_task) =
                            other_task_in_select_branch.acquire_once()
                        {
                            setter_fn(call_state.cast(), slot);

                            local_executor().spawn_shared_task(other_acquired_task);

                            local_executor().exec_task(acquired_task);

                            PopIfAcquiredResult::Ok
                        } else {
                            // Other task have been acquired before. We can forget about it

                            PopIfAcquiredResult::NoData
                        }
                    } else {
                        self.deque.push_front(WaitingTask::InSelector(
                            other_task_in_select_branch,
                            call_state,
                            slot,
                        ));

                        PopIfAcquiredResult::NotAcquired
                    }
                } else if other_task_in_select_branch.is_local() {
                    match unsafe {
                        task_in_select_branch.try_acquire_local_and_shared_tasks_in_select(
                            &mut other_task_in_select_branch,
                            setter_fn,
                            call_state.cast(),
                            slot,
                        )
                    } {
                        PopIfAcquiredResult::Ok => PopIfAcquiredResult::Ok,
                        PopIfAcquiredResult::NoData => PopIfAcquiredResult::NoData,
                        PopIfAcquiredResult::NotAcquired => {
                            self.deque.push_front(WaitingTask::InSelector(
                                other_task_in_select_branch,
                                call_state,
                                slot,
                            ));

                            PopIfAcquiredResult::NotAcquired
                        }
                    }
                } else {
                    match unsafe {
                        task_in_select_branch.try_acquire_two_shared_tasks_in_select(
                            &other_task_in_select_branch,
                            setter_fn,
                            call_state.cast(),
                            slot,
                        )
                    } {
                        PopIfAcquiredResult::Ok => PopIfAcquiredResult::Ok,
                        PopIfAcquiredResult::NoData => PopIfAcquiredResult::NoData,
                        PopIfAcquiredResult::NotAcquired => {
                            self.deque.push_front(WaitingTask::InSelector(
                                other_task_in_select_branch,
                                call_state,
                                slot,
                            ));

                            PopIfAcquiredResult::NotAcquired
                        }
                    }
                }
            }
        }
    }

    generate_try_pop_and_call!();

    /// Tries to pop [`waiting task`](WaitingTask) from the deque and executes it only
    /// if [`TaskInSelectBranch`] was acquired.
    ///
    /// Accepts `is_all_local` to decide whether to use `local` or `shared` methods.
    pub(crate) fn try_pop_front_receiver_and_call_if_acquired<SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
        task_in_select_branch: &mut TaskInSelectBranch,
        is_all_local: bool,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(NonNull<RecvCallState>, NonNull<T>),
    {
        while self.number_of_senders_or_receivers > 0 {
            unsafe {
                match self.try_pop_and_call_if_acquired(
                    &mut setter_fn,
                    task_in_select_branch,
                    is_all_local,
                ) {
                    PopIfAcquiredResult::Ok => {
                        self.number_of_senders_or_receivers -= 1;

                        return PopIfAcquiredResult::Ok;
                    }

                    PopIfAcquiredResult::NotAcquired => {
                        return PopIfAcquiredResult::NotAcquired;
                    }

                    PopIfAcquiredResult::NoData => {
                        self.number_of_senders_or_receivers -= 1;
                    }
                }
            }
        }

        PopIfAcquiredResult::NoData
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
        is_all_local: bool,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(NonNull<SendCallState>, NonNull<T>),
    {
        while self.number_of_senders_or_receivers < 0 {
            unsafe {
                match self.try_pop_and_call_if_acquired(
                    &mut setter_fn,
                    task_in_select_branch,
                    is_all_local,
                ) {
                    PopIfAcquiredResult::Ok => {
                        self.number_of_senders_or_receivers += 1;

                        return PopIfAcquiredResult::Ok;
                    }

                    PopIfAcquiredResult::NotAcquired => {
                        return PopIfAcquiredResult::NotAcquired;
                    }

                    PopIfAcquiredResult::NoData => {
                        self.number_of_senders_or_receivers += 1;
                    }
                }
            }
        }

        PopIfAcquiredResult::NoData
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
    unsafe fn try_pop_and_call<State, SetterFn>(&mut self, mut setter_fn: SetterFn) -> bool
    where
        SetterFn: FnMut(NonNull<State>, NonNull<T>),
    {
        let data = unsafe { self.deque.pop_front().unwrap_unchecked() };
        match data {
            WaitingTask::Common(task, call_state, slot) => {
                setter_fn(call_state.cast(), slot);

                if task.is_local() {
                    local_executor().exec_task(task);
                } else {
                    local_executor().spawn_shared_task(task);
                }

                true
            }
            WaitingTask::InSelector(mut task_in_select, call_state, slot) => {
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
    unsafe fn try_pop_and_call_if_acquired<State, SetterFn>(
        &mut self,
        mut setter_fn: SetterFn,
        task_in_select_branch: &mut TaskInSelectBranch,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(NonNull<State>, NonNull<T>),
    {
        let data = unsafe { self.deque.pop_front().unwrap_unchecked() };
        match data {
            WaitingTask::Common(task, call_state, slot) => {
                if let Some(acquired_task) = task_in_select_branch.acquire_once() {
                    setter_fn(call_state.cast(), slot);

                    if task.is_local() {
                        local_executor().exec_task(task);
                    } else {
                        local_executor().spawn_shared_task(task);
                    }

                    local_executor().spawn_shared_task(acquired_task);

                    PopIfAcquiredResult::Ok
                } else {
                    self.deque
                        .push_front(WaitingTask::Common(task, call_state, slot));

                    PopIfAcquiredResult::NotAcquired
                }
            }

            WaitingTask::InSelector(mut other_task_in_select_branch, call_state, slot) => {
                if other_task_in_select_branch.is_local() {
                    match unsafe {
                        task_in_select_branch.try_acquire_local_and_shared_tasks_in_select(
                            &mut other_task_in_select_branch,
                            setter_fn,
                            call_state.cast(),
                            slot,
                        )
                    } {
                        PopIfAcquiredResult::Ok => PopIfAcquiredResult::Ok,
                        PopIfAcquiredResult::NoData => PopIfAcquiredResult::NoData,
                        PopIfAcquiredResult::NotAcquired => {
                            self.deque.push_front(WaitingTask::InSelector(
                                other_task_in_select_branch,
                                call_state,
                                slot,
                            ));

                            PopIfAcquiredResult::NotAcquired
                        }
                    }
                } else {
                    match unsafe {
                        task_in_select_branch.try_acquire_two_shared_tasks_in_select(
                            &other_task_in_select_branch,
                            setter_fn,
                            call_state.cast(),
                            slot,
                        )
                    } {
                        PopIfAcquiredResult::Ok => PopIfAcquiredResult::Ok,
                        PopIfAcquiredResult::NoData => PopIfAcquiredResult::NoData,
                        PopIfAcquiredResult::NotAcquired => {
                            self.deque.push_front(WaitingTask::InSelector(
                                other_task_in_select_branch,
                                call_state,
                                slot,
                            ));

                            PopIfAcquiredResult::NotAcquired
                        }
                    }
                }
            }
        }
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
        SetterFn: FnMut(NonNull<RecvCallState>, NonNull<T>),
    {
        while self.number_of_senders_or_receivers > 0 {
            unsafe {
                match self.try_pop_and_call_if_acquired(&mut setter_fn, task_in_select_branch) {
                    PopIfAcquiredResult::Ok => {
                        self.number_of_senders_or_receivers -= 1;

                        return PopIfAcquiredResult::Ok;
                    }

                    PopIfAcquiredResult::NotAcquired => {
                        return PopIfAcquiredResult::NotAcquired;
                    }

                    PopIfAcquiredResult::NoData => {
                        self.number_of_senders_or_receivers -= 1;
                    }
                }
            }
        }

        PopIfAcquiredResult::NoData
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
        SetterFn: FnMut(NonNull<SendCallState>, NonNull<T>),
    {
        while self.number_of_senders_or_receivers < 0 {
            unsafe {
                match self.try_pop_and_call_if_acquired(&mut setter_fn, task_in_select_branch) {
                    PopIfAcquiredResult::Ok => {
                        self.number_of_senders_or_receivers += 1;

                        return PopIfAcquiredResult::Ok;
                    }

                    PopIfAcquiredResult::NotAcquired => {
                        return PopIfAcquiredResult::NotAcquired;
                    }

                    PopIfAcquiredResult::NoData => {
                        self.number_of_senders_or_receivers += 1;
                    }
                }
            }
        }

        PopIfAcquiredResult::NoData
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

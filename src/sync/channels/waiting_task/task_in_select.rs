//! This module contains the [`TaskInSelect`] and [`TaskInSelectBranch`].
//!
//! It is public only for implementation the [`select`](crate::select).
//! If you don't want to write your own `select` or understand how does `Orengine` works,
//! you don't need to read it.
/// This module provides a mechanism for managing tasks in a select context.
///
/// It defines the [`TaskInSelect`] struct, which represents a task that can be acquired
/// and released in a concurrent environment. The module also includes the [`TaskInSelectBranch`]
/// struct for handling branches associated with tasks and a thread-local pool for managing
/// [`TaskInSelect`] instances.
use crate::local_executor;
use crate::runtime::{Task, TaskWithDeadline};
use crate::sync::channels::state::CallStatePtr;
use crate::utils::Backoff;
use crate::utils::{clear_with, unlikely};
use crate::utils::{likely, unreachable_hint};
use std::cell::UnsafeCell;
use std::ptr;
use std::ptr::NonNull;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed, Release, SeqCst};
use std::sync::atomic::{AtomicUsize, fence};

/// It means that [`TaskInSelect`] is not acquired.
const NOT_ACQUIRED: usize = 0;
/// It means that [`TaskInSelect`] is acquired.
const ACQUIRED: usize = 1;
/// It means that [`TaskInSelect`] is acquiring now and the caller should wait.
const ACQUIRING_NOW: usize = 2;

/// Inner of [`TaskInSelect`]
#[repr(C)]
pub(crate) struct Inner {
    task: Task,
    resolved_branch_id: NonNull<usize>,
    state: AtomicUsize,
    ref_count: AtomicUsize,
}

/// Represents a task that can be selected and acquired.
pub struct TaskInSelect {
    inner: NonNull<Inner>,
}

unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}

impl TaskInSelect {
    /// Acquires a [`TaskInSelectBranch`] from pool and sets the `task` and `resolved_branch_id`.
    pub fn acquire_for_task(task: Task, resolved_branch_id: NonNull<usize>) -> Self {
        if cfg!(debug_assertions) {
            let mut inner = task_in_select_pool().acquire_for_task(task, resolved_branch_id);

            unsafe {
                inner.as_mut().resolved_branch_id.write(usize::MAX);
            };

            Self { inner }
        } else {
            Self {
                inner: task_in_select_pool().acquire_for_task(task, resolved_branch_id),
            }
        }
    }

    /// Returns a reference to the inner representation.
    fn inner(&self) -> &Inner {
        unsafe { self.inner.as_ref() }
    }

    /// Returns a mutable reference to the inner representation.
    fn inner_mut(&mut self) -> &mut Inner {
        unsafe { self.inner.as_mut() }
    }

    /// Sets the resolved branch ID for the task.
    ///
    /// # Panics
    ///
    /// It panics if the resolved branch ID has already been set.
    fn set_resolved_branch_id(&self, branch_id: usize) {
        debug_assert_eq!(
            unsafe { self.inner().resolved_branch_id.read() },
            usize::MAX,
            "Tried to set resolved branch id twice"
        );

        unsafe { self.inner().resolved_branch_id.write(branch_id) };
    }

    /// Releases the task after all references are gone.
    ///
    /// # Panics
    ///
    /// It panics if the task is not acquired or if the reference count is not zero.
    fn release(&self) {
        debug_assert_eq!(self.inner().ref_count.load(Acquire), 0);
        debug_assert_eq!(
            self.inner().state.load(Acquire),
            ACQUIRED,
            "Attempt to drop TaskSelect (ref count is 0) that was not acquired"
        );

        task_in_select_pool().release(self.inner);
    }
}

impl Clone for TaskInSelect {
    fn clone(&self) -> Self {
        if self.inner().task.is_local() {
            *unsafe { &mut *self.inner.as_ptr() }.ref_count.get_mut() += 1;
        } else {
            self.inner().ref_count.fetch_add(1, Relaxed);
        }

        Self { inner: self.inner }
    }
}

unsafe impl Send for TaskInSelect {}
unsafe impl Sync for TaskInSelect {}

impl Drop for TaskInSelect {
    fn drop(&mut self) {
        if self.inner().task.is_local() {
            let prev = *self.inner_mut().ref_count.get_mut();

            debug_assert!(self.inner().task.is_local());

            *self.inner_mut().ref_count.get_mut() -= 1;
            if prev != 1 {
                return;
            }

            self.release();

            return;
        }

        if self.inner().ref_count.fetch_sub(1, Release) != 1 {
            return;
        }

        fence(Acquire);

        self.release();
    }
}

/// Result of attempting to pop a [`TaskInSelectBranch`] if it is acquired.
#[repr(C)]
pub(crate) enum PopIfAcquiredResult<OtherTask> {
    /// [`TaskInSelectBranch`] was successfully acquired.
    Ok,
    /// No data available (the other [`TaskInSelectBranch`] is already acquired).
    ///
    /// It can be returned only by methods that try to acquire two [`TaskInSelectBranch`].
    ///
    /// Contains the provided [`TaskInSelectBranch`].
    NoData(TaskInSelectBranch),
    /// The provided [`TaskInSelectBranch`] is already acquired.
    ///
    /// It can be returned only by methods that try to acquire two tasks.
    ///
    /// Contains another task.
    NotAcquired(OtherTask),
    /// The [`TaskInSelectBranch`] is already acquired.
    ///
    /// It can be returned only by methods that try to acquire one task.
    AlreadyAcquired,
}

/// Contains a [`TaskInSelect`] and a branch id to resolve with it.
#[repr(C)]
#[derive(Clone)]
pub struct TaskInSelectBranch {
    task_in_select: TaskInSelect,
    associated_branch_id: usize,
}

impl TaskInSelectBranch {
    /// Creates a new `TaskInSelectBranch`.
    pub fn new(task_in_select: TaskInSelect, associated_branch_id: usize) -> Self {
        Self {
            task_in_select,
            associated_branch_id,
        }
    }

    /// Returns whether the task is `local`.
    pub fn is_local(&self) -> bool {
        self.task_in_select.inner().task.is_local()
    }

    /// Returns `true` if the task was already acquired.
    ///
    /// It is used only for free acquired tasks from deques.
    pub(crate) fn is_acquired(&self) -> bool {
        // I want to keep this function takes a shared reference, but in `local` context
        // it is free to get without `load` operations and without mutability.
        #[allow(invalid_reference_casting, reason = "Read the comment above")]
        unsafe fn get_atomic(state: &AtomicUsize) -> usize {
            *unsafe { &mut *ptr::from_ref(state).cast_mut() }.get_mut()
        }

        if self.is_local() {
            let state = unsafe { get_atomic(&self.task_in_select.inner().state) };

            state == ACQUIRED
        } else {
            self.task_in_select.inner().state.load(Acquire) == ACQUIRED
        }
    }

    /// Attempts to acquire a [`TaskInSelect`] and sets the resolved branch id on success.
    ///
    /// Returns None if it is already acquired.
    pub(crate) fn acquire_once(mut self) -> Option<Task> {
        if self.is_local() {
            let was_acquired_ref = self.task_in_select.inner_mut().state.get_mut();

            debug_assert!(*was_acquired_ref < 2);

            if *was_acquired_ref == ACQUIRED {
                None
            } else {
                *was_acquired_ref = ACQUIRED;
                self.task_in_select
                    .set_resolved_branch_id(self.associated_branch_id);

                Some(unsafe { ptr::read(&self.task_in_select.inner_mut().task) })
            }
        } else {
            let backoff = Backoff::new();

            loop {
                let prev_ = self.task_in_select.inner().state.compare_exchange(
                    NOT_ACQUIRED,
                    ACQUIRED,
                    AcqRel,
                    Acquire,
                );

                if let Err(prev) = prev_ {
                    // Can be `ACQUIRED` or `ACQUIRING_NOW`.
                    if prev == ACQUIRED {
                        return None;
                    }

                    // Another thread acquires the first of two tasks and tries to acquire the second one.
                    // It may fail (and set `NOT_ACQUIRED`) or succeed (and set `ACQUIRED`).
                    // We will for this update. It is not a performance issue because it
                    // happens very rarely, and we wait at the max time of `load` + `store`.
                    backoff.snooze();
                } else {
                    self.task_in_select
                        .set_resolved_branch_id(self.associated_branch_id);

                    return Some(unsafe { ptr::read(&self.task_in_select.inner().task) });
                }
            }
        }
    }

    /// Sets the provided `local` [`TaskInSelect`] as acquired only if it is acquired by this call,
    /// and if the provided `FnMut` returns `true`.
    /// It doesn't call the provided `FnMut` if the provided [`TaskInSelect`] is already acquired.
    ///
    /// # Safety
    ///
    /// * It is called in select;
    ///
    /// * Calls with `local` tasks.
    unsafe fn try_acquire_with_task_with_deadline_local_in_select(
        mut self,
        other_task: TaskWithDeadline,
        other_task_executing_fn: &mut impl FnMut(TaskWithDeadline) -> bool,
    ) -> PopIfAcquiredResult<TaskWithDeadline> {
        debug_assert!(self.task_in_select.inner().task.is_local());

        let this_state = self.task_in_select.inner_mut().state.get_mut();
        if *this_state == ACQUIRED {
            return PopIfAcquiredResult::NotAcquired(other_task);
        }

        let was_other_was_executed = other_task_executing_fn(other_task);
        if unlikely(!was_other_was_executed) {
            return PopIfAcquiredResult::NoData(self);
        }

        *this_state = ACQUIRED;

        local_executor().exec_task(unsafe { ptr::read(&self.task_in_select.inner().task) });

        PopIfAcquiredResult::Ok
    }

    /// Sets the provided `shared` [`TaskInSelect`] as acquired only if it is acquired by this call,
    /// and if the provided `FnMut` returns `true`.
    /// It doesn't call the provided `FnMut` if the provided [`TaskInSelect`] is already acquired.
    ///
    /// # Safety
    ///
    /// * It is called in select;
    ///
    /// * Calls with `shared` tasks.
    unsafe fn try_acquire_with_task_with_deadline_shared_in_select(
        self,
        other_task: TaskWithDeadline,
        other_task_executing_fn: &mut impl FnMut(TaskWithDeadline) -> bool,
    ) -> PopIfAcquiredResult<TaskWithDeadline> {
        debug_assert!(!self.task_in_select.inner().task.is_local());

        // Set state to acquiring now.
        let prev_ = self.task_in_select.inner().state.compare_exchange(
            NOT_ACQUIRED,
            ACQUIRING_NOW,
            AcqRel,
            Acquire,
        );

        if let Err(prev) = prev_ {
            // Can be only `ACQUIRED`.
            match prev {
                ACQUIRED => PopIfAcquiredResult::NotAcquired(other_task),

                // bug is occurred
                // because it can be acquiring now only if it is in select,
                // but we are in select and can't acquire,
                // so select was called twice with one TaskInSelect
                _ => unreachable_hint(),
            }
        } else {
            let was_other_task_executed = other_task_executing_fn(other_task);
            if unlikely(!was_other_task_executed) {
                self.task_in_select
                    .inner()
                    .state
                    .store(NOT_ACQUIRED, Release);

                return PopIfAcquiredResult::NoData(self);
            }

            self.task_in_select.inner().state.store(ACQUIRED, Release);

            self.task_in_select
                .set_resolved_branch_id(self.associated_branch_id);

            let this_task = unsafe { ptr::read(&self.task_in_select.inner().task) };

            local_executor().spawn_shared_task(this_task);

            PopIfAcquiredResult::Ok
        }
    }

    /// Sets the provided [`TaskInSelect`] as acquired only if it is acquired by this call,
    /// and if the provided `FnMut` returns `true`.
    /// It doesn't call the provided `FnMut` if the provided [`TaskInSelect`] is already acquired.
    ///
    /// # Safety
    ///
    /// * It is called in select.
    pub(crate) unsafe fn try_acquire_with_task_with_deadline_in_select(
        self,
        other_task: TaskWithDeadline,
        other_task_executing_fn: &mut impl FnMut(TaskWithDeadline) -> bool,
    ) -> PopIfAcquiredResult<TaskWithDeadline> {
        if self.is_local() {
            unsafe {
                self.try_acquire_with_task_with_deadline_local_in_select(
                    other_task,
                    other_task_executing_fn,
                )
            }
        } else {
            unsafe {
                self.try_acquire_with_task_with_deadline_shared_in_select(
                    other_task,
                    other_task_executing_fn,
                )
            }
        }
    }

    /// Attempts to acquire two `local` [`TaskInSelect`] and sets the resolved branch id on success.
    ///
    /// Read [`PopIfAcquiredResult`] for more detail.
    ///
    /// # Safety
    ///
    /// * It is called in select;
    ///
    /// * Calls with `local` tasks.
    unsafe fn try_acquire_two_local_tasks_in_select<T, SetterFn>(
        mut self,
        mut other: Self,
        setter_fn: &mut SetterFn,
        state: CallStatePtr,
        data: NonNull<T>,
    ) -> PopIfAcquiredResult<Self>
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        debug_assert!(self.task_in_select.inner().task.is_local());
        debug_assert!(other.task_in_select.inner().task.is_local());

        let other_state = other.task_in_select.inner_mut().state.get_mut();
        if *other_state == ACQUIRED {
            return PopIfAcquiredResult::NoData(self);
        }

        let this_state = self.task_in_select.inner_mut().state.get_mut();
        if *this_state == ACQUIRED {
            return PopIfAcquiredResult::AlreadyAcquired;
        }

        *other_state = ACQUIRED;
        *this_state = ACQUIRED;

        // Two tasks are acquired
        self.task_in_select
            .set_resolved_branch_id(self.associated_branch_id);
        other
            .task_in_select
            .set_resolved_branch_id(other.associated_branch_id);

        let this_task = unsafe { ptr::read(&self.task_in_select.inner().task) };
        let other_task = unsafe { ptr::read(&other.task_in_select.inner().task) };

        setter_fn(state, data); // set data to receiver/sender task

        let ex = local_executor();

        ex.exec_task(other_task);
        ex.exec_task(this_task);

        PopIfAcquiredResult::Ok
    }

    /// Attempts to acquire two `shared` [`TaskInSelect`] and sets the resolved branch id on success.
    ///
    /// Read [`PopIfAcquiredResult`] for more detail.
    ///
    /// # Safety
    ///
    /// * It is called in select.
    #[must_use]
    pub(crate) unsafe fn try_acquire_two_shared_tasks_in_select<T, SetterFn>(
        self,
        other: Self,
        setter_fn: &mut SetterFn,
        state: CallStatePtr,
        data: NonNull<T>,
    ) -> PopIfAcquiredResult<Self>
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        // TODO maybe better to acquire it in select (try_pop) because if there are two or more
        // WaitingTask::InSelector we can not release it on NoData

        debug_assert!(!self.task_in_select.inner().task.is_local());

        let mut is_first_try = true;
        let backoff = Backoff::new();

        'this_task: loop {
            if likely(is_first_try) {
                is_first_try = false;
            } else {
                self.task_in_select
                    .inner()
                    .state
                    .store(NOT_ACQUIRED, SeqCst);

                backoff.reset();
            }

            // Set state to acquiring now.
            let prev_ = self.task_in_select.inner().state.compare_exchange(
                NOT_ACQUIRED,
                ACQUIRING_NOW,
                AcqRel,
                Acquire,
            );

            if let Err(prev) = prev_ {
                // Can be only `ACQUIRED`.
                match prev {
                    ACQUIRED => return PopIfAcquiredResult::NotAcquired(other),

                    // bug is occurred
                    // because it can be acquiring now only if it is in select,
                    // but we are in select and can't acquire,
                    // so select was called twice with one TaskInSelect
                    _ => unreachable_hint(),
                }
            } else {
                loop {
                    let other_prev_ = other.task_in_select.inner().state.compare_exchange(
                        NOT_ACQUIRED,
                        ACQUIRED,
                        AcqRel,
                        Acquire,
                    );

                    if let Err(other_prev) = other_prev_ {
                        match other_prev {
                            // Can be `ACQUIRED` or acquiring now.
                            ACQUIRED => {
                                // We can't acquire the other task because it is already acquired.

                                self.task_in_select
                                    .inner()
                                    .state
                                    .store(NOT_ACQUIRED, Release);

                                return PopIfAcquiredResult::NoData(self);
                            }
                            ACQUIRING_NOW => {
                                if !backoff.is_completed() {
                                    backoff.snooze();
                                } else {
                                    // Probably a deadlock has occurred

                                    continue 'this_task;
                                }
                            }
                            _ => unreachable_hint(),
                        }
                    } else {
                        self.task_in_select.inner().state.store(ACQUIRED, Release);

                        self.task_in_select
                            .set_resolved_branch_id(self.associated_branch_id);
                        other
                            .task_in_select
                            .set_resolved_branch_id(other.associated_branch_id);

                        let this_task = unsafe { ptr::read(&self.task_in_select.inner().task) };
                        let other_task = unsafe { ptr::read(&other.task_in_select.inner().task) };

                        setter_fn(state, data); // set data to receiver/sender task

                        let ex = local_executor();

                        ex.spawn_shared_task(other_task);
                        ex.spawn_shared_task(this_task);

                        return PopIfAcquiredResult::Ok;
                    }
                }
            }
        }
    }

    /// Attempts to acquire two [`TaskInSelect`] and sets the resolved branch id on success.
    ///
    /// It decides to call [`try_acquire_two_shared_tasks_in_select`]
    /// or [`try_acquire_two_local_tasks_in_select`].
    ///
    /// Read [`PopIfAcquiredResult`] for more detail.
    ///
    /// # Safety
    ///
    /// * It is called in select.
    ///
    /// [`try_acquire_two_shared_tasks_in_select`]: Self::try_acquire_two_shared_tasks_in_select
    /// [`try_acquire_two_local_tasks_in_select`]: Self::try_acquire_two_local_tasks_in_select
    #[must_use]
    pub(crate) unsafe fn try_acquire_two_tasks_in_select<T, SetterFn>(
        self,
        other: Self,
        setter_fn: &mut SetterFn,
        state: CallStatePtr,
        data: NonNull<T>,
    ) -> PopIfAcquiredResult<Self>
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        if self.is_local() {
            unsafe { self.try_acquire_two_local_tasks_in_select(other, setter_fn, state, data) }
        } else {
            unsafe { self.try_acquire_two_shared_tasks_in_select(other, setter_fn, state, data) }
        }
    }
}

/// A pool for managing [`TaskInSelect`] instances in a thread-local context.
///
/// This struct provides methods to acquire and release tasks efficiently, minimizing
/// memory allocation overhead by reusing previously allocated instances.
struct TaskInSelectPool {
    vec: Vec<NonNull<Inner>>,
}

impl TaskInSelectPool {
    /// Creates a new instance of [`TaskInSelectPool`].
    const fn new() -> Self {
        Self { vec: Vec::new() }
    }

    /// Acquires from the pool a [`Inner`] and sets task and resolved branch ID.
    ///
    /// This method attempts to reuse an existing [`Inner`] instance from the pool.
    /// If no instances are available, it allocates a new one.
    fn acquire_for_task(
        &mut self,
        task: Task,
        resolved_branch_id: NonNull<usize>,
    ) -> NonNull<Inner> {
        if let Some(mut inner) = self.vec.pop() {
            let inner_ref = unsafe { inner.as_mut() };

            inner_ref.task = task;
            inner_ref.resolved_branch_id = resolved_branch_id;
            inner_ref.state = AtomicUsize::new(NOT_ACQUIRED);
            inner_ref.ref_count = AtomicUsize::new(1);

            inner
        } else {
            NonNull::from(Box::leak(Box::new(Inner {
                task,
                resolved_branch_id,
                state: AtomicUsize::new(NOT_ACQUIRED),
                ref_count: AtomicUsize::new(1),
            })))
        }
    }

    /// Releases a previously acquired [`Inner`] instance back to the pool.
    ///
    /// If the pool exceeds a certain size, the instance is dropped instead of being retained.
    fn release(&mut self, inner: NonNull<Inner>) {
        if self.vec.len() * size_of::<Inner>() <= 64 * 1024 * 1024 {
            self.vec.push(inner);

            return;
        }

        unsafe { drop(Box::from_raw(inner.as_ptr())) };
    }
}

unsafe impl Send for TaskInSelectPool {}

impl Drop for TaskInSelectPool {
    fn drop(&mut self) {
        clear_with(&mut self.vec, |inner| {
            unsafe { drop(Box::from_raw(inner.as_ptr())) };
        });
    }
}

thread_local! {
    /// Thread-local [`TaskInSelectPool`] therefore, it is lockless.
    // Before refactor: it must be thread-local, or rewrite drop logic in `TaskInSelect`.
    static TASK_IN_SELECT_POOL: UnsafeCell<TaskInSelectPool> = const {
        UnsafeCell::new(TaskInSelectPool::new())
    };
}

/// Returns a mutable reference to the thread-local `TaskInSelectPool`.
fn task_in_select_pool() -> &'static mut TaskInSelectPool {
    unsafe { TASK_IN_SELECT_POOL.with(|pool| &mut *pool.get()) }
}

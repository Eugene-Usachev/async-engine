// TODO docs and think about pub

use crate::local_executor;
use crate::runtime::Task;
use crate::sync::channels::state::CallStatePtr;
use crate::utils::hints::unreachable_hint;
use crate::utils::Backoff;
use fastrand::Rng;
use std::cell::UnsafeCell;
use std::hint::spin_loop;
use std::ops::{Deref, DerefMut};
use std::ptr::NonNull;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed, Release, SeqCst};
use std::sync::atomic::{fence, AtomicBool, AtomicUsize};
use std::time::Duration;
use std::{mem, ptr, thread};

const NOT_ACQUIRED: usize = 0;
const ACQUIRED: usize = 1;
const ACQUIRING_NOW: usize = 2;

#[repr(C)]
pub struct Inner {
    task: Task,
    resolved_branch_id: NonNull<usize>,
    state: AtomicUsize,
    ref_count: AtomicUsize,
}

pub struct TaskInSelect {
    inner: NonNull<Inner>,
}

unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}

impl TaskInSelect {
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

    fn set_resolved_branch_id(&self, branch_id: usize) {
        debug_assert_eq!(
            unsafe { self.resolved_branch_id.read() },
            usize::MAX,
            "Tried to set resolved branch id twice"
        );

        unsafe { self.resolved_branch_id.write(branch_id) };
    }

    fn release(&self) {
        debug_assert_eq!(self.ref_count.load(Acquire), 0);
        debug_assert_eq!(
            self.state.load(Acquire),
            ACQUIRED,
            "Attempt to drop TaskSelect (ref count is 0) that was not acquired"
        );

        task_in_select_pool().release(self.inner);
    }
}

impl Deref for TaskInSelect {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        unsafe { self.inner.as_ref() }
    }
}

impl DerefMut for TaskInSelect {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.inner.as_mut() }
    }
}

impl Clone for TaskInSelect {
    fn clone(&self) -> Self {
        if self.task.is_local() {
            *unsafe { &mut *self.inner.as_ptr() }.ref_count.get_mut() += 1;
        } else {
            self.ref_count.fetch_add(1, Relaxed);
        }

        Self { inner: self.inner }
    }
}

unsafe impl Send for TaskInSelect {}
unsafe impl Sync for TaskInSelect {}

impl Drop for TaskInSelect {
    fn drop(&mut self) {
        if self.task.is_local() {
            let prev = *self.ref_count.get_mut();

            debug_assert!(self.task.is_local());

            *self.ref_count.get_mut() -= 1;
            if prev != 1 {
                return;
            }

            self.release();

            return;
        }

        if self.ref_count.fetch_sub(1, Release) != 1 {
            return;
        }

        fence(Acquire);

        self.release();
    }
}

pub(crate) enum PopIfAcquiredResult {
    Ok,
    NoData(TaskInSelectBranch),
    NotAcquired(TaskInSelectBranch),
    AlreadyAcquired,
}

#[repr(C)]
#[derive(Clone)]
pub struct TaskInSelectBranch {
    task_in_select: TaskInSelect,
    associated_branch_id: usize,
}

impl TaskInSelectBranch {
    pub fn new(task_in_select: TaskInSelect, associated_branch_id: usize) -> Self {
        Self {
            task_in_select,
            associated_branch_id,
        }
    }

    pub(crate) fn acquire_once(mut self) -> Option<Task> {
        if self.task_in_select.task.is_local() {
            let was_acquired_ref = self.task_in_select.state.get_mut();

            debug_assert!(*was_acquired_ref < 2);

            if *was_acquired_ref == ACQUIRED {
                None
            } else {
                *was_acquired_ref = ACQUIRED;
                self.task_in_select
                    .set_resolved_branch_id(self.associated_branch_id);

                Some(unsafe { ptr::read(&self.task_in_select.task) })
            }
        } else {
            let backoff = Backoff::new();

            loop {
                let prev_ = self.task_in_select.state.compare_exchange(
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

                    // Another thread acquires first of two task and trying to acquire second one.
                    // It may fail (and set `NOT_ACQUIRED`) or succeed (and set `ACQUIRED`).
                    // We will for this update. It is not a performance issue, because it
                    // happens very rarely, and we wait at max time of `load` + `store`.
                    backoff.spin();
                } else {
                    self.task_in_select
                        .set_resolved_branch_id(self.associated_branch_id);

                    return Some(unsafe { ptr::read(&self.task_in_select.task) });
                }
            }
        }
    }

    unsafe fn try_acquire_two_local_tasks_in_select<T, SetterFn>(
        mut self,
        mut other: Self,
        setter_fn: &mut SetterFn,
        state: CallStatePtr,
        data: NonNull<T>,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        debug_assert!(self.task_in_select.task.is_local());
        debug_assert!(other.task_in_select.task.is_local());

        if *self.task_in_select.state.get_mut() == ACQUIRED {
            return PopIfAcquiredResult::NotAcquired(other);
        }

        *self.task_in_select.state.get_mut() = ACQUIRED;

        if *other.task_in_select.state.get_mut() == ACQUIRED {
            *self.task_in_select.state.get_mut() = NOT_ACQUIRED;

            return PopIfAcquiredResult::NoData(self);
        }

        // Two tasks are acquired
        self.task_in_select
            .set_resolved_branch_id(self.associated_branch_id);
        other
            .task_in_select
            .set_resolved_branch_id(other.associated_branch_id);

        let this_task = unsafe { ptr::read(&self.task_in_select.task) };
        let other_task = unsafe { ptr::read(&other.task_in_select.task) };

        setter_fn(state, data); // set data to receiver/sender task

        let ex = local_executor();

        ex.exec_task(other_task);
        ex.exec_task(this_task);

        PopIfAcquiredResult::Ok
    }

    /// Tries to acquire two tasks in that are used in `shared` context.
    ///
    /// Returns [`PopIfAcquiredResult::NotAcquired`] if `self` task have been already acquired.
    ///
    /// Returns [`PopIfAcquiredResult::NoData`] if `other` task have been already acquired.
    ///
    /// It is used only when both tasks are in `shared` context.
    ///
    /// # Safety
    ///
    /// * It is called in select;
    ///
    /// * If returns `false` then `other` task must be not lost (saved into queue again).
    #[must_use]
    pub(crate) unsafe fn try_acquire_two_shared_tasks_in_select<T, SetterFn>(
        self,
        other: Self,
        setter_fn: &mut SetterFn,
        state: CallStatePtr,
        data: NonNull<T>,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        // TODO maybe better to acquire it in select (try_pop) because if there are two or more
        // WaitingTask::InSelector we can not release it on NoData

        macro_rules! exec_two_tasks {
            (
                $this_inner:expr,
                $other_inner:expr,
                $this:expr,
                $other:expr,
                $setter_fn:expr,
                $state:expr,
                $data:expr
            ) => {
                $this_inner.set_resolved_branch_id($this.associated_branch_id);
                $other_inner.set_resolved_branch_id($other.associated_branch_id);

                let this_task = unsafe { ptr::read(&$this_inner.task) };
                let other_task = unsafe { ptr::read(&$other_inner.task) };

                $setter_fn($state, $data); // set data to receiver/sender task

                let ex = $crate::local_executor();

                ex.spawn_shared_task(other_task);
                ex.spawn_shared_task(this_task);
            };
        }

        debug_assert!(!self.task_in_select.task.is_local());

        let mut is_first_try = true;
        let backoff = Backoff::new();

        'this_task: loop {
            if is_first_try {
                is_first_try = false;
            } else {
                self.task_in_select.state.store(NOT_ACQUIRED, SeqCst);

                backoff.reset();
            }

            // Set state to acquiring now with another task. Read below for details.
            let prev_ = self.task_in_select.state.compare_exchange(
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
                    let other_prev_ = other.task_in_select.state.compare_exchange(
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

                                self.task_in_select.state.store(NOT_ACQUIRED, Release);

                                return PopIfAcquiredResult::NoData(self);
                            }
                            ACQUIRING_NOW => {
                                if !backoff.is_completed() {
                                    backoff.spin();
                                } else {
                                    // Probably a deadlock has occurred

                                    continue 'this_task;
                                }
                            }
                            _ => unreachable_hint()
                        }
                    } else {
                        self.task_in_select.state.store(ACQUIRED, Release);

                        // TODO rewrite without the macro
                        exec_two_tasks!(
                            self.task_in_select,
                            other.task_in_select,
                            self,
                            other,
                            setter_fn,
                            state,
                            data
                        );

                        return PopIfAcquiredResult::Ok;
                    }
                }
            }
        }
    }

    #[must_use]
    pub(crate) unsafe fn try_acquire_two_tasks_in_select<T, SetterFn>(
        self,
        other: Self,
        setter_fn: &mut SetterFn,
        state: CallStatePtr,
        data: NonNull<T>,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        if self.task_in_select.task.is_local() {
            unsafe { self.try_acquire_two_local_tasks_in_select(other, setter_fn, state, data) }
        } else {
            unsafe { self.try_acquire_two_shared_tasks_in_select(other, setter_fn, state, data) }
        }
    }
}

struct TaskInSelectPool {
    vec: Vec<NonNull<Inner>>,
}

impl TaskInSelectPool {
    const fn new() -> Self {
        Self { vec: Vec::new() }
    }

    fn acquire_for_task(&mut self, task: Task, resolved_branch_id: NonNull<usize>) -> NonNull<Inner> {
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

    fn release(&mut self, inner: NonNull<Inner>) {
        if self.vec.len() * size_of::<Inner>() <= 64 * 1024 * 1024 {
            self.vec.push(inner);

            return;
        }

        unsafe { drop(Box::from_raw(inner.as_ptr())) };
    }
}

unsafe impl Send for TaskInSelectPool {}

// TODO think about it
impl Drop for TaskInSelectPool {
    fn drop(&mut self) {
        for inner in self.vec.drain(..) {
            unsafe { drop(Box::from_raw(inner.as_ptr())) };
        }
    }
}

thread_local! {
    /// Thread-local [`TaskInSelectPool`] therefore, it is lockless.
    // Before refactor: it must be thread-local, or rewrite drop logic in `TaskInSelect`.
    static TASK_IN_SELECT_POOL: UnsafeCell<TaskInSelectPool> = const { UnsafeCell::new(TaskInSelectPool::new()) };
}

fn task_in_select_pool() -> &'static mut TaskInSelectPool {
    unsafe { TASK_IN_SELECT_POOL.with(|pool| &mut *pool.get()) }
}

// TODO r
// static TASK_IN_SELECT_POOL: Mutex<TaskInSelectPool> = Mutex::new(TaskInSelectPool::new());

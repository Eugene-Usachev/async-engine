// TODO docs and think about pub

use crate::local_executor;
use crate::runtime::Task;
use crate::sync::channels::state::CallStatePtr;
use crate::utils::hints::unreachable_hint;
use std::cell::UnsafeCell;
use std::hint::spin_loop;
use std::ptr;
use std::ptr::NonNull;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed, Release};
use std::sync::atomic::{fence, AtomicUsize};

const NOT_ACQUIRED: usize = 0;
const ACQUIRED: usize = 1;

pub struct TaskInSelect {
    task: Task,
    resolved_branch_id: NonNull<usize>,
    /// Can be:
    ///
    /// * [`Default`] - neither acquired nor trying to acquire.
    ///
    /// * [`ACQUIRED`] - acquired.
    ///
    /// * `Was acquiring now` - trying to acquire. It contains another `NonNull<TaskInSelect>`.
    ///   It is needed to prevent deadlocks. Example: two threads are trying to select. First
    ///   acquiring the first task, and the second acquiring the second task. Next, the first
    ///   thread tries to acquire the second task, and the second thread tries to acquire
    ///   the first task. We need to prevent this. In current implementation, threads will
    ///   see that they are trying to acquire the same tasks and the thread with more bigger
    ///   (as usize) task will acquire both tasks.
    state: AtomicUsize,
    ref_count: AtomicUsize,
}

impl TaskInSelect {
    pub fn acquire_for_task_with_lock(
        task: Task,
        resolved_branch_id: NonNull<usize>,
    ) -> NonNull<Self> {
        #[cfg(not(debug_assertions))]
        {
            return task_in_select_pool().acquire_for_task(task, resolved_branch_id);
        }

        #[cfg(debug_assertions)]
        {
            let mut task_in_select =
                task_in_select_pool().acquire_for_task(task, resolved_branch_id);

            unsafe {
                task_in_select.as_mut().resolved_branch_id.write(usize::MAX);
            };

            task_in_select
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

        task_in_select_pool().release(NonNull::from(self));
    }

    pub fn drop_ptr(&self) {
        if self.ref_count.fetch_sub(1, Release) != 1 {
            return;
        }

        fence(Acquire);

        self.release();
    }

    pub unsafe fn drop_ptr_local(&mut self) {
        let prev = *self.ref_count.get_mut();

        debug_assert!(self.task.is_local());

        *self.ref_count.get_mut() -= 1;
        if prev != 1 {
            return;
        }

        self.release();
    }
}

unsafe impl Send for TaskInSelect {}
unsafe impl Sync for TaskInSelect {}

pub(crate) enum PopIfAcquiredResult {
    Ok,
    NoData,
    NotAcquired,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct TaskInSelectBranch {
    inner_ptr: NonNull<TaskInSelect>,
    associated_branch_id: usize,
}

impl TaskInSelectBranch {
    // `Local` tasks can use non-atomic operations. So, methods are separated to methods without
    // `local` suffix (shared) and methods with `local` suffix (local).

    pub unsafe fn from_owned_task_in_select_ptr(
        task_in_select: NonNull<TaskInSelect>,
        associated_branch_id: usize,
    ) -> Self {
        Self {
            inner_ptr: task_in_select,
            associated_branch_id,
        }
    }

    pub fn new(task_in_select: NonNull<TaskInSelect>, associated_branch_id: usize) -> Self {
        unsafe { task_in_select.as_ref() }
            .ref_count
            .fetch_add(1, Relaxed);

        Self {
            inner_ptr: task_in_select,
            associated_branch_id,
        }
    }

    pub unsafe fn new_local(
        mut task_in_select: NonNull<TaskInSelect>,
        associated_branch_id: usize,
    ) -> Self {
        *unsafe { task_in_select.as_mut() }.ref_count.get_mut() += 1;

        Self {
            inner_ptr: task_in_select,
            associated_branch_id,
        }
    }

    #[inline(always)]
    fn with_drop_ptr<const IS_LOCAL: bool, T>(&self, f: impl FnOnce(&mut TaskInSelect) -> T) -> T {
        let inner = unsafe { &mut *self.inner_ptr.as_ptr() };

        let result = f(inner);

        if IS_LOCAL {
            debug_assert!(unsafe { self.inner_ptr.as_ref().task.is_local() });

            unsafe { inner.drop_ptr_local() };
        } else {
            inner.drop_ptr();
        }

        result
    }

    pub(crate) fn acquire_once(&self) -> Option<Task> {
        self.with_drop_ptr::<false, _>(|inner| {
            loop {
                let prev_ = inner
                    .state
                    .compare_exchange(NOT_ACQUIRED, ACQUIRED, AcqRel, Acquire);

                if let Err(prev) = prev_ {
                    // Can be `ACQUIRED` or `ACQUIRING_NOW`.
                    match prev {
                        ACQUIRED => return None,
                        _ => {
                            // Another thread acquire first of two task and trying to acquire second one.
                            // It may fail (and set `NOT_ACQUIRED`) or succeed (and set `ACQUIRED`).
                            // We will for this update. It is not a performance issue, because it
                            // happens very rarely, and we wait at max time of `load` + `store`.
                            spin_loop();
                        }
                    }
                } else {
                    inner.set_resolved_branch_id(self.associated_branch_id);

                    return Some(unsafe { ptr::read(&inner.task) });
                }
            }
        })
    }

    pub(crate) unsafe fn acquire_once_local(&mut self) -> Option<Task> {
        self.with_drop_ptr::<true, _>(|inner| {
            let was_acquired_ref = inner.state.get_mut();

            debug_assert!(*was_acquired_ref < 2);

            if *was_acquired_ref == ACQUIRED {
                None
            } else {
                *was_acquired_ref = ACQUIRED;
                unsafe {
                    self.inner_ptr
                        .as_ref()
                        .set_resolved_branch_id(self.associated_branch_id);
                };

                Some(unsafe { ptr::read(&self.inner_ptr.as_ref().task) })
            }
        })
    }

    pub(crate) unsafe fn try_acquire_two_local_tasks_in_select<T, SetterFn>(
        &mut self,
        other: &mut Self,
        setter_fn: &mut SetterFn,
        state: CallStatePtr,
        data: NonNull<T>,
    ) -> PopIfAcquiredResult
    where
        SetterFn: FnMut(CallStatePtr, NonNull<T>),
    {
        let this_inner = unsafe { self.inner_ptr.as_mut() };
        let other_inner = unsafe { other.inner_ptr.as_mut() };

        if *this_inner.state.get_mut() == ACQUIRED {
            return PopIfAcquiredResult::NotAcquired;
        }

        *this_inner.state.get_mut() = ACQUIRED;

        if *other_inner.state.get_mut() == ACQUIRED {
            *this_inner.state.get_mut() = NOT_ACQUIRED;

            unsafe { other_inner.drop_ptr_local() };

            return PopIfAcquiredResult::NoData;
        }

        // Two tasks are acquired
        this_inner.set_resolved_branch_id(self.associated_branch_id);
        other_inner.set_resolved_branch_id(other.associated_branch_id);

        let this_task = unsafe { ptr::read(&this_inner.task) };
        let other_task = unsafe { ptr::read(&other_inner.task) };

        unsafe { this_inner.drop_ptr_local() };
        unsafe { other_inner.drop_ptr_local() };

        setter_fn(state, data); // set data to receiver/sender task

        let ex = local_executor();

        // TODO first we should add DELTA, because of exec

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
        &self,
        other: &Self,
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

                $this_inner.drop_ptr();
                $other_inner.drop_ptr();

                $setter_fn($state, $data); // set data to receiver/sender task

                let ex = $crate::local_executor();

                ex.spawn_shared_task(other_task);
                ex.spawn_shared_task(this_task);
            };
        }

        let this_inner = unsafe { self.inner_ptr.as_ref() };
        let other_inner = unsafe { other.inner_ptr.as_ref() };
        let other_ptr_as_usize = other.inner_ptr.as_ptr() as usize;

        debug_assert!(
            other_ptr_as_usize != NOT_ACQUIRED && other_ptr_as_usize != ACQUIRED,
            "NonNull<TaskInSelect> contains ptr that equals to 0 or 1. It means that is is invalid."
        );

        // set state to acquiring now with other task. Read below for details.
        let prev_ =
            this_inner
                .state
                .compare_exchange(NOT_ACQUIRED, other_ptr_as_usize, AcqRel, Acquire);

        if let Err(prev) = prev_ {
            // Can be only `ACQUIRED`.
            match prev {
                ACQUIRED => {
                    this_inner.drop_ptr();

                    PopIfAcquiredResult::NotAcquired
                }
                // bug is occurred,
                // because it can be acquiring now only if it is in select,
                // but we are in select and can't acquire,
                // so select was called twice with one TaskInSelect
                _ => unreachable_hint(),
            }
        } else {
            loop {
                let other_prev_ =
                    other_inner
                        .state
                        .compare_exchange(NOT_ACQUIRED, ACQUIRED, AcqRel, Acquire);

                if let Err(other_prev) = other_prev_ {
                    match other_prev {
                        // Can be `ACQUIRED` or acquiring now.
                        ACQUIRED => {
                            // We can't acquire other task, because it is already acquired.

                            this_inner.state.store(NOT_ACQUIRED, Release);

                            other_inner.drop_ptr();

                            break PopIfAcquiredResult::NoData;
                        }
                        acquiring_now_with => {
                            let this_ptr_as_usize = self.inner_ptr.as_ptr() as usize;
                            debug_assert!(
                                other_ptr_as_usize != NOT_ACQUIRED
                                    && other_ptr_as_usize != ACQUIRED,
                                "NonNull<TaskInSelect> contains ptr that equals to 0 or 1. \
                                It means that is is invalid."
                            );

                            if acquiring_now_with != this_ptr_as_usize {
                                spin_loop();
                                continue; // Now other task is trying to acquire another task.
                                // We wait until another thread acquire it
                                // or stop acquiring with fail.
                                // It is not a performance issue, because it
                                // happens very rarely, and we wait at max time
                                // of `compare_exchange` + `store`.
                            }

                            // Now two threads are trying to acquire both tasks.
                            // First thread acquired first task and trying to acquire second task.
                            // Second thread acquired second task and trying to acquire first task.
                            // So, it is a deadlock if not to solve it.
                            // But this and other threads know that it is happening now.
                            // So, they can solve it. We can represent TaskInSelect as usize.
                            // And the thread that acquires the "greatest" task will capture
                            // both tasks.
                            // The other knows this and continues to execute.

                            if this_ptr_as_usize > other_ptr_as_usize {
                                // Current thread acquires both tasks.

                                this_inner.state.store(ACQUIRED, Release);
                                other_inner.state.store(ACQUIRED, Release);

                                exec_two_tasks!(
                                    this_inner,
                                    other_inner,
                                    self,
                                    other,
                                    setter_fn,
                                    state,
                                    data
                                );

                                break PopIfAcquiredResult::Ok;
                            }

                            // Other thread will acquire both tasks
                            // and will drop pointers
                            // and will execute both tasks.

                            break PopIfAcquiredResult::Ok;
                        }
                    }
                }

                unsafe { self.inner_ptr.as_ref() }
                    .state
                    .store(ACQUIRED, Release);
                // other task is already acquired above

                exec_two_tasks!(this_inner, other_inner, self, other, setter_fn, state, data);

                break PopIfAcquiredResult::Ok;
            }
        }
    }

    pub(crate) fn is_local(&self) -> bool {
        unsafe { self.inner_ptr.as_ref() }.task.is_local()
    }
}

struct TaskInSelectPool {
    vec: Vec<NonNull<TaskInSelect>>,
}

impl TaskInSelectPool {
    const fn new() -> Self {
        Self { vec: Vec::new() }
    }

    fn acquire_for_task(
        &mut self,
        task: Task,
        resolved_branch_id: NonNull<usize>,
    ) -> NonNull<TaskInSelect> {
        if let Some(mut inner) = self.vec.pop() {
            let inner_ref = unsafe { inner.as_mut() };

            inner_ref.task = task;
            inner_ref.resolved_branch_id = resolved_branch_id;
            inner_ref.state = AtomicUsize::new(NOT_ACQUIRED);
            inner_ref.ref_count = AtomicUsize::new(1);

            inner
        } else {
            NonNull::from(Box::leak(Box::new(TaskInSelect {
                task,
                resolved_branch_id,
                state: AtomicUsize::new(NOT_ACQUIRED),
                ref_count: AtomicUsize::new(1),
            })))
        }
    }

    fn release(&mut self, inner: NonNull<TaskInSelect>) {
        self.vec.push(inner);
    }
}

thread_local! {
    /// Thread-local [`TaskInSelectPool`], therefore it is lockless.
    // Before refactor: it must be thread-local, or rewrite drop logic in `TaskInSelect`.
    static TASK_IN_SELECT_POOL: UnsafeCell<TaskInSelectPool> = const { UnsafeCell::new(TaskInSelectPool::new()) };
}

fn task_in_select_pool() -> &'static mut TaskInSelectPool {
    unsafe { TASK_IN_SELECT_POOL.with(|pool| &mut *pool.get()) }
}

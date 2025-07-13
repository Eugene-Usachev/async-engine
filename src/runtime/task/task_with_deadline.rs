//! This module contains the [`TaskWithDeadline`].
use crate::local_executor;
use crate::runtime::Task;
use crate::sync::channels::{CallState, CallStatePtr};
use crate::utils::{OrengineInstant, likely};
use std::cell::UnsafeCell;
use std::ptr;
use std::ptr::NonNull;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed};

thread_local! {
    /// The pool of `NonNull<AtomicBool>`s.
    static ATOMIC_POOL: UnsafeCell<Vec<NonNull<AtomicBool>>> = const { UnsafeCell::new(Vec::new()) };
}

/// Returns a mutable reference to the pool of `NonNull<AtomicBool>`s.
fn get_atomic_pool() -> &'static mut Vec<NonNull<AtomicBool>> {
    ATOMIC_POOL.with(|pool| unsafe { &mut *pool.get() })
}

/// `TaskWithDeadline` is a wrapper around [`Task`] which adds a deadline to it. It is used only
/// for channels.
///
/// [`Executor`](crate::Executor) will try to wake it after the deadline is reached.
///
/// This violates the concept of task ownership because it also is stored by [`Executor`].
/// But you don't have to worry, because `Orengine` takes care of it when you don't copy it.
#[repr(C)]
pub(crate) struct TaskWithDeadline {
    task: Task,
    // We don't need ref-counting here,
    // because it `try_wake` should be called exactly twice,
    // and we know when it is called in the second time.
    was_woken: NonNull<AtomicBool>,
    // It will write to the provided `result_ptr` only if it is woken by deadline.
    result_ptr: CallStatePtr,
}

impl TaskWithDeadline {
    /// Creates new [`TaskWithDeadline`].
    pub(crate) fn new(task: Task, result_ptr: CallStatePtr) -> Self {
        Self {
            task,
            was_woken: {
                let mut atomic_ptr = get_atomic_pool()
                    .pop()
                    .unwrap_or_else(|| NonNull::from(Box::leak(Box::new(AtomicBool::new(false)))));

                unsafe { *atomic_ptr.as_mut().get_mut() = false };

                atomic_ptr
            },
            result_ptr,
        }
    }

    /// Creates a new [`TaskWithDeadline`] and registers it. It will write to the provided
    /// `result_ptr` only if it is woken by deadline.
    pub fn create_new_and_register(
        task: Task,
        result_ptr: CallStatePtr,
        deadline: OrengineInstant,
    ) -> Self {
        local_executor().register_task_with_deadline(task, result_ptr, deadline)
    }

    /// Returns if the [`TaskWithDeadline`] is local.
    #[inline]
    pub fn is_local(&self) -> bool {
        self.task.is_local()
    }

    /// Releases all resources.
    pub fn release(self) {
        let pool = get_atomic_pool();
        if likely(pool.len() < 1_000_000) {
            // ~8 MB
            pool.push(self.was_woken);

            return;
        }

        unsafe { drop(Box::from_raw(self.was_woken.as_ptr())) };
    }

    /// Returns if the [`TaskWithDeadline`] was woken.
    pub(crate) fn was_woken(&self) -> bool {
        unsafe {
            if self.is_local() {
                *self.was_woken.as_ref().as_ptr()
            } else {
                self.was_woken.as_ref().load(Acquire)
            }
        }
    }

    /// Tries to wake the [`TaskWithDeadline`] with the provided reason.
    ///
    /// The provided `FnOnce` is called before the task is woken.
    ///
    /// Returns `true` and calls the provided `FnOnce` if it was woken by this call.
    fn try_wake_with_reason<const WAS_WOKEN_BY_DEADLINE: bool>(mut self, f: impl FnOnce()) -> bool {
        unsafe {
            if self.is_local() {
                if !*self.was_woken.as_mut().get_mut() {
                    *self.was_woken.as_mut() = AtomicBool::new(true);
                    if WAS_WOKEN_BY_DEADLINE {
                        self.result_ptr.write(CallState::WokenByDeadline);
                    }

                    f();

                    local_executor().exec_task(ptr::read(&self.task));

                    true
                } else {
                    // We are the second, must release

                    self.release();

                    false
                }
            } else {
                let swapped = self
                    .was_woken
                    .as_ref()
                    .compare_exchange(false, true, AcqRel, Relaxed)
                    .is_ok();

                if swapped {
                    if WAS_WOKEN_BY_DEADLINE {
                        self.result_ptr.write(CallState::WokenByDeadline);
                    }

                    f();

                    local_executor().spawn_shared_task(ptr::read(&self.task));

                    true
                } else {
                    // We are the second, must release

                    self.release();

                    false
                }
            }
        }
    }

    /// Tries to wake the [`TaskWithDeadline`].
    ///
    /// Returns `true` if it was woken by this call.
    pub(crate) fn try_wake_by_deadline(self) -> bool {
        self.try_wake_with_reason::<true>(|| {})
    }

    /// Tries to wake the [`TaskWithDeadline`].
    ///
    /// The provided `FnOnce` is called before the task is woken.
    ///
    /// Returns `true` and calls the provided `FnOnce` if it was woken by this call.
    pub fn try_wake_with(self, f: impl FnOnce()) -> bool {
        self.try_wake_with_reason::<false>(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as orengine;
    use crate::sync::{
        AsyncCondVar, AsyncMutex, AsyncWaitGroup, LocalCondVar, LocalMutex, LocalWaitGroup,
    };
    use crate::{Local, sleep_until};
    use std::time::Duration;

    #[orengine::test_local]
    fn test_task_with_deadline() {
        macro_rules! generate_test_task_with_deadline {
            ($option:pat, $assert_failure_message:expr, $deadline_ident:ident, $wake_block:block, $task_name:ident) => {
                let result = Local::new(0);
                let wg = std::rc::Rc::new(LocalWaitGroup::new());
                let wg_clone = wg.clone();
                let mut woken_result = CallState::FirstCall;
                let woken_result_ptr = CallStatePtr::new(&mut woken_result);
                let task = std::rc::Rc::new(LocalCondVar::new(LocalMutex::new(None)));
                let task_clone = task.clone();
                let start = OrengineInstant::now();
                let $deadline_ident = start + Duration::from_millis(1000);

                wg.inc().await;

                local_executor().exec_local_future(async move {
                    let mut guard = task_clone.lock().await;

                    *guard = Some(unsafe { Task::get_current().await });

                    drop(guard);

                    local_executor().spawn_local(async move {
                        task_clone.notify_one(task_clone.lock().await);
                    });

                    unsafe { Task::park_current_task().await };

                    assert!(
                        matches!(*woken_result_ptr, $option),
                        $assert_failure_message
                    );

                    *result.borrow_mut() += 1;

                    wg_clone.done().await;
                });

                let mut task_guard = task.wait_while(task.lock().await, |v| v.is_none()).await;

                let $task_name = TaskWithDeadline::create_new_and_register(
                    task_guard.take().unwrap(),
                    woken_result_ptr,
                    start + Duration::from_micros(10),
                );

                $wake_block;

                wg.wait().await;
            };
        }

        generate_test_task_with_deadline!(
            CallState::WokenByDeadline,
            "TaskWithDeadline should be woken by deadline",
            deadline,
            {
                local_executor().spawn_local(async move {
                    sleep_until(deadline).await;

                    assert!(
                        !task.try_wake_with(|| {}),
                        "TaskWithDeadline should not be woken by call!"
                    );
                });
            },
            task
        );

        generate_test_task_with_deadline!(
            CallState::FirstCall,
            "TaskWithDeadline should be woken by call",
            _timeout,
            {
                assert!(
                    task.try_wake_with(|| {}),
                    "TaskWithDeadline should be woken by call!"
                );
            },
            task
        );
    }
}

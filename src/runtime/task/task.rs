use crate::runtime::call::Call;
use crate::runtime::task::task_data::TaskData;
use crate::runtime::{Locality, TaskPool};
use crate::{local_executor, Executor};
use std::future::Future;
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::pin::Pin;
use std::task::{Context, Poll};

/// `Task` is a pointer like wrapper of a future.
///
/// If `debug_assertions` is enabled, it keeps additional information to check
/// if the task is safe to be executed.
///
/// # Be careful
///
/// `Task` __must__ be executed via [`Executor::exec_task`](Executor::exec_task).
///
/// # The concept of task ownership
///
/// Orengine works correctly only if you follow the concept of task ownership.
/// It means that a `Task` can be only moved. This follows:
///
/// - only one thread can own a `Task` at the same time,
///   therefore `Task` can be executed without synchronization;
///
/// - only one `Task` instance can exist at the same time,
///   therefore it can be dropped after it returns [`Poll::Ready`], so it doesn't use ref counters.
///
/// # Locality
///
/// Each `Task` has a locality. Read [`Executor`] for more details. You can get the locality
/// with [`Task::is_local`].
///
/// The information about locality contains in [`Task`] but not under the pointer. So, you can read
/// without dereferencing the pointer.
///
/// `Task` locality can be changed only via unsafe
/// [`update_current_task_locality`](update_current_task_locality).
pub struct Task {
    pub(crate) data: TaskData,
    #[cfg(debug_assertions)]
    pub(crate) executor_id: usize,
    #[cfg(debug_assertions)]
    pub(crate) is_executing: crate::utils::Ptr<std::sync::atomic::AtomicBool>,
}

impl Task {
    /// Returns a [`Task`] with the given future.
    ///
    /// # Safety
    ///
    /// - With [`local locality`](Locality::local) it is always safe;
    ///
    /// - With [`shared locality`](Locality::shared) it is safe if the provided [`Future`] is `Send`.
    #[inline]
    pub unsafe fn from_future<F: Future<Output = ()>>(future: F, locality: Locality) -> Self {
        TaskPool::acquire(future, locality)
    }

    /// Returns a [`Task`] from the current `Orengine` [`Context`].
    ///
    /// Use it in [`Future::poll`], because in other cases you can use
    /// [`Task::get_current_task`](Self::get_current).
    ///
    /// # Safety
    ///
    /// - Provided [`Context`] must be `Orengine's`
    ///   (created from [`Orengine's waker`](crate::runtime::waker::create_waker));
    ///
    /// - Using this function must comply with the concept of task ownership (read [`Task`]).
    #[inline]
    pub unsafe fn from_context(cx: &Context) -> Self {
        unsafe { std::ptr::read(cx.waker().data().cast()) }
    }

    /// Returns a [`Task`] from the current `Orengine` [`Context`].
    ///
    /// Use it outside [`Future::poll`], because in [`Future::poll`] you can use
    /// [`Task::from_context`](Self::from_context) and it is more readable in this case.
    ///
    /// # Safety
    ///
    /// - Provided [`Context`] must be `Orengine's`
    ///   (created from [`Orengine's waker`](crate::runtime::waker::create_waker));
    ///
    /// - Using this function must comply with the concept of task ownership (read [`Task`]).
    #[inline(always)]
    pub unsafe fn get_current() -> impl Future<Output = Self> {
        struct GetCurrentTask {}

        impl Future for GetCurrentTask {
            type Output = Task;

            #[inline(always)]
            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                Poll::Ready(unsafe { Task::from_context(cx) })
            }
        }

        GetCurrentTask {}
    }

    /// Returns the future that are wrapped by this [`Task`].
    ///
    /// # Safety
    ///
    /// It is safe because it returns a pointer without dereferencing it.
    ///
    /// Deref it only if you know what you are doing and remember that [`Task`] can be executed
    /// only by [`Executor::exec_task`](Executor::exec_task) and it can't be cloned, only moved.
    #[inline]
    pub fn future_ptr(&self) -> *mut dyn Future<Output = ()> {
        self.data.future_ptr()
    }

    /// Returns whether the task is local or not.
    ///
    /// The information about locality contains in [`Task`] but not under the pointer. So, this
    /// method don't read the pointer and is very cheap.
    #[inline]
    pub fn is_local(&self) -> bool {
        self.data.is_local()
    }

    // TODO docs
    pub unsafe fn park_current_task() -> impl Future<Output = ()> {
        #[repr(C)]
        struct ParkCurrentTask {
            was_called: bool,
        }

        impl Future for ParkCurrentTask {
            type Output = ();

            #[inline(always)]
            fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
                let this = &mut *self;

                match this.was_called {
                    true => Poll::Ready(()),
                    false => {
                        this.was_called = true;

                        Poll::Pending
                    }
                }
            }
        }

        ParkCurrentTask { was_called: false }
    }

    /// Checks if the task is safe to be executed.
    /// It checks `ref_count` and `executor_id` with locality.
    ///
    /// It is zero cost because it can be called only with `debug_assertions`.
    #[cfg(debug_assertions)]
    pub(crate) fn check_safety(&mut self) {
        if unsafe {
            self.is_executing
                .as_ref()
                .load(std::sync::atomic::Ordering::SeqCst)
        } {
            panic!(
                "Attempt to execute an already executing task! It is not allowed! \
            Try to rewrite the code to follow the concept of task ownership: \
            only one thread can own a task at the same time and only one task instance can exist."
            );
        }

        if self.is_local() && self.executor_id != local_executor().id() {
            if cfg!(test) && self.executor_id == usize::MAX {
                // All is ok
                self.executor_id = local_executor().id();
            } else {
                panic!("Local task has been moved to another executor!");
            }
        }
    }

    /// Puts it back to the [`TaskPool`](TaskPool). It is unsafe because you
    /// have to think about making sure it is no longer used.
    ///
    /// # Safety
    ///
    /// Provided [`Task`] is no longer used.
    #[inline]
    pub(crate) unsafe fn release(self, executor: &mut Executor) {
        executor.task_pool().put(self);
    }
}

impl PartialEq for Task {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
}

unsafe impl Send for Task {}
impl UnwindSafe for Task {}
impl RefUnwindSafe for Task {}

/// With `debug_assertions` checks if the [`Task`] is safe to be executed.
///
/// It compares an id of the [`Executor`](crate::Executor) of the current thread with an id of the executor of
/// the [`Task`], if the [`Task`] is `local`.
#[macro_export]
macro_rules! check_task_local_safety {
    ($task:expr) => {
        #[cfg(debug_assertions)]
        {
            if $task.is_local() && $crate::local_executor().id() != $task.executor_id {
                if cfg!(test) && $task.executor_id == usize::MAX {
                    // All is ok
                    $task.executor_id = $crate::local_executor().id();
                } else {
                    panic!(
                        "[BUG] Local task has been moved to another executor.\
                        Please report it. Provide details about the place where the problem occurred \
                        and the conditions under which it happened. \
                        Thank you for helping us make Orengine better!"
                    );
                }
            }
        }
    };
}

/// Gets the [`Task`] from the context and panics if it is `local`.
///
/// # Panics
///
/// If the [`Task`] associated with the context is `local`.
///
/// # Safety
///
/// Provided context contains a valid [`Task`] in `data` field (always true if you call it in
/// Orengine runtime).
#[macro_export]
macro_rules! panic_if_local_in_future {
    ($cx:expr, $name_of_future:expr) => {
        #[cfg(debug_assertions)]
        #[allow(
            clippy::macro_metavars_in_unsafe,
            reason = "else we need to allow unused `unsafe` for `release`"
        )]
        unsafe {
            let task = $crate::runtime::Task::from_context($cx);
            if task.is_local() {
                panic!(
                    "You cannot call a local task in {}, because it can be moved! \
                    Use shared task instead or use local structures if it is possible.",
                    $name_of_future
                );
            }
        }
    };
}

/// Update current [`task`](Task) locality via [`calling`](crate::Executor::invoke_call)
/// [`ChangeCurrentTaskLocality`](Call::ChangeCurrentTaskLocality).
///
/// It is unsafe because you have to think about making sure
/// that current task can have provided locality. Use it only if you know what you are doing.
/// Maybe it is the most unsafe function in the whole crate.
///
/// # Safety
///
/// Provided `locality` is valid for the current [`task`](Task).
pub async unsafe fn update_current_task_locality(locality: Locality) {
    struct UpdateCurrentTaskLocality {
        locality: Locality,
        was_called: bool,
    }

    impl Future for UpdateCurrentTaskLocality {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
            let this = &mut *self;

            if !this.was_called {
                this.was_called = true;

                unsafe {
                    local_executor().invoke_call(Call::change_current_task_locality(this.locality));
                };

                return Poll::Pending;
            }

            Poll::Ready(())
        }
    }

    UpdateCurrentTaskLocality {
        locality,
        was_called: false,
    }
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as orengine;
    use crate::{yield_now, Local};
    use std::ptr;

    #[orengine::test::test_local]
    fn test_update_current_task_locality() {
        struct GetCurrentTaskLocality {}

        impl Future for GetCurrentTaskLocality {
            type Output = bool;

            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let task = unsafe { Task::from_context(cx) };

                Poll::Ready(task.is_local())
            }
        }

        assert!(GetCurrentTaskLocality {}.await);

        unsafe {
            update_current_task_locality(Locality::shared()).await;
        }

        assert!(!GetCurrentTaskLocality {}.await);
    }

    #[orengine::test::test_local]
    fn test_park_current_task() {
        // TODO test it and write docs with it
        let task_to_unpark = Local::new(None);
        let task_to_unpark_clone = task_to_unpark.clone();
        let value = Local::new(0);

        local_executor().spawn_local(async {
            *task_to_unpark_clone.borrow_mut() = Some(unsafe { Task::get_current().await });

            *value.borrow_mut() += 1;

            unsafe { Task::park_current_task().await };

            *value.borrow_mut() += 1;
        });

        loop {
            yield_now().await;

            if let Some(task_to_unpark) = task_to_unpark.borrow_mut().take() {
                assert_eq!(*value.borrow(), 1);

                local_executor().exec_task(task_to_unpark);

                break;
            }
        }

        assert_eq!(*value.borrow(), 2);
    }

    #[orengine::test::test_local]
    fn test_task_eq() {
        let task1 = unsafe { Task::from_future(async {}, Locality::local()) };
        let task1_copy = unsafe { ptr::read(&task1) };
        let task2 = unsafe { Task::from_future(async {}, Locality::local()) };

        assert!(task1 == task1_copy, "Task should be equal to its copy");
        assert!(task1 != task2, "Task should not be equal to another task");
    }
}

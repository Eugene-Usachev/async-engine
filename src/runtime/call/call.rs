use crate::runtime::Task;
use crate::sync::{AsyncMutexGuard, Mutex, Unlock};
use crate::sync_task_queue::SyncTaskList;
use std::fmt::Debug;
use std::mem;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Represents a call from a `Future::poll` to the [`Executor`](crate::runtime::Executor).
///
/// The `Call` enum encapsulates different actions that an executor can take
/// after a future yields [`Poll::Pending`](std::task::Poll::Pending).
/// These actions may involve scheduling
/// the current task, signaling readiness, or interacting with sync primitives.
///
/// # Safety
///
/// After invoking a `Call`, the associated future **must** return `Poll::Pending` immediately.
/// The action defined by the `Call` will be executed **after** the future returns, ensuring
/// that it is safe to perform state transitions or task scheduling without directly affecting
/// the current state of the future. Use this mechanism only if you fully understand the
/// implications and safety concerns of moving a future between different states or threads.
#[derive(Default)]
pub enum Call {
    /// Does nothing
    #[default]
    None,
    /// Pushes the provided task to the given `AtomicTaskList`.
    ///
    /// # Safety
    ///
    /// * `send_to` must be a valid pointer to [`SyncTaskQueue`](SyncTaskList)
    ///
    /// * the reference must live at least as long as this state of the task
    ///
    /// * task must return [`Poll::Pending`](std::task::Poll::Pending) immediately after calling this function
    ///
    /// * calling task must be shared (else you don't need any [`Calls`](Call))
    PushCurrentTaskTo(NonNull<SyncTaskList>),
    /// Releases the given [`Mutex`].
    ///
    /// # Safety
    ///
    /// * the pointer must be a valid pointer to [`Mutex`]
    ///
    /// * the [`Mutex`] must live at least as long as this state of the task
    ReleaseMutex(NonNull<Mutex<()>>),
    /// Unlocks the given `lock`.
    ///
    /// # Safety
    ///
    /// * the pointer must be a valid pointer to [`Unlock`]
    ///
    /// * the `lock` must live at least as long as this state of the task
    ReleaseDynMutex(NonNull<dyn Unlock>),
    /// Pushes the current task to the given `AtomicTaskList` and removes it if the given `AtomicUsize`
    /// is `0` with given `Ordering` after removing executes it.
    ///
    /// # Safety
    ///
    /// * `send_to` must be a valid pointer to [`SyncTaskQueue`](SyncTaskList)
    ///
    /// * task must return [`Poll::Pending`](std::task::Poll::Pending) immediately after calling this function
    ///
    /// * counter must be a valid pointer to [`AtomicUsize`]
    ///
    /// * the references must live at least as long as this state of the task
    ///
    /// * calling task must be shared (else you don't need any [`Calls`](Call))
    PushCurrentTaskToAndRemoveItIfCounterIsZero(
        NonNull<SyncTaskList>,
        NonNull<AtomicUsize>,
        Ordering,
    ),
    /// Stores `false` for the given `AtomicBool` with [`Release`](Ordering::Release) ordering.
    ///
    /// # Safety
    ///
    /// * `atomic_bool` must be a valid pointer to [`AtomicBool`]
    ///
    /// * the [`AtomicBool`] must live at least as long as this state of the task
    ///
    /// * task must return [`Poll::Pending`](std::task::Poll::Pending) immediately after calling this function
    ///
    /// * calling task must be shared (else you don't need any [`Calls`](Call))
    ReleaseAtomicBool(NonNull<AtomicBool>),
    /// Pushes `f` to the blocking pool.
    ///
    /// # Safety
    ///
    /// * the [`Fn`] must live at least as long as this state of the task.
    ///
    /// * task must return [`Poll::Pending`](std::task::Poll::Pending) immediately after calling this function
    ///
    /// * calling task must be shared (else you don't need any [`Calls`](Call))
    PushFnToThreadPool(NonNull<dyn Fn()>),
    /// It is a fallback if [`Call`] don't support necessary action. If you think your action
    /// should be supported, please open an issue.
    ///
    /// # Safety
    ///
    /// Pointer must be a valid pointer to [`FnMut`] and must live at least as long as this
    /// state of the task.
    CallFn(*mut dyn FnMut(Task)),
}

impl Call {
    /// Returns `true` if the `Call` is [`None`](Self::None).
    #[inline]
    pub(crate) fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    /// Pushes the current task to the given `AtomicTaskList`.
    ///
    /// # Safety
    ///
    /// * `send_to` must be a valid pointer to [`SyncTaskQueue`](SyncTaskList)
    ///
    /// * the reference must live at least as long as this state of the task
    ///
    /// * task must return [`Poll::Pending`](std::task::Poll::Pending) immediately after calling this function
    ///
    /// * calling task must be shared (else you don't need any [`Calls`](Call))
    pub unsafe fn push_current_task_to(send_to: NonNull<SyncTaskList>) -> Self {
        Self::PushCurrentTaskTo(send_to)
    }

    /// Pushes the current task to the given `AtomicTaskList` and removes it if the given `AtomicUsize`
    /// is `0` with given `Ordering` after removing executes it.
    ///
    /// # Safety
    ///
    /// * `send_to` must be a valid pointer to [`SyncTaskQueue`](SyncTaskList)
    ///
    /// * task must return [`Poll::Pending`](std::task::Poll::Pending) immediately after calling this function
    ///
    /// * counter must be a valid pointer to [`AtomicUsize`]
    ///
    /// * the references must live at least as long as this state of the task
    ///
    /// * calling task must be shared (else you don't need any [`Calls`](Call))
    pub unsafe fn push_current_task_to_and_remove_it_if_counter_is_zero(
        send_to: NonNull<SyncTaskList>,
        counter: NonNull<AtomicUsize>,
        ordering: Ordering,
    ) -> Self {
        Self::PushCurrentTaskToAndRemoveItIfCounterIsZero(send_to, counter, ordering)
    }

    /// Stores `false` for the given `AtomicBool` with [`Release`](Ordering::Release) ordering.
    ///
    /// # Safety
    ///
    /// * `atomic_bool` must be a valid pointer to [`AtomicBool`]
    ///
    /// * the [`AtomicBool`] must live at least as long as this state of the task
    ///
    /// * task must return [`Poll::Pending`](std::task::Poll::Pending) immediately after calling this function
    ///
    /// * calling task must be shared (else you don't need any [`Calls`](Call))
    pub unsafe fn release_atomic_bool(atomic_bool: NonNull<AtomicBool>) -> Self {
        Self::ReleaseAtomicBool(atomic_bool)
    }

    /// Pushes `f` to the blocking pool.
    ///
    /// # Safety
    ///
    /// * the [`Fn`] must live at least as long as this state of the task.
    ///
    /// * task must return [`Poll::Pending`](std::task::Poll::Pending) immediately after calling this function
    ///
    /// * calling task must be shared (else you don't need any [`Calls`](Call))
    pub unsafe fn push_fn_to_thread_pool(f: NonNull<dyn Fn()>) -> Self {
        Self::PushFnToThreadPool(f)
    }

    /// Releases the `lock`.
    ///
    /// # Safety
    ///
    /// * the pointer must be a valid pointer to [`Unlock`]
    ///
    /// * the `lock` must live at least as long as this state of the task
    pub unsafe fn release_lock<'mutex, T, G>(guard: G) -> Self
    where
        T: 'mutex + ?Sized,
        G: AsyncMutexGuard<'mutex, T> + 'mutex,
        G::Mutex: Sized,
    {
        let dyn_unlock: &dyn Unlock = guard.mutex();
        let static_mutex: *mut dyn Unlock = unsafe { mem::transmute(dyn_unlock) };

        mem::forget(guard);

        unsafe { Self::ReleaseDynMutex(NonNull::new_unchecked(static_mutex)) }
    }

    /// It is a fallback if [`Call`] don't support necessary action. If you think your action
    /// should be supported, please open an issue.
    ///
    /// # Safety
    ///
    /// Pointer must be a valid pointer to [`FnMut`] and must live at least as long as this
    /// state of the task.
    pub unsafe fn call_fn(f: *mut dyn FnMut(Task)) -> Self {
        debug_assert!(!f.is_null());

        Self::CallFn(f)
    }
}

impl Debug for Call {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "Call::None"),
            Self::PushCurrentTaskTo(_) => write!(f, "Call::PushCurrentTaskTo"),
            Self::ReleaseMutex(_) => write!(f, "Call::ReleaseMutex"),
            Self::ReleaseDynMutex(_) => write!(f, "Call::ReleaseDynMutex"),
            Self::PushCurrentTaskToAndRemoveItIfCounterIsZero(_, _, _) => {
                write!(f, "Call::PushCurrentTaskToAndRemoveItIfCounterIsZero")
            }
            Self::ReleaseAtomicBool(_) => write!(f, "Call::ReleaseAtomicBool"),
            Self::PushFnToThreadPool(_) => write!(f, "Call::PushFnToThreadPool"),
            Self::CallFn(_) => write!(f, "Call::CallFn"),
        }
    }
}

impl Eq for Call {}

impl PartialEq for Call {
    fn eq(&self, other: &Self) -> bool {
        mem::discriminant(self) == mem::discriminant(other)
    }
}

#[cfg(test)]
mod tests {
    use crate as orengine;
    use crate::local_executor;
    use crate::runtime::{Call, Task};
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    #[orengine::test::test_local]
    fn test_call_user_fn() {
        struct WriteToPtr {
            was_called: bool,
            func: *mut dyn FnMut(Task),
        }

        impl Future for WriteToPtr {
            type Output = ();

            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let this = &mut *self;

                if !this.was_called {
                    this.was_called = true;
                    unsafe {
                        local_executor().invoke_call(Call::call_fn(this.func));
                        local_executor().spawn_local_task(Task::from_context(cx));
                    };

                    Poll::Pending
                } else {
                    Poll::Ready(())
                }
            }
        }

        let mut value = 0;
        let value_ptr = &raw mut value;
        let mut func = move |_| unsafe {
            *value_ptr = 3;
        };
        let future = WriteToPtr {
            was_called: false,
            func: &mut func,
        };

        future.await;

        assert_eq!(value, 3);
    }
}

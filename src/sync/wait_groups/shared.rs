//! This module contains the [`WaitGroup`].
use crate::panic_if_local_in_future;
use crate::runtime::{Call, IsLocal, Task, local_executor};
use crate::sync::wait_groups::AsyncWaitGroup;
use crate::sync::{AsyncMutex, NaiveMutex, NaiveMutexGuard};
use crate::utils::{TaskVecFromPool, acquire_task_vec_from_pool, clear_with};
use std::future::Future;
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::pin::Pin;
use std::task::{Context, Poll};

/// A [`Future`] to wait for all tasks in the [`WaitGroup`] to complete.
#[repr(C)]
pub struct WaitSharedWaitGroup<'wait_group> {
    maybe_guard: Option<NaiveMutexGuard<'wait_group, Inner>>,
}

impl<'wait_group> WaitSharedWaitGroup<'wait_group> {
    /// Creates a new [`WaitSharedWaitGroup`] future.
    #[inline]
    fn new(guard: NaiveMutexGuard<'wait_group, Inner>) -> Self {
        Self {
            maybe_guard: Some(guard),
        }
    }
}

impl Future for WaitSharedWaitGroup<'_> {
    type Output = ();

    #[allow(unused, reason = "Here we use #[cfg(debug_assertions)].")]
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = &mut *self;
        unsafe { panic_if_local_in_future!(cx, "WaitGroup") };

        if let Some(mut guard) = this.maybe_guard.take() {
            debug_assert!(guard.counter > 0);

            guard.waiting_tasks.push(unsafe { Task::from_context(cx) });

            unsafe {
                local_executor().invoke_call(Call::release_atomic_bool(guard.leak_to_atomic()))
            };

            return Poll::Pending;
        }

        Poll::Ready(())
    }
}

/// Inner of [`WaitGroup`].
struct Inner {
    counter: usize,
    waiting_tasks: TaskVecFromPool,
}

/// `WaitGroup` is a synchronization primitive that allows to [`wait`](Self::wait)
/// until all tasks are [`completed`](Self::done).
///
/// # The difference between `WaitGroup` and [`LocalWaitGroup`](crate::sync::LocalWaitGroup)
///
/// The `WaitGroup` works with `shared tasks` and can be shared between threads.
///
/// Read [`Executor`](crate::Executor) for more details.
///
/// # Example
///
/// ```rust
/// use std::sync::Arc;
/// use std::time::Duration;
/// use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
/// use orengine::{local_executor, sleep};
/// use orengine::sync::{AsyncWaitGroup, WaitGroup};
///
/// # async fn foo() {
/// let wait_group = Arc::new(WaitGroup::new_with_count(10));
/// let number_executed_tasks = Arc::new(AtomicUsize::new(0));
///
/// for i in 0..10 {
///     let wait_group = wait_group.clone();
///     let number_executed_tasks = number_executed_tasks.clone();
///
///     local_executor().spawn_shared(async move {
///         sleep(Duration::from_millis(i)).await;
///
///         number_executed_tasks.fetch_add(1, SeqCst);
///
///         wait_group.done().await;
///     });
/// }
///
/// wait_group.wait().await; // wait until all tasks are completed
/// assert_eq!(number_executed_tasks.load(SeqCst), 10);
/// # }
/// ```
pub struct WaitGroup {
    inner: NaiveMutex<Inner>,
}

impl WaitGroup {
    /// Creates a new `WaitGroup` with the specified count.
    ///
    /// # Example
    ///
    /// ``` no_run
    /// use std::sync::Arc;
    /// use std::time::Duration;
    /// use orengine::{local_executor, sleep};
    /// use orengine::sync::{AsyncWaitGroup, WaitGroup};
    ///
    /// # async fn wg_new_with_count_example() {
    /// let mut wg = Arc::new(WaitGroup::new_with_count(10));
    ///
    /// for i in 0..10 {
    ///     let wg = wg.clone();
    ///
    ///     local_executor().spawn_shared(async move {
    ///         sleep(Duration::from_millis(i)).await;
    ///
    ///         wg.done().await;
    ///     });
    /// }
    ///
    /// wg.wait().await;
    /// # }
    /// ```
    pub fn new_with_count(count: usize) -> Self {
        Self {
            inner: NaiveMutex::new(Inner {
                counter: count,
                waiting_tasks: acquire_task_vec_from_pool(),
            }),
        }
    }

    /// Creates a new `WaitGroup`.
    pub fn new() -> Self {
        Self::new_with_count(0)
    }

    /// Sets the count.
    /// It accepts a mutable reference, therefore, it doesn't use atomic operations.
    ///
    /// # Example
    ///
    /// ``` no_run
    /// use std::sync::Arc;
    /// use std::time::Duration;
    /// use orengine::{local_executor, sleep};
    /// use orengine::sync::{AsyncWaitGroup, WaitGroup};
    ///
    /// fn acquire_wg() -> WaitGroup { WaitGroup::new() }
    ///
    /// # async fn wg_set_example() {
    /// let mut wg = acquire_wg();
    /// wg.set_mut(10);
    ///
    /// let wg = Arc::new(wg);
    ///
    /// for i in 0..10 {
    ///     let wg = wg.clone();
    ///
    ///     local_executor().spawn_shared(async move {
    ///         sleep(Duration::from_millis(i)).await;
    ///
    ///         wg.done().await;
    ///     });
    /// }
    ///
    /// wg.wait().await;
    /// # }
    /// ```
    pub fn set_mut(&mut self, count: usize) {
        self.inner.get_mut().counter = count;
    }

    /// Wakes up all waiting tasks.
    fn wake_all_waiters(inner: &mut Inner) {
        clear_with(&mut inner.waiting_tasks, |task| {
            local_executor().spawn_shared_task(task);
        });
    }
}

impl IsLocal for WaitGroup {
    const IS_LOCAL: bool = false;
}

impl AsyncWaitGroup for WaitGroup {
    #[inline]
    async fn add(&self, count: usize) {
        let mut inner = self.inner.lock().await;

        debug_assert!(inner.counter < usize::MAX / 4, "WaitGroup counter overflow");

        inner.counter += count;
    }

    #[inline]
    async fn count(&self) -> usize {
        let inner = self.inner.lock().await;

        debug_assert!(inner.counter < usize::MAX / 4, "WaitGroup counter overflow");

        inner.counter
    }

    #[inline]
    async fn done(&self) -> usize {
        let mut inner = self.inner.lock().await;

        debug_assert!(
            inner.counter > 0,
            "WaitGroup::done called after counter reached 0"
        );
        debug_assert!(inner.counter < usize::MAX / 4, "WaitGroup counter overflow");

        if inner.counter == 1 {
            Self::wake_all_waiters(&mut inner);
        }

        inner.counter -= 1;

        inner.counter
    }

    #[inline]
    async fn wait(&self) {
        let guard = self.inner.lock().await;

        if guard.counter > 0 {
            WaitSharedWaitGroup::new(guard).await;
        }
    }
}

impl Default for WaitGroup {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl Sync for WaitGroup {}
unsafe impl Send for WaitGroup {}
impl UnwindSafe for WaitGroup {}
impl RefUnwindSafe for WaitGroup {}

// TODO
impl<WG, T> AsyncWaitGroup for T
where
    WG: AsyncWaitGroup,
    T: std::ops::Deref<Target = WG>,
{
    #[inline]
    #[allow(
        clippy::future_not_send,
        reason = "It is not send for non send types, it is fine"
    )]
    async fn add(&self, count: usize) {
        self.deref().add(count).await;
    }

    #[inline]
    #[allow(
        clippy::future_not_send,
        reason = "It is not send for non send types, it is fine"
    )]
    async fn count(&self) -> usize {
        self.deref().count().await
    }

    #[inline]
    #[allow(
        clippy::future_not_send,
        reason = "It is not send for non send types, it is fine"
    )]
    async fn done(&self) -> usize {
        self.deref().done().await
    }

    #[inline]
    #[allow(
        clippy::future_not_send,
        reason = "It is not `Send` only when T is not `Send`, it is fine"
    )]
    async fn wait(&self) {
        self.deref().wait().await;
    }
}

/// ```rust
/// use orengine::sync::{WaitGroup, AsyncWaitGroup};
/// use orengine::yield_now;
///
/// fn check_send<T: Send>(value: T) -> T { value }
///
/// async fn test() {
///     let wg = WaitGroup::new();
///     let _ = check_send(wg.wait()).await;
/// }
/// ```
#[allow(dead_code, reason = "It is used only in compile tests")]
fn test_compile_shared_wait_group() {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as orengine;
    use crate::sync::{AsyncMutex, Mutex};
    use crate::test::sched_future;
    use crate::{sleep, yield_now};
    use std::sync::Arc;
    use std::time::Duration;

    const PAR: usize = 10;

    #[orengine::test::test_shared]
    fn test_shared_wg_many_wait_one() {
        let check_value = Arc::new(Mutex::new(false));
        let wait_group = Arc::new(WaitGroup::new());

        wait_group.inc().await;

        for _ in 0..PAR {
            let check_value = check_value.clone();
            let wait_group = wait_group.clone();

            sched_future(async move {
                wait_group.wait().await;

                assert!(*check_value.lock().await, "not waited");
            });
        }

        yield_now().await;

        *check_value.lock().await = true;

        wait_group.done().await;
    }

    #[orengine::test::test_shared]
    fn test_shared_wg_one_wait_many_task_finished_after_wait() {
        let check_value = Arc::new(std::sync::Mutex::new(PAR));
        let wait_group = Arc::new(WaitGroup::new());

        wait_group.add(PAR).await;

        for _ in 0..PAR {
            let check_value = check_value.clone();
            let wait_group = wait_group.clone();

            sched_future(async move {
                *check_value.lock().unwrap() -= 1;

                sleep(Duration::from_millis(100)).await;

                wait_group.done().await;
            });
        }

        wait_group.wait().await;

        assert_eq!(*check_value.lock().unwrap(), 0, "not waited");
    }

    #[orengine::test::test_shared]
    fn test_shared_wg_one_wait_many_task_finished_before_wait() {
        let check_value = Arc::new(std::sync::Mutex::new(PAR));
        let wait_group = Arc::new(WaitGroup::new());

        wait_group.add(PAR).await;

        for _ in 0..PAR {
            let check_value = check_value.clone();
            let wait_group = wait_group.clone();

            sched_future(async move {
                *check_value.lock().unwrap() -= 1;

                wait_group.done().await;
            });
        }

        wait_group.wait().await;

        assert_eq!(*check_value.lock().unwrap(), 0, "not waited");
    }

    #[orengine::test::test_shared]
    fn test_shared_wg_as_barrier() {
        let check_value = Arc::new(Mutex::new(0));
        let wait_group = Arc::new(WaitGroup::new());

        wait_group.add(6).await;

        for _ in 0..5 {
            let check_value = check_value.clone();
            let wait_group = wait_group.clone();

            local_executor().spawn_shared(async move {
                yield_now().await;

                *check_value.lock().await += 1;

                wait_group.done().await;
                wait_group.wait().await;

                assert_eq!(*check_value.lock().await, 6);
            });
        }

        yield_now().await;

        *check_value.lock().await += 1;

        wait_group.done().await;

        wait_group.wait().await;

        assert_eq!(*check_value.lock().await, 6);
    }
}

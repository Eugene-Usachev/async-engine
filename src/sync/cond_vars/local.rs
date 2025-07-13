//! This module contains the [`LocalCondVar`] struct that implements the [`AsyncCondVar`].
use crate::runtime::{Task, local_executor};
use crate::sync::mutexes::AsyncSubscribableMutex;
use crate::sync::{AsyncCondVar, AsyncMutex, AsyncMutexGuard, LocalMutex};
use crate::utils::{
    PairedWithLock, TaskVecFromPool, acquire_task_vec_from_pool, unlikely, unwrap_or_bug_hint,
};
use std::marker::PhantomData;
use std::mem;
use std::ops::Deref;
use std::pin::Pin;
use std::task::{Context, Poll};

/// `WaitLocalCondVar` is a future that waits until the [`LocalCondVar`] is notified.
#[repr(C)]
struct WaitLocalCondVar<'cond_var, T: 'cond_var, M: AsyncSubscribableMutex<T> + 'cond_var> {
    cond_var: &'cond_var LocalCondVar<T, M>,
    was_called: bool,
    _non_send: PhantomData<*const ()>,
}

impl<'cond_var, T: 'cond_var, M: AsyncSubscribableMutex<T> + 'cond_var>
    WaitLocalCondVar<'cond_var, T, M>
{
    /// Creates a new [`WaitLocalCondVar`] instant.
    fn new(cond_var: &'cond_var LocalCondVar<T, M>) -> Self {
        debug_assert!(cond_var.is_locked());

        Self {
            cond_var,
            was_called: false,
            _non_send: PhantomData,
        }
    }
}

impl<'cond_var, T: 'cond_var, M: AsyncSubscribableMutex<T> + 'cond_var> Future
    for WaitLocalCondVar<'cond_var, T, M>
{
    type Output = M::Guard<'cond_var>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        if !this.was_called {
            this.was_called = true;

            let list = this.cond_var.list.get_by_mutex(this.cond_var);

            unsafe {
                list.push(Task::from_context(cx));

                this.cond_var.unlock();
            }

            return Poll::Pending;
        }

        Poll::Ready(unsafe { self.cond_var.get_locked() })
    }
}

#[cfg(debug_assertions)]
impl<'cond_var, T: 'cond_var, M: AsyncSubscribableMutex<T> + 'cond_var> Drop
    for WaitLocalCondVar<'cond_var, T, M>
{
    fn drop(&mut self) {
        debug_assert!(
            self.was_called,
            "WaitLocalCondVar was not awaited. It can lead to deadlocks."
        );
    }
}

/// `LocalCondVar` is a condition variable that allows tasks to wait until
/// notified by another task.
///
/// It is designed to be used in conjunction with a [`LocalMutex`] to provide a way for tasks
/// to wait for a specific condition to occur.
///
/// # About consuming the [`AsyncMutex`]
///
/// Almost always one [`AsyncCondVar`] is used only with one [`AsyncMutex`].
/// Therefore, this implementation consumes it to prevent misuses and to improve performance.
///
/// But [`AsyncCondVar`] can be dereferenced to its [`AsyncMutex`] to get access to the inner value.
///
/// # The difference between `LocalCondVar` and [`CondVar`](crate::sync::CondVar)
///
/// The `LocalCondVar` works with `local tasks`.
///
/// Read [`Executor`](crate::Executor) for more details.
///
/// # Example
///
/// ```rust
/// use std::rc::Rc;
/// use orengine::sync::{LocalCondVar, LocalMutex, AsyncMutex, AsyncCondVar};
/// use orengine::{local_executor, sleep};
/// use std::time::Duration;
///
/// # async fn test() {
/// let is_ready = Rc::new(LocalCondVar::new(LocalMutex::new(false)));
/// let is_ready_clone = is_ready.clone();
///
/// local_executor().spawn_local(async move {
///     sleep(Duration::from_secs(1)).await;
///
///     let mut lock = is_ready_clone.lock().await;
///
///     *lock = true;
///
///     is_ready_clone.notify_one(lock);
/// });
///
/// let mut lock = is_ready.lock().await;
/// while !*lock {
///     lock = is_ready.wait(lock).await; // wait 1 second
/// }
/// # }
/// ```
pub struct LocalCondVar<T, S: AsyncSubscribableMutex<T> = LocalMutex<T>> {
    mutex: S,
    list: PairedWithLock<TaskVecFromPool, T, S>,
    // impl !Send
    no_send_marker: PhantomData<*const T>,
}

impl<T, S: AsyncSubscribableMutex<T>> LocalCondVar<T, S> {
    /// Creates a new [`LocalCondVar`].
    pub fn new(mutex: S) -> Self {
        Self {
            mutex,
            list: PairedWithLock::new(acquire_task_vec_from_pool()),
            no_send_marker: PhantomData,
        }
    }
}

impl<T, S: AsyncSubscribableMutex<T>> AsyncCondVar<T> for LocalCondVar<T, S> {
    type Mutex = S;

    #[allow(clippy::future_not_send, reason = "It is `local`.")]
    fn wait<'lock>(
        &'lock self,
        guard: <S as AsyncMutex<T>>::Guard<'lock>,
    ) -> impl Future<Output = <S as AsyncMutex<T>>::Guard<'lock>>
    where
        T: 'lock,
    {
        if cfg!(debug_assertions) {
            assert_eq!(
                std::ptr::from_ref(guard.mutex()) as usize,
                &raw const self.mutex as usize,
                "Attempt to wait on condvar from different mutex"
            );
        }

        mem::forget(guard);

        WaitLocalCondVar::new(self)
    }

    fn notify_one(&self, guard: <S as AsyncMutex<T>>::Guard<'_>) {
        let list = self.list.get(&guard);

        if let Some(task) = list.pop() {
            let _ = unsafe { guard.leak() };

            local_executor().exec_task(task);
        }
    }

    fn notify_all(&self, mut guard: <S as AsyncMutex<T>>::Guard<'_>) {
        let list = self.list.get(&guard);
        let len = list.len();

        if unlikely(len == 0) {
            return;
        }

        let task = list.pop().unwrap();

        for _ in 0..len - 1 {
            let task = unwrap_or_bug_hint(list.pop());

            self.mutex.subscribe_task(task, &mut guard);
        }

        let _ = unsafe { guard.leak() };

        local_executor().exec_task(task);
    }
}

impl<T, S: AsyncSubscribableMutex<T>> Deref for LocalCondVar<T, S> {
    type Target = S;

    fn deref(&self) -> &Self::Target {
        &self.mutex
    }
}
unsafe impl<T, S: AsyncSubscribableMutex<T>> Sync for LocalCondVar<T, S> {}

/// ```compile_fail
/// use orengine::sync::{LocalMutex, LocalCondVar, AsyncMutex, AsyncCondVar};
/// use orengine::yield_now;
///
/// fn check_send<T: Send>(value: T) -> T { value }
///
/// struct NonSend {
///     value: i32,
///     // impl !Send
///     no_send_marker: std::marker::PhantomData<*const ()>,
/// }
///
/// async fn test() {
///     let mutex = LocalMutex::new(NonSend {
///         value: 0,
///         no_send_marker: std::marker::PhantomData,
///     });
///     let cvar = LocalCondVar::new(mutex);
///     let mut guard = cvar.lock().await;
///
///     guard = check_send(cvar.wait(guard)).await;
///
///     yield_now().await;
///
///     assert_eq!(guard.value, 0);
///
///     drop(guard);
/// }
/// ```
///
/// ```compile_fail
/// use orengine::sync::{LocalMutex, LocalCondVar, AsyncMutex, AsyncCondVar};
/// use orengine::yield_now;
///
/// fn check_send<T: Send>(value: T) -> T { value }
///
/// // impl Send
/// struct CanSend {
///     value: i32,
/// }
///
/// async fn test() {
///     let mutex = LocalMutex::new(CanSend {
///         value: 0,
///     });
///     let cvar = LocalCondVar::new(mutex);
///     let mut guard = cvar.lock().await;
///
///     guard = check_send(cvar.wait(guard)).await;
///
///     yield_now().await;
///
///     assert_eq!(guard.value, 0);
///
///     drop(guard);
/// }
/// ```
#[allow(dead_code, reason = "It is used only in compile tests")]
fn test_compile_local_cond_var() {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as orengine;
    use crate::runtime::local_executor;
    use crate::sleep::sleep;
    use crate::sync::{AsyncMutex, AsyncWaitGroup, LocalMutex, LocalWaitGroup};
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    const TIME_TO_SLEEP: Duration = Duration::from_millis(1);

    #[allow(clippy::future_not_send, reason = "It is local.")]
    async fn test_notify_one() {
        let start = Instant::now();
        let cvar = Rc::new(LocalCondVar::new(LocalMutex::new(false)));
        let cvar2 = cvar.clone();

        // Inside our lock, spawn a new thread and then wait for it to start.
        local_executor().spawn_local(async move {
            let mut started = cvar2.lock().await;

            sleep(TIME_TO_SLEEP).await;

            *started = true;

            // We notify the condvar that the value has changed.
            cvar2.notify_one(started);
        });

        // Wait for the thread to start up.
        let mut started = cvar.lock().await;
        while !*started {
            started = cvar.wait(started).await;
        }

        assert!(start.elapsed() >= TIME_TO_SLEEP);
    }

    #[allow(clippy::future_not_send, reason = "It is local.")]
    async fn test_notify_all() {
        const NUMBER_OF_WAITERS: usize = 10;

        let start = Instant::now();
        let cvar = Rc::new(LocalCondVar::new(LocalMutex::new(false)));
        let cvar2 = cvar.clone();

        // Inside our lock, spawn a new thread and then wait for it to start.
        local_executor().spawn_local(async move {
            sleep(TIME_TO_SLEEP).await;

            let mut started = cvar2.lock().await;

            *started = true;

            // We notify the condvar that the value has changed.
            cvar2.notify_all(started);
        });

        let wg = Rc::new(LocalWaitGroup::new());

        for _ in 0..NUMBER_OF_WAITERS {
            let cvar = cvar.clone();
            let wg = wg.clone();

            wg.add(1).await;

            local_executor().spawn_local(async move {
                let mut started = cvar.lock().await;

                while !*started {
                    started = cvar.wait(started).await;
                }

                wg.done().await;
            });
        }

        wg.wait().await;

        assert!(start.elapsed() >= TIME_TO_SLEEP);
    }

    #[orengine::test::test_local]
    fn test_local_cond_var_notify_one() {
        test_notify_one().await;
    }

    #[orengine::test::test_local]
    fn test_local_cond_var_notify_all() {
        test_notify_all().await;
    }
}

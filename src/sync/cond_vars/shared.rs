use crate::panic_if_local_in_future;
use crate::runtime::{Call, Task, local_executor};
use crate::sync::{AsyncCondVar, AsyncMutex, AsyncMutexGuard, AsyncSubscribableMutex, Mutex};
use crate::utils::{
    PairedWithLock, TaskVecFromPool, acquire_task_vec_from_pool, unlikely, unwrap_or_bug_hint,
};
use std::mem;
use std::ops::Deref;
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::pin::Pin;
use std::task::{Context, Poll};

/// `WaitCondVar` is a future that waits until the [`CondVar`] is notified.
#[repr(C)]
struct WaitCondVar<'cond_var, T: 'cond_var, M: AsyncSubscribableMutex<T> + 'cond_var> {
    cond_var: &'cond_var CondVar<T, M>,
    was_called: bool,
}

impl<'cond_var, T: 'cond_var, M: AsyncSubscribableMutex<T> + 'cond_var>
    WaitCondVar<'cond_var, T, M>
{
    /// Creates a new [`WaitCondVar`] instant.
    fn new(cond_var: &'cond_var CondVar<T, M>) -> Self {
        debug_assert!(cond_var.is_locked());

        Self {
            cond_var,
            was_called: false,
        }
    }
}

impl<'cond_var, T: 'cond_var, M: AsyncSubscribableMutex<T> + 'cond_var> Future
    for WaitCondVar<'cond_var, T, M>
{
    type Output = M::Guard<'cond_var>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        panic_if_local_in_future!(cx, "CondVar");

        let this = &mut *self;

        if !this.was_called {
            this.was_called = true;

            let list = this.cond_var.list.get_by_mutex(this.cond_var);

            unsafe {
                list.push(Task::from_context(cx));

                local_executor().invoke_call(Call::release_lock(&this.cond_var.mutex));
            }

            return Poll::Pending;
        }

        Poll::Ready(unsafe { self.cond_var.get_locked() })
    }
}

#[cfg(debug_assertions)]
impl<'cond_var, T: 'cond_var, M: AsyncSubscribableMutex<T> + 'cond_var> Drop
    for WaitCondVar<'cond_var, T, M>
{
    fn drop(&mut self) {
        debug_assert!(
            self.was_called,
            "WaitCondVar was not awaited. It can lead to deadlocks."
        );
    }
}

/// `CondVar` is a condition variable that allows tasks to wait until
/// notified by another task.
///
/// It is designed to be used in conjunction with a [`Mutex`] to provide a way for tasks
/// to wait for a specific condition to occur.
///
/// # About consuming the [`AsyncMutex`]
///
/// Almost always one [`AsyncCondVar`] is used only with one [`AsyncMutex`].
/// Therefore, this implementation consumes it to prevent misuses and to improve performance.
///
/// But [`AsyncCondVar`] can be dereferenced to its [`AsyncMutex`] to get access to the inner value.
///
/// # The difference between `CondVar` and [`LocalCondVar`](crate::sync::LocalCondVar)
///
/// The `CondVar` works with `shared tasks` and can be shared between threads.
///
/// Read [`Executor`](crate::Executor) for more details.
///
/// # Example
///
/// ```rust
/// use std::sync::Arc;
/// use orengine::sync::{CondVar, Mutex, AsyncMutex, AsyncCondVar};
/// use orengine::{local_executor, sleep};
/// use std::time::Duration;
///
/// # async fn test() {
/// let is_ready = Arc::new(CondVar::new(Mutex::new(false)));
/// let is_ready_clone = is_ready.clone();
///
/// local_executor().spawn_shared(async move {
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
pub struct CondVar<T, S: AsyncSubscribableMutex<T> = Mutex<T>> {
    mutex: S,
    list: PairedWithLock<TaskVecFromPool, T, S>,
    phantom: std::marker::PhantomData<T>,
}

impl<T, S: AsyncSubscribableMutex<T>> CondVar<T, S> {
    /// Creates a new [`CondVar`].
    pub fn new(mutex: S) -> Self {
        Self {
            mutex,
            list: PairedWithLock::new(acquire_task_vec_from_pool()),
            phantom: std::marker::PhantomData,
        }
    }
}

impl<T, S> AsyncCondVar<T> for CondVar<T, S>
where
    S: AsyncSubscribableMutex<T>,
{
    type Mutex = S;

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

        WaitCondVar::new(self)
    }

    fn notify_one(&self, guard: <S as AsyncMutex<T>>::Guard<'_>) {
        let list = self.list.get(&guard);

        if let Some(task) = list.pop() {
            let _ = unsafe { guard.leak() };

            local_executor().spawn_shared_task(task);
        }
    }

    fn notify_all(&self, guard: <S as AsyncMutex<T>>::Guard<'_>) {
        let list = self.list.get(&guard);
        let len = list.len();

        if unlikely(len == 0) {
            return;
        }

        let _ = unsafe { guard.leak() };

        let task = unwrap_or_bug_hint(list.pop());

        for _ in 0..len - 1 {
            let task = unwrap_or_bug_hint(list.pop());

            self.mutex.subscribe_task(task);
        }

        local_executor().spawn_shared_task(task);
    }
}

impl<T, S: AsyncSubscribableMutex<T>> Deref for CondVar<T, S> {
    type Target = S;

    fn deref(&self) -> &Self::Target {
        &self.mutex
    }
}

unsafe impl<T, S: AsyncSubscribableMutex<T>> Sync for CondVar<T, S> {}
unsafe impl<T: Send, S: AsyncSubscribableMutex<T> + Send> Send for CondVar<T, S> {}
impl<T, S: AsyncSubscribableMutex<T>> UnwindSafe for CondVar<T, S> {}
impl<T, S: AsyncSubscribableMutex<T>> RefUnwindSafe for CondVar<T, S> {}

/// ```compile_fail
/// use orengine::sync::{Mutex, CondVar, AsyncMutex, AsyncCondVar};
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
///     let cvar = CondVar::new(Mutex::new(NonSend {
///         value: 0,
///         no_send_marker: std::marker::PhantomData,
///     });
///     let mut guard = mutex.lock().await;
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
/// ```rust
/// use orengine::sync::{Mutex, CondVar, AsyncMutex, AsyncCondVar};
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
///     let cvar = CondVar::new(Mutex::new(CanSend { value: 0 }));
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
fn test_compile_shared_cond_var() {}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use crate::runtime::local_executor;
    use crate::sleep::sleep;
    use crate::sync::{AsyncMutex, AsyncWaitGroup, WaitGroup};

    use super::*;
    use crate as orengine;
    use crate::test::sched_future_to_another_thread;

    const TIME_TO_SLEEP: Duration = Duration::from_millis(10);

    async fn test_notify_one() {
        let start = Instant::now();
        let cvar = Arc::new(CondVar::new(Mutex::new(false)));
        let cvar2 = cvar.clone();

        sched_future_to_another_thread(async move {
            let mut started = cvar2.lock().await;

            sleep(TIME_TO_SLEEP).await;

            *started = true;

            cvar2.notify_one(started);
        });

        let mut started = cvar.lock().await;
        while !*started {
            started = cvar.wait(started).await;
        }

        assert!(start.elapsed() >= TIME_TO_SLEEP);
    }

    async fn test_notify_all() {
        const NUMBER_OF_WAITERS: usize = 4;

        let start = Instant::now();
        let cvar = Arc::new(CondVar::new(Mutex::new(false)));
        let cvar2 = cvar.clone();

        local_executor().spawn_shared(async move {
            sleep(TIME_TO_SLEEP).await;

            let mut started = cvar2.lock().await;

            *started = true;

            cvar2.notify_all(started);
        });

        let wg = Arc::new(WaitGroup::new());
        for _ in 0..NUMBER_OF_WAITERS {
            let cvar = cvar.clone();
            let wg = wg.clone();

            wg.add(1);

            sched_future_to_another_thread(async move {
                let mut started = cvar.lock().await;

                while !*started {
                    started = cvar.wait(started).await;
                }

                wg.done();
            });
        }

        wg.wait().await;

        assert!(start.elapsed() >= TIME_TO_SLEEP);
    }

    #[orengine::test::test_shared]
    fn test_shared_cond_var_notify_one() {
        test_notify_one().await;
    }

    #[orengine::test::test_shared]
    fn test_shared_cond_var_notify_all() {
        test_notify_all().await;
    }
}

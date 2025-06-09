use crate::runtime::IsLocal;
use crate::sync::AsyncMutex;
use crate::sync::mutexes::AsyncSubscribableMutex;
use std::future::Future;
use std::ops::Deref;

/// `AsyncCondVar` is a `condition variable` that allows tasks to wait until
/// notified by another task.
///
/// # About consuming the [`AsyncMutex`]
///
/// Almost always one [`AsyncCondVar`] is used only with one [`AsyncMutex`].
/// Therefore, this implementation consumes it to prevent misuses and to improve performance.
///
/// But [`AsyncCondVar`] can be dereferenced to its [`AsyncMutex`] to get access to the inner value.
///
/// It is designed to be used in conjunction with a [`AsyncSubscribableMutex`] to provide
/// a way for tasks to wait for a specific condition to occur.
///
/// # Examples
///
/// Read the documentation of [`LocalCondVar`](crate::sync::LocalCondVar)
/// and [`CondVar`](crate::sync::CondVar) for examples.
pub trait AsyncCondVar<T>: IsLocal + Deref<Target = Self::Mutex> {
    type Mutex: AsyncSubscribableMutex<T>;

    /// Wait for a notification.
    ///
    /// # Example
    ///
    /// ```no_run
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
    fn wait<'lock>(
        &'lock self,
        guard: <Self::Mutex as AsyncMutex<T>>::Guard<'lock>,
    ) -> impl Future<Output = <Self::Mutex as AsyncMutex<T>>::Guard<'lock>>
    where
        T: 'lock;

    /// Blocks the current [`Task`] until the provided condition becomes false.
    ///
    /// `condition` is checked immediately; if not met (returns `true`), this
    /// will [`wait`](Self::wait) for the next notification then check again. This repeats
    /// until `condition` returns `false`, in which case this function returns.
    ///
    /// This function will atomically unlock the mutex specified (represented by
    /// `guard`) and block the current [`Task`]. This means that any calls
    /// to [`notify_one`] or [`notify_all`] which happen logically after the
    /// mutex is unlocked are candidates to wake this [`Task`] up. When this
    /// function call returns, the lock specified will have been re-acquired.
    ///
    /// [`notify_one`]: Self::notify_one
    /// [`notify_all`]: Self::notify_all
    /// [`Task`]: crate::runtime::Task
    ///
    /// # Example
    ///
    /// ```no_run
    /// use std::rc::Rc;
    /// use orengine::sync::{LocalCondVar, LocalMutex, AsyncMutex, AsyncCondVar};
    /// use orengine::{local_executor, sleep};
    /// use std::time::Duration;
    ///
    /// # async fn test() {
    ///
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
    /// let mut is_ready_lock = is_ready.wait_while(
    ///     is_ready.lock().await,
    ///     |lock| !*lock
    /// ).await; // wait 1 second
    /// # }
    /// ```
    async fn wait_while<'lock>(
        &'lock self,
        guard: <Self::Mutex as AsyncMutex<T>>::Guard<'lock>,
        predicate: impl Fn(&mut T) -> bool,
    ) -> <Self::Mutex as AsyncMutex<T>>::Guard<'lock>
    where
        T: 'lock,
    {
        let mut guard = guard;
        while predicate(&mut guard) {
            guard = self.wait(guard).await;
        }

        guard
    }

    /// Notifies one waiting task.
    ///
    /// # Example
    ///
    /// ```rust
    /// use orengine::sync::{AsyncSubscribableMutex, AsyncCondVar, AsyncMutex};
    ///
    /// async fn inc_counter_and_notify_one<'mutex, CondVar>(counter: &CondVar)
    /// where
    ///     CondVar: AsyncCondVar<i32>
    /// {
    ///     let mut lock = counter.lock().await;
    ///
    ///     *lock += 1;
    ///
    ///     counter.notify_one(lock);
    /// }
    /// ```
    fn notify_one(&self, guard: <Self::Mutex as AsyncMutex<T>>::Guard<'_>);

    /// Notifies all waiting tasks.
    ///
    /// # Example
    ///
    /// ```rust
    /// use orengine::sync::{AsyncSubscribableMutex, AsyncCondVar, AsyncMutex};
    ///
    /// async fn inc_counter_and_notify_all<'mutex, CondVar>(counter: &CondVar)
    /// where
    ///     CondVar: AsyncCondVar<i32>
    /// {
    ///     let mut lock = counter.lock().await;
    ///
    ///     *lock += 1;
    ///
    ///     counter.notify_all(lock);
    /// }
    /// ```
    fn notify_all(&self, guard: <Self::Mutex as AsyncMutex<T>>::Guard<'_>);
}

#[cfg(test)]
mod tests {
    use crate as orengine;
    use crate::sync::{AsyncCondVar, AsyncMutex, LocalCondVar, LocalMutex};
    use crate::{local_executor, sleep};
    use std::rc::Rc;
    use std::time::Duration;

    #[orengine::test::test_local]
    fn test_cond_var_wait_while() {
        let is_ready = Rc::new(LocalCondVar::new(LocalMutex::new(false)));
        let is_ready_clone = is_ready.clone();

        local_executor().spawn_local(async move {
            sleep(Duration::from_secs(1)).await;

            let mut lock = is_ready_clone.lock().await;
            *lock = true;

            is_ready_clone.notify_one(lock);
        });

        let is_ready_lock = is_ready
            .wait_while(is_ready.lock().await, |lock| !*lock)
            .await; // wait 1 second

        assert!(*is_ready_lock);
    }
}

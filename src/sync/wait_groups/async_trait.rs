use crate::runtime::IsLocal;
use std::future::Future;

/// `AsyncWaitGroup` is a synchronization primitive that allows to [`wait`](Self::wait)
/// until all tasks are [`completed`](Self::done).
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
/// let wait_group = Arc::new(WaitGroup::new());
/// let number_executed_tasks = Arc::new(AtomicUsize::new(0));
///
/// for i in 0..10 {
///     let number_executed_tasks = number_executed_tasks.clone();
///     let wait_group = wait_group.clone();
///
///     wait_group.inc();
///
///     local_executor().spawn_shared(async move {
///         sleep(Duration::from_millis(i)).await;
///         number_executed_tasks.fetch_add(1, SeqCst);
///         wait_group.done();
///     });
/// }
///
/// wait_group.wait().await; // wait until all tasks are completed
/// assert_eq!(number_executed_tasks.load(SeqCst), 10);
/// # }
/// ```
pub trait AsyncWaitGroup: IsLocal {
    /// Adds `count` to the `WaitGroup` counter.
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::rc::Rc;
    /// use std::time::Duration;
    /// use orengine::{local_executor, sleep, Local};
    /// use orengine::sync::{AsyncWaitGroup, LocalWaitGroup};
    ///
    /// # async fn foo() {
    /// let wait_group = Rc::new(LocalWaitGroup::new());
    /// let number_executed_tasks = Rc::new(Local::new(0));
    ///
    /// wait_group.add(10);
    /// for i in 0..10 {
    ///     let wait_group = wait_group.clone();
    ///     let number_executed_tasks = number_executed_tasks.clone();
    ///
    ///     local_executor().spawn_local(async move {
    ///         sleep(Duration::from_millis(i)).await;
    ///         *number_executed_tasks.borrow_mut() += 1;
    ///         wait_group.done();
    ///     });
    /// }
    ///
    /// wait_group.wait().await; // wait until 10 tasks are completed
    /// assert_eq!(*number_executed_tasks.borrow(), 10);
    /// # }
    /// ```
    fn add(&self, count: usize);

    /// [`Adds`](Self::add) 1 to the `WaitGroup` counter.
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::rc::Rc;
    /// use std::time::Duration;
    /// use orengine::{local_executor, sleep, Local};
    /// use orengine::sync::{AsyncWaitGroup, LocalWaitGroup};
    ///
    /// # async fn foo() {
    /// let wait_group = Rc::new(LocalWaitGroup::new());
    /// let number_executed_tasks = Rc::new(Local::new(0));
    ///
    /// for i in 0..10 {
    ///     let wait_group = wait_group.clone();
    ///     let number_executed_tasks = number_executed_tasks.clone();
    ///
    ///     wait_group.inc();
    ///
    ///     local_executor().spawn_local(async move {
    ///         sleep(Duration::from_millis(i)).await;
    ///         *number_executed_tasks.borrow_mut() += 1;
    ///         wait_group.done();
    ///     });
    /// }
    ///
    /// wait_group.wait().await; // wait until all tasks are completed
    /// assert_eq!(*number_executed_tasks.borrow(), 10);
    /// # }
    /// ```
    #[inline]
    fn inc(&self) {
        self.add(1);
    }

    /// Returns the `WaitGroup` counter.
    ///
    /// Example
    ///
    /// ```rust
    /// use orengine::sync::{AsyncWaitGroup, LocalWaitGroup};
    ///
    /// # async fn foo() {
    /// let wait_group = LocalWaitGroup::new();
    /// assert_eq!(wait_group.count(), 0);
    /// wait_group.inc();
    /// assert_eq!(wait_group.count(), 1);
    /// wait_group.done();
    /// assert_eq!(wait_group.count(), 0);
    /// # }
    /// ```
    fn count(&self) -> usize;

    /// Decreases the `WaitGroup` counter by 1 and wakes up all tasks that are waiting
    /// if the counter reaches 0.
    ///
    /// Returns the current value of the counter.
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::rc::Rc;
    /// use orengine::local_executor;
    /// use orengine::sync::{AsyncWaitGroup, LocalWaitGroup};
    ///
    /// # async fn foo() {
    /// let wait_group = Rc::new(LocalWaitGroup::new());
    /// let wait_group_clone = wait_group.clone();
    ///
    /// wait_group.inc();
    ///
    /// local_executor().spawn_local(async move {
    ///     // wake up the waiting task, because a current and the only one task is done
    ///     let count = wait_group_clone.done();
    ///     assert_eq!(count, 0);
    /// });
    ///
    /// wait_group.wait().await; // wait until all tasks are completed
    /// # }
    /// ```
    fn done(&self) -> usize;

    /// Waits until the `WaitGroup` counter reaches 0.
    ///
    /// # Example
    ///
    /// ```rust
    /// use std::time::Duration;
    /// use std::rc::Rc;
    /// use orengine::{local_executor, sleep, Local};
    /// use orengine::sync::{AsyncWaitGroup, LocalWaitGroup};
    ///
    /// # async fn foo() {
    /// let wait_group = Rc::new(LocalWaitGroup::new());
    /// let number_executed_tasks = Rc::new(Local::new(0));
    ///
    /// for i in 0..10 {
    ///     let wait_group = wait_group.clone();
    ///     let number_executed_tasks = number_executed_tasks.clone();
    ///
    ///     wait_group.inc();
    ///
    ///     local_executor().spawn_local(async move {
    ///         sleep(Duration::from_millis(i)).await;
    ///         *number_executed_tasks.borrow_mut() += 1;
    ///         wait_group.done();
    ///     });
    /// }
    ///
    /// wait_group.wait().await; // wait until all tasks are completed
    /// assert_eq!(*number_executed_tasks.borrow(), 10);
    /// # }
    /// ```
    fn wait(&self) -> impl Future<Output = ()>;
}

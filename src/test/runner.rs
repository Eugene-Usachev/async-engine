//! This module provides a way to run tests by reusing
//! the same [`Executor`] via [`run_test_and_block_on_local`]
//! and [`run_test_and_block_on_shared`].
//!
//! # Example
//!
//! ```no_run
//! use std::time::Duration;
//! use orengine::test::run_test_and_block_on_local;
//!
//! async fn awesome_async_function() -> usize {
//!     42
//! }
//!
//! #[cfg(test)]
//! fn test_awesome_async_function() {
//!     run_test_and_block_on_local(async {
//!         assert_eq!(awesome_async_function().await, 42);
//!     }, Some(Duration::from_millis(1))); // Panics due to timeout after 1 ms
//! }
//! ```
//!
//! # Shortcuts
//!
//! You can use macro [`orengine::test::test_local`](crate::test::test_local) instead
//! of [`run_test_and_block_on_local`] and [`orengine::test::test_shared`](crate::test::test_shared)
//! instead of [`run_test_and_block_on_shared`]. Read [`run_test_and_block_on_local`] and
//! [`run_test_and_block_on_shared`] to find examples.
use crate::bug_message::BUG_MESSAGE;
use crate::runtime::executor::get_local_executor_ref;
use crate::runtime::{Config, Locality, Task};
use crate::{Executor, local_executor, stop_executor, yield_now};
use std::future::Future;
use std::panic::UnwindSafe;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;
use std::{panic, thread};

/// Prints the first test message. It contains information about build configuration.
fn print_first_test_message() {
    #[cfg(target_os = "linux")]
    {
        println!("OS: Linux, using io-uring");
    }

    #[cfg(not(target_os = "linux"))]
    {
        #[cfg(feature = "fallback_thread_pool")]
        {
            println!("OS: Not Linux, using fallback with thread pool");
        }

        #[cfg(not(feature = "fallback_thread_pool"))]
        {
            println!("OS: Not Linux, using fallback without thread pool");
        }
    }
}

/// Initializes the local executor only if it is not initialized
/// and returns `&'static mut Executor`.
pub(crate) fn get_local_executor() -> &'static mut Executor {
    static PRINTED: std::sync::Once = std::sync::Once::new();
    PRINTED.call_once(print_first_test_message);

    if get_local_executor_ref().is_none() {
        let cfg = Config::default().disable_work_sharing();

        Executor::init_with_config(cfg);
    }

    local_executor()
}

/// Upgrades provided future to release all previous tasks.
#[allow(clippy::future_not_send, reason = "This can be non-Send")]
pub(crate) async fn upgrade_future<Fut>(future: Fut)
where
    Fut: Future<Output = ()> + 'static,
{
    while local_executor().number_of_spawned_tasks() > 0 {
        yield_now().await;
    }

    future.await;
}

/// Upgrades provided future to release all previous tasks and close its executor after its executing.
#[allow(clippy::future_not_send, reason = "This can be non-Send")]
pub(crate) async fn upgrade_future_for_with_timeout<Fut>(future: Fut)
where
    Fut: Future<Output = ()> + 'static,
{
    upgrade_future(async move {
        future.await;

        stop_executor(local_executor().id());
    })
    .await;
}

/// An error that is returned by [`run_in_another_thread_and_wait_for_result_with_timeout`]
/// on timeout.
pub struct ErrTimeout;

impl std::fmt::Debug for ErrTimeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Test timed out")
    }
}

/// Runs a function in another thread and waits for its result with timeout.
///
/// # Panics
///
/// It panics when the provided function panics.
fn run_in_another_thread_and_wait_for_result_with_timeout<F>(
    f: F,
    timeout: Duration,
) -> Result<(), ErrTimeout>
where
    F: UnwindSafe + Send + 'static + FnOnce() -> Task,
{
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);

    thread::spawn(move || {
        let ex = get_local_executor();
        let task = f();

        ex.spawn_task(task);

        let res = panic::catch_unwind(move || {
            local_executor().run();
        });

        let _ = sender.send(res);
    });

    match receiver.recv_timeout(timeout) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(msg)) => panic::resume_unwind(msg),
        Err(RecvTimeoutError::Timeout) => panic!("Test timed out!"),
        _ => panic!("{BUG_MESSAGE}"),
    }
}

/// Initializes the local executor (if it is not initialized) and blocks the current
/// thread until the created `local` future is completed or the provided timeout is reached.
///
/// # The difference between `run_test_and_block_on_local` and [`run_test_and_block_on_shared`]
///
/// `run_test_and_block_on_local` creates a `local` task, while `run_test_and_block_on_shared`
/// creates a `shared` task.
///
/// Read more about `local` and `shared` tasks in [`Executor`].
///
/// # Example
///
/// ```no_run
/// use std::time::Duration;
/// use orengine::test::run_test_and_block_on_local;
///
/// async fn awesome_async_function() -> usize {
///     42
/// }
///
/// #[cfg(test)]
/// fn test_awesome_async_function() {
///     run_test_and_block_on_local(|| async {
///         assert_eq!(awesome_async_function().await, 42);
///     }, Some(Duration::from_millis(1))); // Panics due to timeout after 1 ms
/// }
/// ```
///
/// # Shortcut
///
/// You can use [`orengine::test::test_local`](crate::test::test_local).
/// The example below is equivalent to the one above:
///
/// ```no_run
/// async fn awesome_async_function() -> usize {
///     42
/// }
///
/// #[orengine::test::test_local]
/// fn test_awesome_async_function() {
///     assert_eq!(awesome_async_function().await, 42);
/// }
/// ```
///
/// # Panics
///
/// It panics if the spawned future is panic or if the timeout is reached.
pub fn run_test_and_block_on_local<Fut>(creator: fn() -> Fut, timeout: Option<Duration>)
where
    Fut: Future<Output = ()> + 'static,
{
    if let Some(timeout) = timeout {
        run_in_another_thread_and_wait_for_result_with_timeout(
            move || unsafe {
                Task::from_future(
                    upgrade_future_for_with_timeout(creator()),
                    Locality::local(),
                )
            },
            timeout,
        )
        .unwrap();
    } else {
        get_local_executor()
            .run_and_block_on_local(async move {
                upgrade_future(creator()).await;
            })
            .expect(BUG_MESSAGE);
    }
}

/// Initializes the local executor (if it is not initialized) and blocks the current
/// thread until the created `shared` future is completed or the provided timeout is reached.
///
/// # The difference between `run_test_and_block_on_shared` and [`run_test_and_block_on_local`]
///
/// `run_test_and_block_on_shared` creates a `shared` task, while `run_test_and_block_on_local`
/// creates a `local` task.
///
/// Read more about `local` and `shared` tasks in [`Executor`].
///
/// # Example
///
/// ```no_run
/// use std::time::Duration;
/// use orengine::test::run_test_and_block_on_shared;
/// # async fn get_some_result_from_shared_state() -> Result<(), ()> { Ok(()) }
///
/// async fn awesome_async_shared_function() -> usize {
///     if get_some_result_from_shared_state().await.is_err() {
///         return 0;
///     }
///
///     3
/// }
///
/// #[cfg(test)]
/// fn test_awesome_async_function() {
///     run_test_and_block_on_shared(|| async {
///         assert_eq!(awesome_async_shared_function().await, 3);
///     }, Some(Duration::from_millis(1))); // Panics due to timeout after 1 ms
/// }
/// ```
///
/// # Shortcut
///
/// You can use [`orengine::test::test_shared`](crate::test::test_shared).
/// An example below is equivalent to the one above:
///
/// ```no_run
/// # async fn get_some_result_from_shared_state() -> Result<(), ()> { Ok(()) }
///
/// async fn awesome_async_shared_function() -> usize {
///     if get_some_result_from_shared_state().await.is_err() {
///         return 0;
///     }
///
///     3
/// }
///
/// #[orengine::test::test_shared]
/// fn test_awesome_async_function() {
///     assert_eq!(awesome_async_shared_function().await, 3);
/// }
/// ```
#[allow(
    clippy::missing_panics_doc,
    reason = "Panics on when a bug is occurred"
)]
pub fn run_test_and_block_on_shared<Fut>(creator: fn() -> Fut, timeout: Option<Duration>)
where
    Fut: Future<Output = ()> + Send + 'static,
{
    if let Some(timeout) = timeout {
        run_in_another_thread_and_wait_for_result_with_timeout(
            move || unsafe {
                Task::from_future(
                    upgrade_future_for_with_timeout(creator()),
                    Locality::shared(),
                )
            },
            timeout,
        )
        .unwrap();
    } else {
        get_local_executor()
            .run_and_block_on_shared(async move {
                upgrade_future(creator()).await;
            })
            .expect(BUG_MESSAGE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as orengine;
    use crate::sleep;
    use std::thread;

    async fn awesome_fn() {}

    fn awesome_fn_that_sync_time_out() {
        thread::sleep(Duration::from_secs(10));
    }

    async fn awesome_fn_that_async_time_out() {
        sleep(Duration::from_secs(10)).await;
    }

    #[test]
    fn test_test_runner_not_timeout() {
        run_test_and_block_on_shared(awesome_fn, None);
        run_test_and_block_on_shared(awesome_fn, Some(Duration::from_secs(1)));

        run_test_and_block_on_local(awesome_fn, None);
        run_test_and_block_on_local(awesome_fn, Some(Duration::from_secs(1)));
    }

    #[test]
    #[should_panic = "Test timed out"]
    fn test_test_runner_sync_timeout() {
        run_test_and_block_on_local(
            || async {
                awesome_fn_that_sync_time_out();
            },
            Some(Duration::from_millis(1)),
        );
    }

    #[test]
    #[should_panic = "Test timed out"]
    fn test_test_runner_async_timeout() {
        run_test_and_block_on_local(
            awesome_fn_that_async_time_out,
            Some(Duration::from_millis(1)),
        );
    }

    #[orengine::test::test_local(timeout_ms = 3000)]
    fn test_test_macro_not_timeout() {}

    #[orengine::test::test_local(timeout_ms = 500)]
    #[should_panic = "Test timed out"]
    fn test_test_macro_sync_timeout() {
        awesome_fn_that_sync_time_out();
    }

    #[orengine::test::test_local(timeout_ms = 500)]
    #[should_panic = "Test timed out"]
    fn test_test_macro_async_timeout() {
        awesome_fn_that_async_time_out().await;
    }
}

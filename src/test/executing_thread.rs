// TODO
// TODO update whole docs about handles
//! This module contains utilities for parallel testing via
//! [`sched_future_to_another_thread`] or [`sched_future`](ExecutorPool::sched_future).

use crate::bug_message::BUG_MESSAGE;
use crate::runtime::Config;
use crate::test::job::Job;
use crate::test::{
    ExecutorPoolJoinHandle, MainTestEndpointHandlers,
    add_new_executor_to_main_test_endpoint_handlers, clone_main_test_endpoint_handlers,
    deregister_local_main_fn_endpoint_handlers, main_test_endpoint_handlers_was_panicked,
    mark_handle_ready, number_of_main_test_endpoint_handlers, register_main_test_endpoint_handlers,
    set_main_test_endpoint_handlers_was_panicked,
};
use crate::{Executor, local_executor, sleep};
use std::panic::{AssertUnwindSafe, UnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::{Acquire, Release};
use std::sync::mpsc::sync_channel;
use std::thread;
use std::time::Duration;

pub(crate) struct ExecutingThread {}

const CONFIG: Config = Config::default().disable_work_sharing();

impl ExecutingThread {
    fn create_job_and_handle<Fut: Future<Output = ()> + 'static>(
        future: Fut,
        is_local: bool,
    ) -> ExecutorPoolJoinHandle {
        let (result_sender, result_receiver) = sync_channel(1);

        let mut job = Job::new(
            future,
            is_local,
            result_sender,
            clone_main_test_endpoint_handlers(),
        );

        thread::Builder::new()
            .name("orengine-test-executor".to_string())
            .spawn(move || {
                fn should_wait() -> bool {
                    !main_test_endpoint_handlers_was_panicked()
                        && number_of_main_test_endpoint_handlers() > 0
                }

                let result_sender = job.result_sender.take().unwrap();

                register_main_test_endpoint_handlers(
                    job.main_test_endpoint_handlers.take().unwrap(),
                );

                let was_marked = Arc::new(AtomicBool::new(false));
                let was_marked_clone = was_marked.clone();

                let res = catch_unwind(AssertUnwindSafe(move || {
                    Executor::init_with_config(CONFIG)
                        .run_and_block_on_local(async move {
                            let was_ready = Arc::new(AtomicBool::new(false));
                            let was_ready_clone = was_ready.clone();
                            let is_job_local = job.is_local;
                            let future = async move {
                                job.await;

                                was_ready_clone.store(true, Release);
                            };

                            if is_job_local {
                                local_executor().exec_local_future(future);
                            } else {
                                local_executor().exec_shared_future(future);
                            }

                            let mut dur = Duration::from_millis(1);

                            while !was_ready.load(Acquire) {
                                sleep(dur).await;

                                if dur < Duration::from_millis(64) {
                                    dur *= 2;
                                }
                            }

                            was_marked_clone.store(true, Release);

                            mark_handle_ready();

                            while should_wait() {
                                sleep(dur).await;

                                if dur < Duration::from_millis(64) {
                                    dur *= 2;
                                }
                            }
                        })
                        .expect(BUG_MESSAGE);
                }));

                if !was_marked.load(Acquire) {
                    mark_handle_ready();
                }

                match res {
                    Ok(()) => {
                        let _ = result_sender.try_send(Ok(())); // Error means that the main fn has panicked
                    }
                    Err(panic_msg) => unsafe {
                        set_main_test_endpoint_handlers_was_panicked();

                        local_executor().graceful_stop();

                        result_sender
                            .try_send(Err(Box::new(panic_msg)))
                            .expect(BUG_MESSAGE);
                    },
                }

                deregister_local_main_fn_endpoint_handlers();
            })
            .expect("Spawned for test executor panicked, but it should not happen. It is a bug.");

        ExecutorPoolJoinHandle::new(result_receiver)
    }

    // TODO docs
    pub(crate) fn sched_future_for_main_fn<Fut: Future<Output = ()> + UnwindSafe + 'static>(
        future: Fut,
        is_local: bool,
        handles: MainTestEndpointHandlers,
    ) -> ExecutorPoolJoinHandle {
        register_main_test_endpoint_handlers(handles);

        Self::create_job_and_handle(future, is_local)
    }
}

/// Schedules a future to any free executor in the [`pool`](ExecutorPool).
///
/// It is used to test parallelism.
///
/// # Example
///
/// ```rust
/// use std::sync::Arc;
/// use std::sync::atomic::AtomicUsize;
/// use std::sync::atomic::Ordering::SeqCst;
/// use std::time::Duration;
/// use orengine::test::{run_test_and_block_on_shared, ExecutorPool};
/// use orengine::yield_now;
///
/// async fn awesome_function(atomic_to_sync_test: Arc<AtomicUsize>) {
///     atomic_to_sync_test.fetch_add(1, SeqCst);
///     yield_now().await;
///     atomic_to_sync_test.fetch_add(1, SeqCst);
/// }
///
/// #[cfg(test)]
/// fn test_awesome_function() {
///     run_test_and_block_on_shared(async {
///         let atomic_to_sync_test = Arc::new(AtomicUsize::new(0));
///         let mut handles = Vec::with_capacity(10);
///
///         for _ in 0..10 {
///             let join = ExecutorPool::sched_future(
///                 awesome_function(atomic_to_sync_test.clone())
///             ).await;
///
///             handles.push(join);
///         }
///
///         for handle in handles {
///             handle.join().await;
///         }
///
///         assert_eq!(atomic_to_sync_test.load(SeqCst), 20);
///     }, Some(Duration::from_millis(1))); // Times out after 1 ms
/// }
/// ```
#[allow(
    clippy::missing_panics_doc,
    reason = "It panics only when a bug is occurred"
)]
pub fn sched_future<Fut>(future: Fut)
where
    Fut: Future<Output = ()> + Send + 'static,
{
    let handle = ExecutingThread::create_job_and_handle(future, false);

    add_new_executor_to_main_test_endpoint_handlers(handle);
}

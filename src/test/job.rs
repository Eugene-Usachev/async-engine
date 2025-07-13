//! This module contains the [`Job`] struct and items for working with jobs.

use crate::test::MainTestEndpointHandlers;
use std::pin::Pin;
use std::task::Poll;
use std::thread;

pub(crate) type SyncSender<T> = std::sync::mpsc::SyncSender<T>;
pub(crate) type SyncReceiver<T> = std::sync::mpsc::Receiver<T>;
pub(crate) type JobResult = thread::Result<()>;

/// `Job` is a wrapper for a [`Future`] via polling it and sending the result
/// (caught panic) to the `result_sender`.
///
/// It also contains a `sender` to acquired [`Executor`] that will be released after
/// the task is done.
pub(crate) struct Job {
    pub(crate) future: Box<dyn Future<Output = ()>>,
    pub(crate) is_local: bool,
    pub(crate) main_test_endpoint_handlers: Option<MainTestEndpointHandlers>,
    pub(crate) result_sender: Option<SyncSender<JobResult>>,
}

impl Job {
    /// Creates a new `Job` instance. Read [`Job`] for more information.
    pub(crate) fn new<Fut: Future<Output = ()> + 'static>(
        future: Fut,
        is_local: bool,
        result_sender: SyncSender<JobResult>,
        main_test_endpoint_handlers: Option<MainTestEndpointHandlers>,
    ) -> Self {
        Self {
            future: Box::new(future),
            is_local,
            result_sender: Some(result_sender),
            main_test_endpoint_handlers,
        }
    }
}

impl Future for Job {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        // TODO
        // fn return_result(this: &mut Job) -> Poll<<Job as Future>::Output> {
        //     let job_sender = this.sender.take().expect(BUG_MESSAGE);
        //
        //     EXECUTOR_POOL.senders_to_executors.push(job_sender);
        //
        //     Poll::Ready(())
        // }

        // TODO
        // fn wait_if_there_is_work(this: &mut Job, cx: &mut std::task::Context) -> Poll<<Job as Future>::Output> {
        //     let are_all_tasks_handles = this
        //         .main_test_endpoint_handlers
        //         .as_ref()
        //         .is_some_and(|handles| {
        //             const NUMBER_OF_BACKGROUND_TASKS: usize = 1;
        //
        //             let number_of_handles = handles.lock().expect(BUG_MESSAGE).len();
        //
        //             // TODO r
        //             println!("Thread: {:?}, Number of handles: {}, number of spawned tasks: {}",
        //                      thread::current().id(), number_of_handles, local_executor().number_of_spawned_tasks());
        //
        //             number_of_handles + NUMBER_OF_BACKGROUND_TASKS == local_executor().number_of_spawned_tasks()
        //         });
        //
        //     if local_executor().has_work_except_spawned_tasks() || !are_all_tasks_handles {
        //         let task = unsafe { Task::from_context(cx) };
        //         if task.is_local() {
        //             local_executor().spawn_task_at_end_of_local_tasks_queue(task);
        //         } else {
        //             local_executor().spawn_task_at_end_of_shared_tasks_queue(task);
        //         }
        //
        //         return Poll::Pending;
        //     }
        //
        //     return_result(this)
        // }

        let this = &mut *self;

        // TODO
        // if this.main_completed {
        //     return wait_if_there_is_work(this, cx);
        // }

        let pinned_future = unsafe { Pin::new_unchecked(&mut *this.future) };

        if pinned_future.poll(cx).is_ready() {
            //TODO this.main_completed = true;

            //TODO return wait_if_there_is_work(this, cx);

            return Poll::Ready(());
        }

        Poll::Pending
    }
}

#[allow(
    clippy::non_send_fields_in_send_ty,
    reason = "We guarantee that `Job` is `Send`"
)]
unsafe impl Send for Job {}

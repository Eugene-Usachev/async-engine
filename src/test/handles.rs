use crate::bug_message::BUG_MESSAGE;
use crate::test::job::{JobResult, SyncReceiver};
// TODO docs
use std::any::Any;
use std::cell::RefCell;
use std::panic;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Release};
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Inner {
    /// Starts from 1 because the main thread is always considered as a pending handle.
    pending_len: AtomicUsize,
    /// `pending` is a _lazy_ list of pending handles. The real number of pending handles
    /// is stored in `pending_len`.
    pending: Mutex<Vec<ExecutorPoolJoinHandle>>,
    was_panicked: AtomicBool,
}

#[derive(Clone)]
pub(crate) struct MainTestEndpointHandlers(Arc<Inner>);

impl MainTestEndpointHandlers {
    pub(crate) fn new() -> Self {
        Self(Arc::new(Inner {
            pending_len: AtomicUsize::new(1),
            pending: Mutex::new(Vec::new()),
            was_panicked: AtomicBool::new(false),
        }))
    }

    fn with_pending(&self, func: impl FnOnce(&mut Vec<ExecutorPoolJoinHandle>)) {
        func(&mut self.0.pending.lock().expect(BUG_MESSAGE));
    }

    pub(crate) fn set_was_panicked(&self) {
        self.0.was_panicked.store(true, Release);
    }

    pub(crate) fn was_panicked(&self) -> bool {
        self.0.was_panicked.load(Acquire)
    }
}

/// `ExecutorPoolJoinHandle` is used to wait for the task sent to the [`ExecutorPool`]
/// to complete. It can be gotten by [`sched_future()`].
///
/// If you don't need to wait,
/// use [`sched_future_to_another_thread`].
///
/// # Panic
///
/// If not [`joined`](ExecutorPoolJoinHandle::join) before dropping.
pub(crate) struct ExecutorPoolJoinHandle {
    was_joined: bool,
    sync_receiver: SyncReceiver<JobResult>,
}

impl ExecutorPoolJoinHandle {
    /// Creates a new `ExecutorPoolJoinHandle` instance.
    pub(crate) fn new(sync_receiver: SyncReceiver<JobResult>) -> Self {
        Self {
            was_joined: false,
            sync_receiver,
        }
    }

    /// Waits for the task sent to the [`ExecutorPool`] to complete.
    ///
    /// It blocks the current thread until the task is done or it the task timeout.
    /// If `None` is provided, it waits forever.
    ///
    /// It also calls `before_panic_func` before throwing the panic.
    ///
    /// # Panics
    ///
    /// If test fn was panicked. It is used to `should_panic`.
    pub(crate) fn join_timeout(
        &mut self,
        timeout: Option<Duration>,
    ) -> Result<JobResult, RecvTimeoutError> {
        assert!(!self.was_joined);

        let res = if let Some(timeout) = timeout {
            match self.sync_receiver.recv_timeout(timeout) {
                Ok(res) => res,
                Err(RecvTimeoutError::Timeout) => return Err(RecvTimeoutError::Timeout),
                _ => panic!("{BUG_MESSAGE}"),
            }
        } else {
            self.sync_receiver.recv().expect(BUG_MESSAGE)
        };

        self.was_joined = true;

        Ok(res)
    }

    // TODO r
    // /// Waits for the task sent to the [`ExecutorPool`] to complete.
    // ///
    // /// It blocks the current thread until the task is done or it the task timeout.
    // ///
    // /// # Panics
    // ///
    // /// If test fn was panicked. It is used to `should_panic`.
    // pub(crate) fn join_timeout(self, timeout: Duration) {
    //     self.join_timeout_(Some((timeout, "The scheduled task timed out!")));
    // }

    // TODO docs
    fn try_join(&mut self) -> Option<JobResult> {
        assert!(!self.was_joined);

        if let Ok(res) = self.sync_receiver.try_recv() {
            self.was_joined = true;

            Some(res)
        } else {
            None
        }
    }
}

unsafe impl Send for ExecutorPoolJoinHandle {}

thread_local! {
    static MAIN_TEST_ENDOINT_HANDLERS: RefCell<Option<MainTestEndpointHandlers>> = const { RefCell::new(None) };
}

pub(crate) fn register_main_test_endpoint_handlers(handlers: MainTestEndpointHandlers) {
    MAIN_TEST_ENDOINT_HANDLERS.with(|handlers_| {
        *handlers_.borrow_mut() = Some(handlers);
    });
}

pub(crate) fn add_new_executor_to_main_test_endpoint_handlers(handler: ExecutorPoolJoinHandle) {
    MAIN_TEST_ENDOINT_HANDLERS.with(|handlers_| {
        if let Some(handlers) = handlers_.borrow_mut().as_ref() {
            handlers.0.pending_len.fetch_add(1, Release);

            handlers.with_pending(move |pending| {
                pending.push(handler);
            });
        }
    });
}

pub(crate) fn number_of_main_test_endpoint_handlers() -> usize {
    MAIN_TEST_ENDOINT_HANDLERS.with(|handlers| {
        handlers
            .borrow()
            .as_ref()
            .map(|handlers| handlers.0.pending_len.load(Acquire))
            .unwrap()
    })
}

pub(crate) fn mark_handle_ready() {
    MAIN_TEST_ENDOINT_HANDLERS.with(|handlers| {
        assert!(
            handlers
                .borrow()
                .as_ref()
                .unwrap()
                .0
                .pending_len
                .fetch_sub(1, AcqRel)
                > 0,
            "[BUG] decrementing number of pending handles with overflow"
        );
    });
}

pub(crate) fn clone_main_test_endpoint_handlers() -> Option<MainTestEndpointHandlers> {
    MAIN_TEST_ENDOINT_HANDLERS.with(|handlers| handlers.borrow().as_ref().map(Clone::clone))
}

/// Check if at least one of the main test endpoint handlers was panicked.
///
/// Returns number of pending handlers on success.
pub(crate) fn check_main_test_endpoint_handlers() -> Result<usize, Box<dyn Any + Send>> {
    MAIN_TEST_ENDOINT_HANDLERS.with(|handlers| {
        let mut maybe_first_panic = None;
        let mut number_of_pending_handlers = 0;

        handlers
            .borrow()
            .as_ref()
            .expect("[BUG] main test endpoint handlers are not registered")
            .with_pending(|pending| {
                pending.retain_mut(|handlers| {
                    if let Some(res) = handlers.try_join() {
                        if let Err(err) = res {
                            maybe_first_panic = Some(err);
                        }

                        return false;
                    }

                    true
                });

                number_of_pending_handlers = pending.len();
            });

        if let Some(err) = maybe_first_panic {
            assert!(main_test_endpoint_handlers_was_panicked());

            return Err(err);
        }

        Ok(number_of_pending_handlers)
    })
}

pub(crate) fn deregister_local_main_fn_endpoint_handlers() {
    MAIN_TEST_ENDOINT_HANDLERS.with(|handlers| {
        assert!(handlers.borrow().is_some());

        *handlers.borrow_mut() = None;
    });
}

pub(crate) fn set_main_test_endpoint_handlers_was_panicked() {
    MAIN_TEST_ENDOINT_HANDLERS.with(|handlers| {
        handlers
            .borrow()
            .as_ref()
            .expect("[BUG] main test endpoint handlers are not registered")
            .set_was_panicked();
    });
}

pub(crate) fn main_test_endpoint_handlers_was_panicked() -> bool {
    MAIN_TEST_ENDOINT_HANDLERS.with(|handlers| {
        handlers
            .borrow()
            .as_ref()
            .expect("[BUG] main test endpoint handlers are not registered")
            .was_panicked()
    })
}

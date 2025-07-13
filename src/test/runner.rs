// TODO docs

use crate::runtime::Locality;
use crate::test::{ExecutingThread, MainTestEndpointHandlers, check_main_test_endpoint_handlers};
use ahash::{HashMap, HashMapExt};
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, LazyLock, Once};
use std::time::{Duration, Instant};
use std::{mem, panic, thread};

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

static PRINT_ONCE: Once = Once::new();

// TODO docs
fn run_test_and_block_on<Fut>(
    creator: fn() -> Fut,
    timeout: Option<Duration>,
    locality: Locality,
    exclusive_in: Option<String>,
) where
    Fut: Future<Output = ()> + 'static,
{
    type SyncMutex<T> = std::sync::Mutex<T>;
    type SyncRWLock<T> = std::sync::RwLock<T>;

    static FULL_LOCK: SyncRWLock<()> = SyncRWLock::new(());
    static LOCKS: LazyLock<SyncMutex<HashMap<String, Arc<SyncMutex<()>>>>> =
        LazyLock::new(|| SyncMutex::new(HashMap::new()));

    #[allow(unused, reason = "It is needed to drop")]
    enum FullLockGuard<'a> {
        Read(std::sync::RwLockReadGuard<'a, ()>),
        Write(std::sync::RwLockWriteGuard<'a, ()>),
    }

    PRINT_ONCE.call_once(print_first_test_message);

    let locks = exclusive_in.map_or_else(
        || (FullLockGuard::Read(FULL_LOCK.read().unwrap()), None),
        |exclusive_in| {
            if exclusive_in == "*" {
                (FullLockGuard::Write(FULL_LOCK.write().unwrap()), None)
            } else {
                let mut locks = LOCKS.lock().unwrap();
                let part_lock = locks
                    .entry(exclusive_in)
                    .or_insert_with(|| Arc::new(SyncMutex::new(())));
                let static_part_lock = unsafe {
                    mem::transmute::<
                        &std::sync::Mutex<()>,
                        &'static std::sync::Mutex<()>, // It never drops, but it needs to be Arc (to implement Send)
                    >(Arc::as_ref(part_lock))
                };

                (
                    FullLockGuard::Read(FULL_LOCK.read().unwrap()),
                    Some(static_part_lock.lock().unwrap()),
                )
            }
        },
    );

    let future = creator();
    let handlers = MainTestEndpointHandlers::new();

    let mut handle = ExecutingThread::sched_future_for_main_fn(
        AssertUnwindSafe(future),
        locality.is_local(),
        handlers,
    );
    let mut was_main_fn_ready = false;
    let limit = timeout.map_or(Duration::from_secs(100_000), |t| {
        t.max(Duration::from_millis(1000))
    });
    let deadline = Instant::now() + limit;
    let mut sleep_time = Duration::from_micros(100);
    let mut number_of_handlers_ = 0;

    while Instant::now() < deadline {
        if !was_main_fn_ready {
            if let Ok(job_result) = handle.join_timeout(Some(sleep_time)) {
                if let Err(err) = job_result {
                    drop(locks);

                    panic::resume_unwind(err);
                }

                was_main_fn_ready = true;
            }
        }

        thread::sleep(sleep_time);

        sleep_time = (sleep_time * 3 / 2).min(Duration::from_millis(100));

        // Maybe another handler has panicked.
        // Then we probably can't wait for the main fn and need to throw the panic.
        match check_main_test_endpoint_handlers() {
            Ok(number_of_handlers) => {
                number_of_handlers_ = number_of_handlers;

                if number_of_handlers == 0 && was_main_fn_ready {
                    drop(locks);

                    return;
                }
            }
            Err(panic_msg) => {
                drop(locks);

                panic::resume_unwind(panic_msg);
            }
        }
    }

    drop(locks);

    let helpful_msg = if was_main_fn_ready {
        format!("main test endpoint is ready but {number_of_handlers_} handlers are still running")
    } else if number_of_handlers_ == 0 {
        "main test endpoint is not ready".to_string()
    } else {
        format!(
            "main test endpoint is not ready and {number_of_handlers_} handlers are still running"
        )
    };

    panic!("Test timed out: {helpful_msg}");
}

// TODO docs
pub fn run_test_and_block_on_local<Fut>(
    creator: fn() -> Fut,
    timeout: Option<Duration>,
    exclusive_in: Option<String>,
) where
    Fut: Future<Output = ()> + 'static,
{
    run_test_and_block_on(creator, timeout, Locality::local(), exclusive_in);
}

// TODO docs
pub fn run_test_and_block_on_shared<Fut>(
    creator: fn() -> Fut,
    timeout: Option<Duration>,
    exclusive_in: Option<String>,
) where
    Fut: Future<Output = ()> + 'static,
{
    run_test_and_block_on(creator, timeout, Locality::shared(), exclusive_in);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as orengine;
    use crate::sleep;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering::SeqCst;
    use std::thread;

    async fn awesome_fn() {}

    fn awesome_fn_that_sync_time_out() {
        thread::sleep(Duration::from_secs(10));
    }

    async fn awesome_fn_that_async_time_out() {
        sleep(Duration::from_secs(10)).await;
    }

    #[test]
    fn test_test_runner_local_not_timeout() {
        run_test_and_block_on_local(awesome_fn, None, None);
        run_test_and_block_on_local(awesome_fn, Some(Duration::from_secs(1)), None);
    }

    #[test]
    fn test_test_runner_shared_not_timeout() {
        run_test_and_block_on_shared(awesome_fn, None, None);
        run_test_and_block_on_shared(awesome_fn, Some(Duration::from_secs(1)), None);
    }

    #[test]
    #[should_panic = "Test timed out"]
    fn test_test_runner_sync_timeout() {
        run_test_and_block_on_local(
            || async {
                awesome_fn_that_sync_time_out();
            },
            Some(Duration::from_millis(1)),
            None,
        );
    }

    #[test]
    #[should_panic = "Test timed out: main test endpoint is not ready"]
    fn test_test_runner_async_timeout() {
        run_test_and_block_on_local(
            awesome_fn_that_async_time_out,
            Some(Duration::from_millis(1)),
            None,
        );
    }

    #[orengine::test::test_local(timeout_ms = 3000)]
    fn test_test_macro_not_timeout() {}

    #[orengine::test::test_local(timeout_ms = 500)]
    #[should_panic = "Test timed out: main test endpoint is not ready"]
    fn test_test_macro_sync_timeout() {
        awesome_fn_that_sync_time_out();
    }

    #[orengine::test::test_local(timeout_ms = 500)]
    #[should_panic = "Test timed out: main test endpoint is not ready"]
    fn test_test_macro_async_timeout() {
        awesome_fn_that_async_time_out().await;
    }

    #[test]
    fn test_test_full_exclusive() {
        static IS_RUNNING: AtomicBool = AtomicBool::new(false);

        thread::spawn(|| {
            run_test_and_block_on_local(
                || async move {
                    assert!(!IS_RUNNING.swap(true, SeqCst));

                    sleep(Duration::from_millis(100)).await;

                    IS_RUNNING.store(false, SeqCst);
                },
                None,
                Some("*".to_string()),
            );
        });

        thread::sleep(Duration::from_millis(10));

        run_test_and_block_on_local(
            || async move {
                assert!(!IS_RUNNING.load(SeqCst));
            },
            None,
            Some("test_test_full_exclusive".to_string()),
        );
    }

    #[test]
    fn test_test_part_exclusive() {
        static IS_RUNNING: AtomicBool = AtomicBool::new(false);

        thread::spawn(|| {
            run_test_and_block_on_local(
                || async move {
                    assert!(!IS_RUNNING.swap(true, SeqCst));

                    sleep(Duration::from_millis(100)).await;

                    IS_RUNNING.store(false, SeqCst);
                },
                None,
                Some("test_test_part_exclusive".to_string()),
            );
        });

        thread::sleep(Duration::from_millis(10));

        run_test_and_block_on_local(
            || async move {
                assert!(!IS_RUNNING.load(SeqCst));
            },
            None,
            Some("test_test_part_exclusive".to_string()),
        );
    }
}

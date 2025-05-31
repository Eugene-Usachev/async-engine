use crate::runtime::{Task, local_executor};
use std::time::Duration;

/// Sleeps at least until `Instant::now() + duration`. It works only in `orengine` runtime.
///
/// # Example
///
/// ```no_run
/// use orengine::sleep;
/// use std::time::Duration;
///
/// orengine::Executor::init().run_with_local_future(async {
///     sleep(Duration::from_millis(100)).await;
///
///     println!("Hello after at least 100 millis!");
/// });
/// ```
#[inline]
pub async fn sleep(duration: Duration) {
    let task = unsafe { Task::get_current().await };

    local_executor().register_sleeping_task(
        task,
        local_executor().start_round_time_for_deadlines() + duration,
    );

    unsafe { Task::park_current_task().await }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as orengine;
    use crate::local::Local;
    use crate::yield_now;
    use std::time::Duration;

    #[orengine::test::test_local]
    fn test_sleep() {
        #[allow(clippy::future_not_send, reason = "It is `local`.")]
        async fn sleep_for(dur: Duration, number: u16, arr: Local<Vec<u16>>) {
            sleep(dur).await;
            arr.borrow_mut().push(number);
        }

        let arr = Local::new(Vec::new());
        let ex = local_executor();

        yield_now().await; // release exec_series

        ex.exec_local_future(sleep_for(Duration::from_millis(1), 1, arr.clone()));
        ex.exec_local_future(sleep_for(Duration::from_millis(2), 2, arr.clone()));
        ex.exec_local_future(sleep_for(Duration::from_millis(3), 3, arr.clone()));
        ex.exec_local_future(sleep_for(Duration::from_millis(4), 4, arr.clone()));

        sleep(Duration::from_millis(5)).await;

        assert_eq!(arr.borrow().len(), 4);
    }
}

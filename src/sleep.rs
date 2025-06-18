//! This module provides the [`sleep`] and [`sleep_until`] functions.
use crate::runtime::{local_executor, Task};
use crate::utils::OrengineInstant;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

#[repr(C)]
struct Sleep {
    deadline: OrengineInstant,
    #[cfg(not(unix))]
    was_called: bool,
}

impl Future for Sleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;

        #[cfg(unix)]
        let was_called = this.deadline.into_u64() == 0;

        #[cfg(not(unix))]
        let was_called = this.was_called;

        if was_called {
            Poll::Ready(())
        } else {
            local_executor()
                .register_sleeping_task(unsafe { Task::from_context(cx) }, this.deadline);

            #[cfg(unix)]
            {
                this.deadline = OrengineInstant::from_u64(0);
            }

            #[cfg(not(unix))]
            {
                this.was_called = true;
            }

            Poll::Pending
        }
    }
}

/// Sleeps for a given duration or more. It works only in `orengine` runtime.
///
/// # Accuracy
///
/// This function uses [`start_round_time_for_deadlines`] to get the current time.
/// It means that very likely the current task will be woken after the duration, but
/// it's not guaranteed.
/// It also means that often the task will be woken with a 0-100 microseconds delay.
/// If you need more accuracy, use [`sleep_until`] instead.
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
///
/// [`start_round_time_for_deadlines`]: crate::Executor::start_round_time_for_deadlines
#[inline]
pub fn sleep(duration: Duration) -> impl Future<Output = ()> {
    Sleep {
        deadline: local_executor().start_round_time_for_deadlines() + duration,
        #[cfg(not(unix))]
        was_called: false,
    }
}

/// Sleeps until a given instant or more. It works only in `orengine` runtime.
///
/// # Example
///
/// ```no_run
/// use orengine::{local_executor, sleep_until};
/// use std::time::Duration;
///
/// orengine::Executor::init().run_with_local_future(async {
///     let instant = local_executor().start_round_time_for_deadlines() + Duration::from_millis(100);
///
///     sleep_until(instant).await;
///
///     println!("Hello after at least 100 millis!");
/// });
/// ```
#[inline]
pub fn sleep_until(instant: impl Into<OrengineInstant>) -> impl Future<Output = ()> {
    Sleep {
        deadline: instant.into(),
        #[cfg(not(unix))]
        was_called: false,
    }
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

        let start = OrengineInstant::now();

        sleep_until(start + Duration::from_millis(5)).await;

        assert!(OrengineInstant::now().duration_since(start) >= Duration::from_micros(5000));

        assert_eq!(arr.borrow().as_slice(), [1, 2, 3, 4]);
    }
}

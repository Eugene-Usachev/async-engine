//! This module contains functions that can be used to make progress in the async runtime.
//!
//! Read [`short_preempt`] and [`long_preempt`] for more information.
use crate::local_executor;

/// If `feature = "disable_preemption"` is disabled, it tries to execute the
/// next [`task`](crate::runtime::Task) of the current [`executor`](Executor).
///
/// If `feature = "disable_preemption"` is enabled, it spins.
///
/// In the async runtime you should use this function instead of [`std::hint::spin_loop`] not to
/// waste the CPU time.
///
/// It can be used in cases when you can't use `.await`
/// (for example, to implement waiting in lock-free algorithms).
/// But if you can use [`yield_now`](crate::yield_now), it is better to use it.
///
/// # Warning
///
/// This method can be used only in Orengine's async runtime.
/// Calling it out of the runtime is an undefined behavior (or panic with `debug_assertions`).
#[inline(always)]
pub fn short_preempt() {
    #[cfg(feature = "disable_preemption")]
    {
        std::hint::spin_loop();
    }

    #[cfg(not(feature = "disable_preemption"))]
    {
        local_executor().exec_next_task_or_spin();
    }
}

/// If `feature = "disable_preemption"` is disabled, it tries to make some progress with pollers.
///
/// If `feature = "disable_preemption"` is enabled, it calls [`std::thread::yield_now`].
///
/// In the async runtime, you should use this function instead of [`std::thread::yield_now`] not to
/// waste the timeslice.
///
/// It can be used in cases when you can't use `.await`
/// (for example, to implement waiting in lock-free algorithms).
/// But if you can use [`yield_now`](crate::yield_now), it is better to use it.
///
/// # Warning
///
/// This method can be used only in Orengine's async runtime.
/// Calling it out of the runtime is an undefined behavior (or panic with `debug_assertions`).
#[inline(always)]
pub fn long_preempt() {
    #[cfg(feature = "disable_preemption")]
    {
        std::thread::yield_now();
    }

    #[cfg(not(feature = "disable_preemption"))]
    {
        local_executor().next_poll();
    }
}

#[cfg(all(test, not(feature = "disable_preemption")))]
mod tests {
    use crate as orengine;
    use crate::utils::{short_preempt, Backoff};
    use crate::{local_executor, sleep, Local};
    use std::time::Duration;

    #[orengine::test_local(timeout_ms = 1000)]
    fn test_short_preempt() {
        // The idea: we have a task that can't make progress before another task and never calls `yield_now`.
        // So, if `short_preempt` doesn't work, the test will fail with timeout.

        let signal = Local::new(false);
        let signal_clone = signal.clone();

        local_executor().spawn_local(async move {
            println!("test_short_preempt: 2");

            *signal_clone.borrow_mut() = true;
        });

        println!("test_short_preempt: 1");

        while !*signal.borrow() {
            short_preempt();
        }

        println!("test_short_preempt: 3");
    }

    #[orengine::test_local(timeout_ms = 1000)]
    fn test_long_preempt() {
        // The idea: we have a task that can't make progress before another task and never calls `yield_now`.
        // So, if `long_preempt` (called by `Backoff`) doesn't work, the test will fail with timeout.

        let signal = Local::new(false);
        let signal_clone = signal.clone();

        local_executor().spawn_local(async move {
            sleep(Duration::from_micros(100)).await;

            println!("test_long_preempt: 2");

            *signal_clone.borrow_mut() = true;
        });

        println!("test_long_preempt: 1");

        let backoff = Backoff::new();

        while !*signal.borrow() {
            backoff.snooze();
        }

        println!("test_long_preempt: 3");
    }
}
// Copied from crossbeam and updated not to use thread::park

use crate::yield_now;
use core::cell::Cell;
use core::fmt;
use std::hint;

const SPIN_LIMIT: usize = 5;

/// Performs exponential backoff in spin loops.
///
/// Backing off in spin loops reduces contention and improves overall performance.
///
/// This primitive can execute *YIELD* and *PAUSE* instructions, yield the current thread to the OS
/// scheduler, and tell when it is a good time to block the thread using a different synchronization
/// mechanism. Each step of the back off procedure takes roughly twice as long as the previous
/// step.
pub struct Backoff {
    step: Cell<usize>,
}

impl Backoff {
    /// Creates a new `Backoff`.
    #[inline]
    pub fn new() -> Self {
        Self { step: Cell::new(0) }
    }

    /// Resets the `Backoff`.
    #[inline]
    pub(crate) fn reset(&self) {
        self.step.set(0);
    }

    /// Backs off in a lock-free loop.
    ///
    /// This method should be used when we need to retry an operation because another thread made
    /// progress.
    ///
    /// The processor may yield using the *YIELD* or *PAUSE* instruction.
    #[inline]
    pub fn spin(&self) {
        if !self.is_completed() {
            for _ in 0..1 << self.step.get() {
                hint::spin_loop();
                hint::spin_loop();
            }

            self.step.set(self.step.get() + 1);
        } else {
            // little optimization: `SPIN_LIMIT - 2` is calculated in the compile time and next
            // it uses four times fewer jumps.
            for _ in 0..1 << (SPIN_LIMIT - 2) {
                hint::spin_loop();
                hint::spin_loop();
                hint::spin_loop();
                hint::spin_loop();
            }
        }
    }

    /// Backs off in a blocking loop.
    ///
    /// This method should be used when we need to wait for another thread to make progress.
    ///
    /// The processor may yield using the *YIELD* or *PAUSE* instruction.
    ///
    /// In contrast to [`spin`](Self::spin), this method calls [`yield_now`] If it waits too long.
    ///
    /// If possible, use [`is_completed`](Self::is_completed) to check when it is advised to stop
    /// using backoff and block the current thread using a different synchronization
    /// mechanism instead.
    ///
    /// # When to use
    ///
    /// Even with Orengine runtime task switching isn't free, moreover, task switching often
    /// leads to the loss of some cached data for this task.
    ///
    /// Of course, switching tasks still takes only a couple of tens of nanoseconds,
    /// and the cache is cleared of application data much less than when switching threads.
    ///
    /// But in some cases, we can guarantee that most often we will have to wait only a couple
    /// of nanoseconds (for example, inserting data into an atomic ring queue)
    /// and only sometimes longer. It is for such situations that this method exists.
    ///
    /// Do not use it if you often have to wait more than 100 nanoseconds.
    pub async fn snooze(&self) {
        if !self.is_completed() {
            self.spin();

            return;
        }

        yield_now().await;
    }

    /// Returns `true` if exponential backoff has completed and blocking the thread is advised.
    #[inline]
    pub fn is_completed(&self) -> bool {
        self.step.get() == SPIN_LIMIT
    }
}

impl fmt::Debug for Backoff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Backoff")
            .field("step", &self.step)
            .field("is_completed", &self.is_completed())
            .finish()
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl Send for Backoff {}
unsafe impl Sync for Backoff {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as orengine;
    use crate::runtime::Task;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::{Duration, Instant};

    #[orengine::test::test_local]
    async fn test_backoff() {
        struct TestBackoff {
            backoff: Backoff,
        }

        impl Future for TestBackoff {
            type Output = ();

            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                for _ in 0..SPIN_LIMIT {
                    assert!(matches!(Box::pin(self.backoff.snooze()).as_mut().poll(cx), Poll::Ready(())));
                }

                assert!(self.backoff.is_completed());

                assert!(matches!(Box::pin(self.backoff.snooze()).as_mut().poll(cx), Poll::Pending));

                Poll::Ready(())
            }
        }

        let backoff = Backoff::new();

        let now = Instant::now();

        assert!(!backoff.is_completed());

        for _ in 0..100 {
            backoff.spin();
        }
        assert!(Instant::now().duration_since(now) >= Duration::from_nanos(100));

        assert!(backoff.is_completed());
        backoff.reset();

        assert!(!backoff.is_completed());

        TestBackoff { backoff }.await;

        unsafe { Task::park_current_task().await };
    }
}
// Copied from crossbeam and updated not to use thread::park

use core::cell::Cell;
use core::fmt;
use std::hint;

const SPIN_LIMIT: usize = 6;

/// Performs exponential backoff in spin loops.
///
/// Backing off in spin loops reduces contention and improves overall performance.
///
/// This primitive can execute *YIELD* and *PAUSE* instructions, yield the current thread to the OS
/// scheduler, and tell when it is a good time to block the thread using a different synchronization
/// mechanism. Each step of the back off procedure takes roughly twice as long as the previous
/// step.
pub(crate) struct Backoff {
    step: Cell<usize>,
}

impl Backoff {
    /// Creates a new `Backoff`.
    #[inline]
    pub(crate) fn new() -> Self {
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
    pub(crate) fn spin(&self) {
        if !self.is_completed() {
            for _ in 0..1 << self.step.get() {
                hint::spin_loop();
            }

            self.step.set(self.step.get() + 1);
        } else {
            for _ in 0..1 << SPIN_LIMIT {
                hint::spin_loop();
            }
        }
    }

    /// Returns `true` if exponential backoff has completed and blocking the thread is advised.
    #[inline]
    pub(crate) fn is_completed(&self) -> bool {
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

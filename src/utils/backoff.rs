//! This module provides a [`Backoff`]. It is worked from `crossbeam::Backoff`, read it for more
//! information.
#[cfg(debug_assertions)]
use crate::utils::OrengineInstant;
use crate::utils::{likely, long_preempt, short_preempt};
use core::cell::Cell;
use core::fmt;
// TODO update docs

const SPIN_LIMIT: u32 = 6;

/// It is worked from `crossbeam::Backoff`, read it for more information.
pub struct Backoff {
    step: Cell<u32>,
    #[cfg(debug_assertions)]
    start: Cell<Option<OrengineInstant>>,
}

impl Backoff {
    /// It is worked from `crossbeam::Backoff`, read it for more information.
    #[inline]
    pub fn new() -> Self {
        Self {
            step: Cell::new(0),
            #[cfg(debug_assertions)]
            start: Cell::new(None),
        }
    }

    /// It is worked from `crossbeam::Backoff`, read it for more information.
    #[inline]
    pub fn reset(&self) {
        self.step.set(0);

        #[cfg(debug_assertions)]
        {
            self.start.set(None);
        }
    }

    #[cfg(debug_assertions)]
    fn panic_if_possible_deadlock(&self) {
        if let Some(start) = self.start.get() {
            let now = OrengineInstant::now();
            let elapsed = now - start;

            assert!(elapsed.as_millis() < 1000, "[BUG] Deadlock detected");
        } else {
            self.start.set(Some(OrengineInstant::now()));
        }
    }

    /// It is worked from `crossbeam::Backoff`, read it for more information.
    #[inline]
    #[allow(
        dead_code,
        reason = "It is fork, therefore it is more convenient to keep all original methods"
    )]
    pub fn spin(&self) {
        #[cfg(debug_assertions)]
        {
            self.panic_if_possible_deadlock();
        }

        short_preempt();

        if self.step.get() < SPIN_LIMIT {
            self.step.set(self.step.get() + 1);
        }
    }

    /// It is worked from `crossbeam::Backoff`, read it for more information.
    #[inline]
    pub fn snooze(&self) {
        #[cfg(debug_assertions)]
        {
            self.panic_if_possible_deadlock();
        }

        if likely(self.step.get() < SPIN_LIMIT) {
            short_preempt();

            self.step.set(self.step.get() + 1);
        } else {
            long_preempt();

            self.step.set(0);
        }
    }

    /// It is worked from `crossbeam::Backoff`, read it for more information.
    #[inline]
    pub fn is_completed(&self) -> bool {
        self.step.get() >= SPIN_LIMIT
    }
}

impl fmt::Debug for Backoff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug_struct = f.debug_struct("Backoff");
        let mut updated = debug_struct
            .field("step", &self.step)
            .field("is_completed", &self.is_completed());

        #[cfg(debug_assertions)]
        {
            updated = updated.field("start", &self.start);
        }

        updated.finish()
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}

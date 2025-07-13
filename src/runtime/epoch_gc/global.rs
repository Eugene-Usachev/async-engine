use crate::runtime::epoch_gc::{EpochGCLocalManager, NumberOfExecutorsInEpoch};
// TODO docs
use crate::utils::unlikely;
use crossbeam::utils::CachePadded;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{Acquire, Release};

pub(crate) struct EpochGC {
    current_epoch: CachePadded<AtomicUsize>,
    number_of_executors_in_epoch: CachePadded<NumberOfExecutorsInEpoch>,
}

impl EpochGC {
    pub(crate) const fn new() -> Self {
        Self {
            current_epoch: CachePadded::new(AtomicUsize::new(0)),
            number_of_executors_in_epoch: CachePadded::new(NumberOfExecutorsInEpoch::new()),
        }
    }

    #[allow(unused_variables, reason = "It is used only with debug_assertions")]
    pub(crate) fn register_new_executor(&self) -> EpochGCLocalManager {
        self.number_of_executors_in_epoch.register_new_executor();

        EpochGCLocalManager::from_current_epoch(self.current_epoch())
    }

    /// Returns `true` if all executors passed the current epoch.
    #[allow(unused_variables, reason = "It is used only with debug_assertions")]
    pub(crate) fn deregister_executor(&self) -> bool {
        if unlikely(
            self.number_of_executors_in_epoch
                .deregister_executor_and_decrement_counter(),
        ) {
            self.current_epoch.fetch_add(1, Release);

            return true;
        }

        false
    }

    /// Returns `true` if all executors passed the current epoch.
    pub(crate) fn executor_passed_epoch(&self) -> bool {
        if unlikely(self.number_of_executors_in_epoch.executor_passed_epoch()) {
            // All executors passed the epoch, we can update the current epoch
            self.current_epoch.fetch_add(1, Release);

            return true;
        }

        false
    }

    pub(crate) fn current_epoch(&self) -> usize {
        self.current_epoch.load(Acquire)
    }
}

pub(crate) static GLOBAL_EPOCH_GC: EpochGC = EpochGC::new();

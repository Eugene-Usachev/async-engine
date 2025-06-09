use crate::sync::{AsyncMutex, AsyncMutexGuard};
use std::cell::UnsafeCell;
use std::marker::PhantomData;

/// A structure that pairs a value of type `T` with an asynchronous mutex `M`.
/// It ensures that the `T` value is accessed only when the associated mutex `M` is locked.
pub(crate) struct PairedWithLock<T, MT: ?Sized, M: AsyncMutex<MT> + ?Sized> {
    value: UnsafeCell<T>,
    #[cfg(debug_assertions)]
    prev_is_locked: crossbeam::atomic::AtomicCell<Option<*const M>>,
    phantom_data1: PhantomData<MT>,
    phantom_data2: PhantomData<M>,
}

impl<T, MT: ?Sized, M: AsyncMutex<MT> + ?Sized> PairedWithLock<T, MT, M> {
    /// Creates a new [`PairedWithLock`] instance, initializing the inner value with value.
    #[allow(
        unused_variables,
        reason = "One of the arguments is for debug_assertions only"
    )]
    pub(crate) const fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
            #[cfg(debug_assertions)]
            prev_is_locked: crossbeam::atomic::AtomicCell::new(None),
            phantom_data1: PhantomData,
            phantom_data2: PhantomData,
        }
    }

    /// Returns a mutable reference to the inner `T` value.
    ///
    /// This method requires a reference to the associated mutex `M`
    /// and asserts that the mutex is currently locked.
    ///
    /// In debug builds, it also verifies that the same mutex is used for
    /// all accesses to prevent misuse.
    #[allow(clippy::mut_from_ref, reason = "We guarantee safety using this code")]
    #[allow(unused_variables, reason = "`mutex` is used in debug_assertions only")]
    pub(crate) fn get_by_mutex(&self, mutex: &M) -> &mut T {
        #[cfg(debug_assertions)]
        {
            assert!(mutex.is_locked());

            let prev = self
                .prev_is_locked
                .swap(Some(std::ptr::from_ref::<M>(mutex)));

            assert!(
                prev.is_none() || prev == Some(std::ptr::from_ref::<M>(mutex)),
                "Attempt to use PairedWithLock with different mutexes"
            );
        }

        unsafe { &mut *self.value.get() }
    }

    /// Returns a mutable reference to the inner `T` value.
    ///
    /// This method takes an [`AsyncMutexGuard`] from the associated mutex,
    /// internally calling [`get_by_mutex`](Self::get_by_mutex) with
    /// the mutex retrieved from the guard.
    #[allow(clippy::mut_from_ref, reason = "We guarantee safety using this code")]
    #[allow(
        unused_variables,
        reason = "`mutex_guard` is used in debug_assertions only"
    )]
    pub(crate) fn get(&self, mutex_guard: &M::Guard<'_>) -> &mut T {
        self.get_by_mutex(mutex_guard.mutex())
    }
}

unsafe impl<T, MT: ?Sized, M: AsyncMutex<MT>> Sync for PairedWithLock<T, MT, M> {}
unsafe impl<T: Send, MT: ?Sized + Send, M: AsyncMutex<MT> + Send> Send
    for PairedWithLock<T, MT, M>
{
}

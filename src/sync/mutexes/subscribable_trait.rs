//! This module contains the [`AsyncSubscribableMutex`] trait.
use crate::runtime::Task;
use crate::sync::{AsyncMutex, AsyncMutexGuard};
use std::future::Future;
use std::marker::PhantomData;
use std::mem;
use std::pin::Pin;
use std::task::{Context, Poll};

enum GuardOrMutexRef<'mutex, T: ?Sized + 'mutex, Mu: AsyncSubscribableMutex<T> + ?Sized> {
    Guard(Mu::Guard<'mutex>),
    MutexRef(&'mutex Mu),
}

/// `WaitLockOfSubscribableMutex` implements [`Future`] that waits for a `lock`
/// with `subscription`.
///
/// For more details, read the documentation of the trait [`AsyncSubscribableMutex`].
pub struct WaitLockOfSubscribableMutex<'mutex, T, Mutex>
where
    T: 'mutex + ?Sized,
    Mutex: AsyncSubscribableMutex<T> + ?Sized,
{
    guard_or_mutex_ref: GuardOrMutexRef<'mutex, T, Mutex>,
    phantom_data: PhantomData<T>,
}

impl<'mutex, T, Mutex> WaitLockOfSubscribableMutex<'mutex, T, Mutex>
where
    T: 'mutex + ?Sized,
    Mutex: AsyncSubscribableMutex<T> + ?Sized,
{
    /// Creates a new `WaitLockOfSubscribableMutex`.
    pub fn new(guard: Mutex::Guard<'mutex>) -> Self {
        WaitLockOfSubscribableMutex {
            guard_or_mutex_ref: GuardOrMutexRef::Guard(guard),
            phantom_data: PhantomData,
        }
    }
}

impl<'mutex, T, Mutex> Future for WaitLockOfSubscribableMutex<'mutex, T, Mutex>
where
    T: 'mutex + ?Sized,
    Mutex: AsyncSubscribableMutex<T> + ?Sized,
{
    type Output = Mutex::Guard<'mutex>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let mutex;

        match &mut this.guard_or_mutex_ref {
            GuardOrMutexRef::Guard(guard) => {
                guard.mutex().low_level_subscribe(cx, guard);

                mutex = guard.mutex();
            }
            GuardOrMutexRef::MutexRef(mutex) => {
                return Poll::Ready(unsafe { mutex.get_locked() });
            }
        }

        drop(mem::replace(
            &mut this.guard_or_mutex_ref,
            GuardOrMutexRef::MutexRef(mutex),
        ));

        Poll::Pending
    }
}

/// `AsyncSubscribableMutex` is a trait that extends [`AsyncMutex`] by a
/// [`subscribe`](Self::subscribe) and [`low_level_subscribe`](Self::low_level_subscribe) methods.
///
/// For more details read [`subscribe`](Self::subscribe) and
/// [`low_level_subscribe`](Self::low_level_subscribe).
pub trait AsyncSubscribableMutex<T: ?Sized>: AsyncMutex<T> {
    /// Subscribing allows you to wait for the following [`unlock`] call.
    ///
    /// It means that one of the following calls [`unlock`] will wake
    /// the provided task up. It doesn't guarantee that [`unlock`] will be called
    /// or that the task will not wait if [`mutex`](AsyncMutex) is unlocked.
    ///
    /// It accepts the [`guard`](AsyncSubscribableMutex::Guard) to ensure that this method is called only while
    /// the [`mutex`](AsyncMutex) is locked.
    ///
    /// Indeed, this method is used to implement [`AsyncCondVar`](crate::sync::AsyncCondVar).
    ///
    /// This method is a bit more efficient than [`subscribe`](Self::subscribe).
    ///
    /// [`unlock`]: crate::sync::mutexes::Unlock::unlock
    fn subscribe_task<'mutex>(&self, task: Task, guard: &mut Self::Guard<'mutex>);

    /// Subscribing allows you to wait for the following [`unlock`] call.
    ///
    /// It means that one of the following calls [`unlock`] will wake
    /// the current task up. It doesn't guarantee that [`unlock`] will be called
    /// or that the task will not wait if [`mutex`](AsyncMutex) is unlocked.
    ///
    /// It accepts the [`guard`](AsyncSubscribableMutex::Guard) to ensure that this method is called only while
    /// the [`mutex`](AsyncMutex) is locked.
    ///
    /// Indeed, this method is used to implement [`AsyncCondVar`](crate::sync::AsyncCondVar).
    ///
    /// This method is a bit more efficient than [`subscribe`](Self::subscribe).
    ///
    /// [`unlock`]: crate::sync::mutexes::Unlock::unlock
    fn low_level_subscribe<'mutex>(&self, cx: &Context, guard: &mut Self::Guard<'mutex>);

    /// Subscribing allows you to wait for the following [`unlock`] call.
    ///
    /// It means that one of the following calls [`unlock`] will wake
    /// the current task up. It doesn't guarantee that [`unlock`] will be called
    /// or that the task will not wait if [`mutex`](AsyncMutex) is unlocked.
    ///
    /// It accepts the [`guard`](AsyncSubscribableMutex::Guard) to ensure that this method is called only while
    /// the [`mutex`](AsyncMutex) is locked.
    ///
    /// Indeed, this method is used to implement [`AsyncCondVar`](crate::sync::AsyncCondVar).
    ///
    /// [`subscribe`](Self::subscribe) is a bit more expensive than
    /// [`low_level_subscribe`](Self::low_level_subscribe).
    ///
    /// [`unlock`]: crate::sync::mutexes::Unlock::unlock
    fn subscribe<'mutex>(guard: Self::Guard<'mutex>) -> impl Future<Output = Self::Guard<'mutex>>
    where
        Self: 'mutex,
        T: 'mutex,
    {
        WaitLockOfSubscribableMutex::<T, Self>::new(guard)
    }
}

//! This module provides the [`Once`].
use std::future::Future;
use std::sync::atomic::AtomicIsize;
use std::sync::atomic::Ordering::{Acquire, Relaxed};

use crate::runtime::IsLocal;
use crate::sync::{AsyncOnce, CallOnceResult, OnceState};
use crate::utils::unwrap_or_bug_hint;

/// `Once` is an asynchronous [`std::Once`](std::sync::Once).
///
/// # Usage
///
/// `Once` is used to call a function only once.
///
/// # The difference between `Once` and [`LocalOnce`](crate::sync::LocalOnce)
///
/// The `Once` works with `shared tasks` and can be shared between threads.
///
/// Read [`Executor`](crate::Executor) for more details.
///
/// # Example
///
/// ```rust
/// use orengine::sync::{AsyncOnce, Once};
///
/// static START: Once = Once::new();
///
/// async fn async_print_msg_on_start() {
///     START.call_once(async {
///         // some async code
///         println!("start");
///     }).await;
/// }
///
/// async fn print_msg_on_start() {
///     START.call_once_sync(|| {
///         println!("start");
///     });
/// }
/// ```
#[repr(C)]
pub struct Once {
    state: AtomicIsize,
}

impl Once {
    /// Creates a new `Once`.
    pub const fn new() -> Self {
        Self {
            state: AtomicIsize::new(OnceState::not_called()),
        }
    }
}

impl IsLocal for Once {
    const IS_LOCAL: bool = false;
}

impl AsyncOnce for Once {
    #[inline]
    #[allow(
        clippy::future_not_send,
        reason = "It is not `Send` only when Fut is not `Send`, it is fine"
    )]
    async fn call_once<Fut: Future<Output = ()>>(&self, f: Fut) -> CallOnceResult {
        if self
            .state
            .compare_exchange(
                OnceState::NotCalled.into(),
                OnceState::Called.into(),
                Acquire,
                Relaxed,
            )
            .is_ok()
        {
            f.await;
            CallOnceResult::Called
        } else {
            CallOnceResult::WasAlreadyCompleted
        }
    }

    #[inline]
    fn call_once_sync<F: FnOnce()>(&self, f: F) -> CallOnceResult {
        if self
            .state
            .compare_exchange(
                OnceState::NotCalled.into(),
                OnceState::Called.into(),
                Acquire,
                Relaxed,
            )
            .is_ok()
        {
            f();

            CallOnceResult::Called
        } else {
            CallOnceResult::WasAlreadyCompleted
        }
    }

    #[inline]
    fn state(&self) -> OnceState {
        unwrap_or_bug_hint(OnceState::try_from(self.state.load(Acquire)))
    }
}

impl Default for Once {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl Sync for Once {}
unsafe impl Send for Once {}

/// ```rust
/// use orengine::sync::{Once, AsyncOnce};
/// use orengine::yield_now;
///
/// fn check_send<T: Send>(value: T) -> T { value }
///
/// async fn test() {
///     let once = Once::new();
///     let _ = check_send(once.call_once(async {})).await;
/// }
/// ```
#[allow(dead_code, reason = "It is used only in compile tests")]
fn test_compile_shared_once() {}

#[cfg(test)]
mod tests {
    use crate as orengine;
    use crate::sleep;
    use crate::sync::{AsyncOnce, AsyncWaitGroup, CallOnceResult, Once, OnceState, WaitGroup};
    use crate::test::sched_future;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering::SeqCst;
    use std::time::Duration;

    #[orengine::test::test_shared]
    fn test_async_shared_once() {
        let a = Arc::new(AtomicBool::new(false));
        let wg = Arc::new(WaitGroup::new());
        let once = Arc::new(Once::new());

        assert_eq!(once.state(), OnceState::NotCalled);
        assert!(!once.is_completed());

        for _ in 0..10 {
            let a = a.clone();
            let wg = wg.clone();
            let once = once.clone();

            wg.add(1).await;

            sched_future(async move {
                let _ = once
                    .call_once(async move {
                        sleep(Duration::from_millis(1)).await;
                        assert!(!a.load(SeqCst));
                        a.store(true, SeqCst);
                    })
                    .await;
                wg.done().await;
            });
        }

        wg.wait().await;

        assert!(once.is_completed());
        assert_eq!(
            once.call_once(async {}).await,
            CallOnceResult::WasAlreadyCompleted
        );
    }

    #[orengine::test::test_shared]
    fn test_sync_shared_once() {
        let a = Arc::new(AtomicBool::new(false));
        let wg = Arc::new(WaitGroup::new());
        let once = Arc::new(Once::new());

        assert_eq!(once.state(), OnceState::NotCalled);
        assert!(!once.is_completed());

        for _ in 0..10 {
            let a = a.clone();
            let wg = wg.clone();
            let once = once.clone();

            wg.add(1).await;

            sched_future(async move {
                let _ = once.call_once_sync(|| {
                    assert!(!a.load(SeqCst));
                    a.store(true, SeqCst);
                });

                wg.done().await;
            });
        }

        wg.wait().await;

        assert!(once.is_completed());
        assert_eq!(
            once.call_once_sync(|| ()),
            CallOnceResult::WasAlreadyCompleted
        );
    }
}

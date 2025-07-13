use orengine::sync::{AsyncRWLock, AsyncWaitGroup, RWLock, WaitGroup};
use orengine::test::sched_future;
use orengine::{local_executor, yield_now};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::AcqRel;
use std::thread;

#[orengine::test::test_shared(timeout_ms = 10000, exclusive_in = "*")]
fn stress_test_shared_rw_lock() {
    const PAR: usize = 6;
    const NUMBER_OF_TASKS: usize = 3;
    const TRIES: usize = 100000;

    static STEP: AtomicUsize = AtomicUsize::new(0);
    static TOTAL_READ: AtomicUsize = AtomicUsize::new(0);

    async fn work_with_lock(rw_lock: &RWLock<usize>, wg: &WaitGroup) {
        let step = STEP.fetch_add(1, AcqRel);

        let is_write = step % 6 == 0; // TODO 4

        if is_write {
            let mut lock = rw_lock.write().await;

            *lock += 1;

            let new = *lock;

            yield_now().await;

            assert_eq!(*lock, new);
        } else {
            let lock = rw_lock.read().await;

            let value = *lock;

            yield_now().await;

            assert_eq!(*lock, value);

            TOTAL_READ.fetch_add(value, AcqRel);
        }

        wg.done().await;
    }

    for _ in 0..10 {
        let rw_lock = Arc::new(RWLock::new(0));
        let wg = Arc::new(WaitGroup::new());

        wg.add(PAR * NUMBER_OF_TASKS * TRIES).await;

        for _ in 1..PAR {
            let wg = wg.clone();
            let rw_lock = rw_lock.clone();

            sched_future(AssertUnwindSafe(async move {
                for _ in 0..NUMBER_OF_TASKS {
                    let wg = wg.clone();
                    let rw_lock = rw_lock.clone();

                    local_executor().spawn_shared(async move {
                        for _ in 0..TRIES {
                            work_with_lock(&rw_lock, &wg).await;
                        }
                    });
                }

                wg.wait().await;
            }));
        }

        for _ in 0..NUMBER_OF_TASKS {
            let wg = wg.clone();
            let rw_lock = rw_lock.clone();

            local_executor().spawn_shared(async move {
                for _ in 0..TRIES {
                    work_with_lock(&rw_lock, &wg).await;
                }
            });
        }

        wg.wait().await;

        thread::yield_now();
    }
}

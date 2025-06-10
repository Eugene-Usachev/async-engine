use crate::acquire_global_lock;
use orengine::sync::{AsyncMutex, AsyncWaitGroup, Mutex, NaiveMutex, WaitGroup};
use orengine::test::sched_future_to_another_thread;
use std::sync::Arc;

#[orengine::test::test_shared(timeout_ms = 10000)]
fn stress_test_shared_mutex() {
    const PAR: usize = 5;
    const TRIES: usize = 40000;

    async fn work_with_lock(mutex: &Mutex<usize>, wg: &WaitGroup) {
        let mut lock = mutex.lock().await;

        *lock += 1;

        wg.done();
    }

    let lock = acquire_global_lock();

    for _ in 0..20 {
        let mutex = Arc::new(Mutex::new(0));
        let wg = Arc::new(WaitGroup::new());

        wg.add(PAR * TRIES);

        for _ in 1..PAR {
            let wg = wg.clone();
            let mutex = mutex.clone();
            sched_future_to_another_thread(async move {
                for _ in 0..TRIES {
                    work_with_lock(&mutex, &wg).await;
                }
            });
        }

        for _ in 0..TRIES {
            work_with_lock(&mutex, &wg).await;
        }

        wg.wait().await;

        assert_eq!(*mutex.lock().await, TRIES * PAR);
    }

    drop(lock);
}

#[orengine::test::test_shared(timeout_ms = 10000)]
fn stress_test_naive_mutex() {
    const PAR: usize = 10;
    const TRIES: usize = 10000;

    async fn work_with_lock(mutex: &NaiveMutex<usize>, wg: &WaitGroup) {
        let mut lock = mutex.lock().await;

        *lock += 1;

        wg.done();
    }

    let lock = acquire_global_lock();

    for _ in 0..20 {
        let mutex = Arc::new(NaiveMutex::new(0));
        let wg = Arc::new(WaitGroup::new());

        wg.add(PAR * TRIES);

        for _ in 1..PAR {
            let wg = wg.clone();
            let mutex = mutex.clone();

            sched_future_to_another_thread(async move {
                for _ in 0..TRIES {
                    work_with_lock(&mutex, &wg).await;
                }
            });
        }

        for _ in 0..TRIES {
            work_with_lock(&mutex, &wg).await;
        }

        wg.wait().await;

        assert_eq!(*mutex.lock().await, TRIES * PAR);
    }

    drop(lock);
}

use orengine::sync::{AsyncMutex, AsyncWaitGroup, Mutex, NaiveMutex, WaitGroup};
use orengine::test::sched_future;
use std::sync::Arc;

#[orengine::test::test_shared(timeout_ms = 10000, exclusive_in = "*")]
fn stress_test_shared_mutex() {
    const PAR: usize = 5;
    const TRIES: usize = 40000;

    async fn work_with_lock(mutex: &Mutex<usize>, wg: &WaitGroup) {
        let mut lock = mutex.lock().await;

        *lock += 1;

        wg.done().await;
    }

    for _ in 0..20 {
        let mutex = Arc::new(Mutex::new(0));
        let wg = Arc::new(WaitGroup::new());

        wg.add(PAR * TRIES).await;

        for _ in 1..PAR {
            let wg = wg.clone();
            let mutex = mutex.clone();

            sched_future(async move {
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
}

#[orengine::test::test_shared(timeout_ms = 10000, exclusive_in = "*")]
fn stress_test_naive_mutex() {
    const PAR: usize = 10;
    const TRIES: usize = 10000;

    async fn work_with_lock(mutex: &NaiveMutex<usize>, wg: &WaitGroup) {
        let mut lock = mutex.lock().await;

        *lock += 1;

        wg.done().await;
    }

    for _ in 0..20 {
        let mutex = Arc::new(NaiveMutex::new(0));
        let wg = Arc::new(WaitGroup::new());

        wg.add(PAR * TRIES).await;

        for _ in 1..PAR {
            let wg = wg.clone();
            let mutex = mutex.clone();

            sched_future(async move {
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
}

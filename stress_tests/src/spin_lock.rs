use crate::global_lock;
use orengine::sync::{AsyncWaitGroup, WaitGroup};
use orengine::test::sched_future_to_another_thread;
use orengine::utils::SpinLock;
use std::sync::Arc;

#[orengine::test::test_shared]
fn stress_test_mutex() {
    const PAR: usize = 4;
    const TRIES: usize = 1000;

    fn work_with_lock(mutex: &SpinLock<usize>, wg: &WaitGroup) {
        let mut lock = mutex.lock();
        *lock += 1;
        lock.unlock();

        wg.done();
    }

    let lock = global_lock();

    for _ in 0..20 {
        let mutex = Arc::new(SpinLock::new(0));
        let wg = Arc::new(WaitGroup::new());
        wg.add(PAR * TRIES);
        for _ in 1..PAR {
            let wg = wg.clone();
            let mutex = mutex.clone();
            sched_future_to_another_thread(async move {
                for _ in 0..TRIES {
                    work_with_lock(&mutex, &wg);
                }
            });
        }

        for _ in 0..TRIES {
            work_with_lock(&mutex, &wg);
        }

        wg.wait().await;

        assert_eq!(*mutex.lock(), TRIES * PAR);
    }

    drop(lock);
}
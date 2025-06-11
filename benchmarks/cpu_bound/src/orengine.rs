mod runtime;

use memory_stats::memory_stats;
use orengine::runtime::Config;
use orengine::sync::{AsyncMutex, AsyncRWLock, AsyncWaitGroup, LocalWaitGroup, Mutex, RWLock};
use orengine::{local_executor, sleep, sleep_until, yield_now, Executor};
use runtime::{DurationResult, Results, Runtime, SpawnManyTaskResult};
use std::hint::black_box;
use std::mem::MaybeUninit;
use std::rc::Rc;
use std::time::{Duration, Instant};

pub(crate) struct OrengineRuntime;

macro_rules! generate_create_task_and_yield {
    ($name:expr, $number_of_repetitions:expr, $size:expr) => {{
        let res = init_executor()
            .run_and_block_on_local(async move {
                const BATCH_SIZE: usize = 50;

                let ex = local_executor();
                let start = std::time::Instant::now();
                let wg = Rc::new(LocalWaitGroup::new());

                for _ in 0..$number_of_repetitions / BATCH_SIZE {
                    for _ in 0..BATCH_SIZE {
                        let wg_clone = wg.clone();

                        wg.inc();

                        ex.exec_local_future(async move {
                            let a = [MaybeUninit::<u8>::uninit(); $size];

                            yield_now().await;

                            black_box(a);

                            wg_clone.done();
                        });
                    }

                    wg.wait().await;
                }

                Some(DurationResult {
                    duration: start.elapsed(),
                    number_of_repetitions: $number_of_repetitions,
                })
            })
            .unwrap();

        println!("{} passed", $name);

        res
    }};
}

fn init_executor() -> &'static mut Executor {
    Executor::init_with_config(
        Config::default()
            .disable_io_worker()
            .disable_work_sharing()
            .set_numbers_of_blocking_workers(0),
    )
}

impl Runtime for OrengineRuntime {
    fn create_small_task_and_yield(&mut self) -> Option<DurationResult> {
        generate_create_task_and_yield!(
            "create_small_task_and_yield",
            30_000_000,
            Self::SMALL_TASK_SIZE
        )
    }

    fn create_large_task_and_yield(&mut self) -> Option<DurationResult> {
        generate_create_task_and_yield!(
            "create_large_task_and_yield",
            3_000_000,
            Self::LARGE_TASK_SIZE
        )
    }

    fn lock_and_update_and_unlock_mutex(&mut self) -> Option<DurationResult> {
        const REPETITIONS: usize = 80_000_000;

        init_executor()
            .run_and_block_on_shared(async {
                let start = Instant::now();
                let mutex = Mutex::new(0);

                for _ in 0..REPETITIONS {
                    let mut lock = mutex.lock().await;

                    *lock += 1;
                }

                println!("lock_and_update_and_unlock_mutex passed");

                Some(DurationResult {
                    duration: start.elapsed(),
                    number_of_repetitions: REPETITIONS,
                })
            })
            .unwrap()
    }

    fn lock_for_read_and_unlock_rwlock(&mut self) -> Option<DurationResult> {
        const REPETITIONS: usize = 80_000_000;

        init_executor()
            .run_and_block_on_shared(async {
                let start = Instant::now();
                let rwlock = RWLock::new(0);

                for _ in 0..REPETITIONS {
                    let lock = rwlock.read().await;

                    black_box(*lock);
                }

                println!("lock_for_read_and_unlock_rwlock passed");

                Some(DurationResult {
                    duration: start.elapsed(),
                    number_of_repetitions: REPETITIONS,
                })
            })
            .unwrap()
    }

    fn lock_for_write_and_read_and_unlock_rwlock(&mut self) -> Option<DurationResult> {
        const REPETITIONS: usize = 80_000_000;

        init_executor()
            .run_and_block_on_shared(async {
                let start = Instant::now();
                let rwlock = RWLock::new(0);

                for _ in 0..REPETITIONS {
                    let lock = rwlock.write().await;

                    black_box(*lock);
                }

                println!("lock_for_write_and_read_and_unlock_rwlock passed");

                Some(DurationResult {
                    duration: start.elapsed(),
                    number_of_repetitions: REPETITIONS,
                })
            })
            .unwrap()
    }

    fn yield_task(&mut self) -> Option<DurationResult> {
        const REPETITIONS: usize = 10_000_000;

        init_executor()
            .run_and_block_on_shared(async {
                let start = Instant::now();

                for _ in 0..REPETITIONS {
                    yield_now().await;
                }

                println!("yield_task passed");

                Some(DurationResult {
                    duration: start.elapsed(),
                    number_of_repetitions: REPETITIONS,
                })
            })
            .unwrap()
    }

    fn spawn_many_tasks(&mut self) -> Option<SpawnManyTaskResult> {
        const MANY_TASKS: usize = 15_000_000;

        init_executor()
            .run_and_block_on_local(async {
                let start_mem = memory_stats().unwrap().physical_mem;
                let ex = local_executor();

                for _ in 0..MANY_TASKS {
                    ex.exec_local_future(async {
                        sleep_until(Instant::now() + Duration::from_secs(1000)).await;
                    });
                }

                sleep(Duration::from_millis(500)).await; // TODO
                sleep(Duration::from_millis(500)).await;

                let end_mem = memory_stats().unwrap().physical_mem;

                println!("spawn_many_tasks passed");

                Some(SpawnManyTaskResult {
                    task_count: MANY_TASKS,
                    memory_usage_in_bytes: (end_mem - start_mem) as u64,
                })
            })
            .unwrap()
    }

    fn bench() -> Results {
        let mut runtime = OrengineRuntime;

        Results {
            create_small_task_and_yield: Self::create_small_task_and_yield(&mut runtime),
            create_large_task_and_yield: Self::create_large_task_and_yield(&mut runtime),
            lock_and_unlock_mutex: Self::lock_and_update_and_unlock_mutex(&mut runtime),
            lock_for_read_and_unlock_rwlock: Self::lock_for_read_and_unlock_rwlock(&mut runtime),
            lock_for_write_and_read_and_unlock_rwlock:
                Self::lock_for_write_and_read_and_unlock_rwlock(&mut runtime),
            yield_task: Self::yield_task(&mut runtime),
            spawn_many_tasks: Self::spawn_many_tasks(&mut runtime),
        }
    }

    fn name() -> &'static str {
        "Orengine"
    }
}

fn main() {
    // TODO
    OrengineRuntime::bench_and_print();

    // OrengineRuntime {}.create_large_task_and_yield();
    // OrengineRuntime {}.create_large_task_and_yield();
    // OrengineRuntime {}.create_large_task_and_yield();
}

use crate::runtime::{DurationResult, Results, Runtime, SpawnManyTaskResult};
use memory_stats::memory_stats;
use std::hint::black_box;
use std::time::Duration;
use tokio::time::{sleep, sleep_until, Instant};

pub(crate) struct TokioRuntime;

macro_rules! generate_create_task_and_yield {
    ($name:expr, $number_of_repetitions:expr, $size:expr) => {
        {
            tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap()
                .block_on(async move {
                    let start = std::time::Instant::now();

                    for _ in 0..$number_of_repetitions {
                        let _ = tokio::spawn(async {
                            let a = [0u8; $size];

                            tokio::task::yield_now().await;

                            black_box(a);
                        })
                        .await;
                    }

                    println!("{} passed", $name);

                    Some(DurationResult {
                        duration: start.elapsed(),
                        number_of_repetitions: $number_of_repetitions,
                    })
                })
        }
    };
}

fn new_tokio_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
}

impl Runtime for TokioRuntime {
    fn create_small_task_and_yield(&mut self) -> Option<DurationResult> {
        generate_create_task_and_yield!("create_small_task_and_yield", 5_000_000, Self::SMALL_TASK_SIZE)
    }

    fn create_large_task_and_yield(&mut self) -> Option<DurationResult> {
        generate_create_task_and_yield!("create_large_task_and_yield", 1_000_000, Self::LARGE_TASK_SIZE)
    }

    fn lock_and_update_and_unlock_mutex(&mut self) -> Option<DurationResult> {
        const REPETITIONS: usize = 50_000_000;

        new_tokio_runtime()
            .block_on(async move {
                let start = Instant::now();

                let mutex = tokio::sync::Mutex::new(0);

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
    }

    fn lock_for_read_and_unlock_rwlock(&mut self) -> Option<DurationResult> {
        const REPETITIONS: usize = 50_000_000;

        new_tokio_runtime()
            .block_on(async move {
                let start = Instant::now();
                let rwlock = tokio::sync::RwLock::new(0);

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
    }

    fn lock_for_write_and_read_and_unlock_rwlock(&mut self) -> Option<DurationResult> {
        const REPETITIONS: usize = 50_000_000;

        new_tokio_runtime()
            .block_on(async move {
                let start = Instant::now();
                let rwlock = tokio::sync::RwLock::new(0);

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
    }

    fn yield_task(&mut self) -> Option<DurationResult> {
        const REPETITIONS: usize = 10_000_000;

        new_tokio_runtime()
            .block_on(async move {
                let start = Instant::now();

                for _ in 0..REPETITIONS {
                    tokio::task::yield_now().await;
                }

                println!("yield_task passed");

                Some(DurationResult {
                    duration: start.elapsed(),
                    number_of_repetitions: REPETITIONS,
                })
            })
    }

    fn spawn_many_tasks(&mut self) -> Option<SpawnManyTaskResult> {
        const MANY_TASKS: usize = 5_000_000;

        new_tokio_runtime()
            .block_on(async move {
                let start_mem = memory_stats().unwrap().virtual_mem;

                for _ in 0..MANY_TASKS {
                    tokio::spawn(async {
                        sleep_until(Instant::now() + Duration::from_secs(10)).await;
                    });
                }

                sleep(Duration::from_secs(1)).await;

                let end_mem = memory_stats().unwrap().virtual_mem;

                println!("spawn_many_tasks passed");

                Some(SpawnManyTaskResult {
                    task_count: MANY_TASKS,
                    memory_usage_in_bytes: (end_mem - start_mem) as u64,
                })
            })
    }

    fn bench() -> Results {
        let mut runtime = TokioRuntime;

        Results {
            create_small_task_and_yield: Self::create_small_task_and_yield(&mut runtime),
            create_large_task_and_yield: Self::create_large_task_and_yield(&mut runtime),
            lock_and_unlock_mutex: Self::lock_and_update_and_unlock_mutex(&mut runtime),
            lock_for_read_and_unlock_rwlock: Self::lock_for_read_and_unlock_rwlock(&mut runtime),
            lock_for_write_and_read_and_unlock_rwlock: Self::lock_for_write_and_read_and_unlock_rwlock(&mut runtime),
            yield_task: Self::yield_task(&mut runtime),
            spawn_many_tasks: Self::spawn_many_tasks(&mut runtime),
        }
    }

    fn name() -> &'static str {
        "Tokio"
    }
}
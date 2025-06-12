mod runtime;

use crate::runtime::{Results, SpawnManyTaskResult};
use memory_stats::memory_stats;
use runtime::{DurationResult, Runtime};
use smol::{block_on, future, LocalExecutor};
use std::hint::black_box;
use std::mem::MaybeUninit;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{Acquire, Release};
use std::time::{Duration, Instant};

pub(crate) struct SmolRuntime;

macro_rules! generate_create_task_and_yield {
    ($name:expr, $number_of_repetitions:expr, $size:expr) => {{
        const BATCH_SIZE: usize = 50;

        block_on(LocalExecutor::new().run(async move {
            let start = std::time::Instant::now();
            let mut handles = Vec::with_capacity(BATCH_SIZE);

            for _ in 0..$number_of_repetitions / BATCH_SIZE {
                for _ in 0..BATCH_SIZE {
                    handles.push(smol::spawn(async {
                        let a = [MaybeUninit::<u8>::uninit(); $size];

                        future::yield_now().await;

                        black_box(a);
                    }));
                }

                for handle in handles.drain(..) {
                    handle.await;
                }
            }

            println!("{} passed", $name);

            Some(DurationResult {
                duration: start.elapsed(),
                number_of_repetitions: $number_of_repetitions,
            })
        }))
    }};
}

impl Runtime for SmolRuntime {
    fn name() -> &'static str {
        "Smol"
    }

    fn create_small_task_and_yield(&mut self) -> Option<DurationResult> {
        generate_create_task_and_yield!(
            "create_small_task_and_yield",
            1_000_000,
            Self::SMALL_TASK_SIZE
        )
    }

    fn create_large_task_and_yield(&mut self) -> Option<DurationResult> {
        generate_create_task_and_yield!(
            "create_large_task_and_yield",
            500_000,
            Self::LARGE_TASK_SIZE
        )
    }

    fn lock_and_update_and_unlock_mutex(&mut self) -> Option<DurationResult> {
        const REPETITIONS: usize = 10_000_000;

        block_on(LocalExecutor::new().run(async move {
            let start = Instant::now();

            let mutex = smol::lock::Mutex::new(0);

            for _ in 0..REPETITIONS {
                let mut lock = mutex.lock().await;

                *lock += 1;
            }

            println!("lock_and_update_and_unlock_mutex passed");

            Some(DurationResult {
                duration: start.elapsed(),
                number_of_repetitions: REPETITIONS,
            })
        }))
    }

    fn lock_for_read_and_unlock_rwlock(&mut self) -> Option<DurationResult> {
        const REPETITIONS: usize = 10_000_000;

        block_on(LocalExecutor::new().run(async move {
            let start = Instant::now();

            let mutex = smol::lock::RwLock::new(0);

            for _ in 0..REPETITIONS {
                let lock = mutex.read().await;

                black_box(*lock);
            }

            println!("lock_and_update_and_unlock_mutex passed");

            Some(DurationResult {
                duration: start.elapsed(),
                number_of_repetitions: REPETITIONS,
            })
        }))
    }

    fn lock_for_write_and_read_and_unlock_rwlock(&mut self) -> Option<DurationResult> {
        const REPETITIONS: usize = 10_000_000;

        block_on(LocalExecutor::new().run(async move {
            let start = Instant::now();

            for _ in 0..REPETITIONS {
                future::yield_now().await;
            }

            println!("lock_for_write_and_read_and_unlock_rwlock passed");

            Some(DurationResult {
                duration: start.elapsed(),
                number_of_repetitions: REPETITIONS,
            })
        }))
    }

    fn yield_task(&mut self) -> Option<DurationResult> {
        const REPETITIONS: usize = 10_000_000;
        const BATCH_SIZE: usize = 50;

        block_on(LocalExecutor::new().run(async move {
            let start = Instant::now();
            let mut handles = Vec::with_capacity(BATCH_SIZE);

            for _ in 0..BATCH_SIZE {
                handles.push(smol::spawn(async {
                    for _ in 0..REPETITIONS / BATCH_SIZE {
                        future::yield_now().await;
                    }
                }));
            }

            for handle in handles.drain(..) {
                handle.await;
            }

            println!("yield_task passed");

            Some(DurationResult {
                duration: start.elapsed(),
                number_of_repetitions: REPETITIONS,
            })
        }))
    }

    fn spawn_many_tasks(&mut self) -> Option<SpawnManyTaskResult> {
        const MANY_TASKS: usize = 10_000_000;

        block_on(LocalExecutor::new().run(async move {
            static SPAWNED: AtomicUsize = AtomicUsize::new(0);

            let start_mem = memory_stats().unwrap().physical_mem;

            for _ in 0..MANY_TASKS {
                smol::spawn(async {
                    SPAWNED.fetch_add(1, Release);

                    smol::Timer::after(Duration::from_secs(100)).await;
                })
                .detach();
            }

            while SPAWNED.load(Acquire) < MANY_TASKS {
                smol::Timer::after(Duration::from_millis(200)).await;
            }

            let end_mem = memory_stats().unwrap().physical_mem;

            println!("spawn_many_tasks passed");

            Some(SpawnManyTaskResult {
                task_count: MANY_TASKS,
                memory_usage_in_bytes: (end_mem - start_mem) as u64,
            })
        }))
    }

    fn bench() -> Results {
        let mut runtime = SmolRuntime;

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
}

fn main() {
    SmolRuntime::bench_and_print();
}

mod runtime;

use memory_stats::memory_stats;
use runtime::{DurationResult, Results, Runtime, SpawnManyTaskResult};
use std::hint::black_box;
use std::mem::MaybeUninit;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{Acquire, Release};
use std::time::Duration;
use std::time::Instant;

use std::sync::{Arc, Mutex, RwLock};
use std::thread;

pub(crate) struct StdThreadRuntime;

macro_rules! generate_create_task_and_yield {
    ($name:expr, $number_of_repetitions:expr, $size:expr) => {{
        const BATCH_SIZE: usize = 50;

        let start = Instant::now();
        let mut handles = Vec::with_capacity(BATCH_SIZE);

        for _ in 0..$number_of_repetitions / BATCH_SIZE {
            for _ in 0..BATCH_SIZE {
                handles.push(thread::spawn(move || {
                    let a = [MaybeUninit::<u8>::uninit(); $size];

                    thread::yield_now();

                    black_box(a);
                }));
            }

            for handle in handles.drain(..) {
                handle.join().unwrap();
            }
        }

        println!("{} passed", $name);

        Some(DurationResult {
            duration: start.elapsed(),
            number_of_repetitions: $number_of_repetitions,
        })
    }};
}

impl Runtime for StdThreadRuntime {
    fn create_small_task_and_yield(&mut self) -> Option<DurationResult> {
        generate_create_task_and_yield!(
            "create_small_task_and_yield",
            20_000,
            Self::SMALL_TASK_SIZE
        )
    }

    fn create_large_task_and_yield(&mut self) -> Option<DurationResult> {
        generate_create_task_and_yield!(
            "create_large_task_and_yield",
            20_000,
            Self::LARGE_TASK_SIZE
        )
    }

    fn lock_and_update_and_unlock_mutex(&mut self) -> Option<DurationResult> {
        const REPETITIONS: usize = 50_000_000;

        let start = Instant::now();
        let mutex = Mutex::new(0);

        for _ in 0..REPETITIONS {
            let mut lock = mutex.lock().unwrap();
            *lock += 1;
        }

        println!("lock_and_update_and_unlock_mutex passed");

        Some(DurationResult {
            duration: start.elapsed(),
            number_of_repetitions: REPETITIONS,
        })
    }

    fn lock_for_read_and_unlock_rwlock(&mut self) -> Option<DurationResult> {
        const REPETITIONS: usize = 50_000_000;

        let start = Instant::now();
        let rwlock = RwLock::new(0);

        for _ in 0..REPETITIONS {
            let lock = rwlock.read().unwrap();

            black_box(*lock);
        }

        println!("lock_for_read_and_unlock_rwlock passed");

        Some(DurationResult {
            duration: start.elapsed(),
            number_of_repetitions: REPETITIONS,
        })
    }

    fn lock_for_write_and_read_and_unlock_rwlock(&mut self) -> Option<DurationResult> {
        const REPETITIONS: usize = 50_000_000;

        let start = Instant::now();
        let rwlock = RwLock::new(0);

        for _ in 0..REPETITIONS {
            #[allow(clippy::readonly_write_lock, reason = "false positive")]
            let lock = rwlock.write().unwrap();

            black_box(*lock);
        }

        println!("lock_for_write_and_read_and_unlock_rwlock passed");

        Some(DurationResult {
            duration: start.elapsed(),
            number_of_repetitions: REPETITIONS,
        })
    }

    fn yield_task(&mut self) -> Option<DurationResult> {
        const REPETITIONS: usize = 1_000_000;
        const BATCH_SIZE: usize = 50;

        let start = Instant::now();
        let mut handles = Vec::with_capacity(BATCH_SIZE);

        for _ in 0..BATCH_SIZE {
            handles.push(thread::spawn(move || {
                for _ in 0..REPETITIONS / BATCH_SIZE {
                    thread::yield_now();
                }
            }));
        }

        for handle in handles.drain(..) {
            handle.join().unwrap();
        }

        println!("yield_task passed");

        Some(DurationResult {
            duration: start.elapsed(),
            number_of_repetitions: REPETITIONS,
        })
    }

    fn spawn_many_tasks(&mut self) -> Option<SpawnManyTaskResult> {
        const MANY_TASKS: usize = 10_000;

        let spawned_counter = Arc::new(AtomicUsize::new(0));

        let start_mem = memory_stats().unwrap().physical_mem;

        for _ in 0..MANY_TASKS {
            let spawned_clone = Arc::clone(&spawned_counter);
            thread::spawn(move || {
                spawned_clone.fetch_add(1, Release);

                thread::sleep(Duration::from_secs(100));
            });
        }

        while spawned_counter.load(Acquire) < MANY_TASKS {
            thread::sleep(Duration::from_millis(200));
        }

        let end_mem = memory_stats().unwrap().physical_mem;

        println!("spawn_many_tasks passed");

        Some(SpawnManyTaskResult {
            task_count: MANY_TASKS,
            memory_usage_in_bytes: (end_mem - start_mem) as u64,
        })
    }

    fn bench() -> Results {
        let mut runtime = StdThreadRuntime;

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
        "sync"
    }
}

fn main() {
    StdThreadRuntime::bench_and_print();
}

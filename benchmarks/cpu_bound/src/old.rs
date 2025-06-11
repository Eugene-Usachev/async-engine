#![allow(internal_features)]
use std::thread;
use std::time::Duration;
// use orengine::runtime::{local_executor, stop_all_executors};
// use orengine::sync::{AsyncMutex, AsyncRWLock, AsyncWaitGroup, LocalWaitGroup};
// use orengine::Executor;
// use smol::future;
// use std::hint::black_box;
// use std::rc::Rc;
// use std::thread;
// use tools::bench;
//
// fn init_orengine_cpu_bound() {
//     let cfg = orengine::runtime::Config::default()
//         .disable_work_sharing()
//         .disable_io_worker()
//         .set_numbers_of_blocking_workers(0);
//
//     Executor::init_with_config(cfg);
// }
//
// fn bench_create_task_and_yield() {
//     const LARGE_SIZE: usize = 9000;
//
//     // TODO
//     // bench("async_std small create_task_and_yield", |mut b| {
//     //     async_std::task::block_on(async move {
//     //         b.iter_async(|| async {
//     //             let a = async_std::task::spawn(async {
//     //                 async_std::task::yield_now().await;
//     //                 black_box(0)
//     //             })
//     //             .await;
//     //             black_box(a);
//     //         })
//     //         .await;
//     //     });
//     // });
//     //
//     // bench("tokio small create_task_and_yield", |mut b| {
//     //     tokio::runtime::Builder::new_current_thread()
//     //         .build()
//     //         .unwrap()
//     //         .block_on(async move {
//     //             b.iter_async(|| async {
//     //                 let a = tokio::spawn(async {
//     //                     tokio::task::yield_now().await;
//     //                     black_box(0)
//     //                 })
//     //                 .await
//     //                 .unwrap();
//     //                 black_box(a);
//     //             })
//     //             .await;
//     //         });
//     // });
//     //
//     // bench("smol small create_task_and_yield", |mut b| {
//     //     future::block_on(smol::LocalExecutor::new().run(async move {
//     //         b.iter_async(|| async {
//     //             let a = smol::spawn(async {
//     //                 future::yield_now().await;
//     //                 black_box(0)
//     //             })
//     //             .await;
//     //             black_box(a);
//     //         })
//     //         .await;
//     //     }));
//     // });
//     //
//     bench("orengine small create_task_and_yield", |mut b| {
//         init_orengine_cpu_bound();
//
//         local_executor().run_and_block_on_local(async move {
//             let wg = Rc::new(LocalWaitGroup::new());
//
//             b.iter_async(|| async {
//                 let wg_clone = wg.clone();
//                 local_executor().spawn_local(async move {
//                     orengine::yield_now().await;
//
//                     wg_clone.done();
//                 });
//
//                 wg.wait().await;
//             })
//             .await;
//         })
//             .unwrap();
//     });
//     //
//     // bench("sync small create_task_and_yield", |mut b| {
//     //     b.iter(|| {
//     //         let a = thread::spawn(|| {
//     //             thread::yield_now();
//     //             black_box(1)
//     //         })
//     //         .join()
//     //         .unwrap();
//     //         black_box(a);
//     //     });
//     // });
//     //
//     // bench("async_std large create_task_and_yield", |mut b| {
//     //     async_std::task::block_on(async move {
//     //         b.iter_async(|| async {
//     //             let a = async_std::task::spawn(async {
//     //                 let a = black_box([0u8; LARGE_SIZE]);
//     //                 async_std::task::yield_now().await;
//     //                 black_box(a.len())
//     //             })
//     //             .await;
//     //             black_box(a);
//     //         })
//     //         .await;
//     //     });
//     // });
//     //
//     // bench("tokio large create_task_and_yield", |mut b| {
//     //     tokio::runtime::Builder::new_current_thread()
//     //         .build()
//     //         .unwrap()
//     //         .block_on(async move {
//     //             b.iter_async(|| async {
//     //                 let a = tokio::spawn(async {
//     //                     let a = black_box([0u8; LARGE_SIZE]);
//     //                     tokio::task::yield_now().await;
//     //                     black_box(a.len())
//     //                 })
//     //                 .await
//     //                 .unwrap();
//     //                 black_box(a);
//     //             })
//     //             .await;
//     //         });
//     // });
//     //
//     // bench("smol large create_task_and_yield", |mut b| {
//     //     future::block_on(smol::LocalExecutor::new().run(async move {
//     //         b.iter_async(|| async {
//     //             let a = smol::spawn(async {
//     //                 let a = black_box([0u8; LARGE_SIZE]);
//     //                 future::yield_now().await;
//     //                 black_box(a.len())
//     //             })
//     //             .await;
//     //             black_box(a);
//     //         })
//     //         .await;
//     //     }));
//     // });
//
//     bench("orengine large create_task_and_yield", |mut b| {
//         init_orengine_cpu_bound();
//         local_executor().run_and_block_on_local(async move {
//             let wg = Rc::new(LocalWaitGroup::new());
//
//             b.iter_async(|| async {
//                 let wg_clone = wg.clone();
//
//                 local_executor().spawn_local(async move {
//                     let a = black_box([0u8; LARGE_SIZE]);
//
//                     orengine::yield_now().await;
//
//                     black_box(a.len());
//
//                     wg_clone.done();
//                 });
//
//                 wg.wait().await;
//             })
//             .await;
//         })
//             .unwrap();
//     });
//
//     bench("sync large create_task_and_yield", |mut b| {
//         b.iter(|| {
//             let a = thread::spawn(|| {
//                 let a = black_box([0u8; LARGE_SIZE]);
//                 thread::yield_now();
//                 black_box(a.len())
//             })
//             .join()
//             .unwrap();
//             black_box(a);
//         });
//     });
// }
//
// fn bench_yield_task() {
//     const NUMBER_TASKS: usize = 10;
//     const YIELDS: usize = 10_000;
//     const YIELDS_PER_TASK: usize = YIELDS / NUMBER_TASKS;
//
//     // bench("async_std task switch", |mut b| {
//     //     async_std::task::block_on(async move {
//     //         b.iter_async(|| async {
//     //             let (tx, rx) = async_std::channel::bounded(NUMBER_TASKS);
//     //             for _ in 0..NUMBER_TASKS {
//     //                 let tx = tx.clone();
//     //                 async_std::task::spawn(async move {
//     //                     for _ in 0..YIELDS_PER_TASK {
//     //                         async_std::task::yield_now().await;
//     //                     }
//     //
//     //                     tx.send(()).await.unwrap();
//     //                 });
//     //             }
//     //
//     //             for _ in 0..NUMBER_TASKS {
//     //                 rx.recv().await.unwrap();
//     //             }
//     //         })
//     //         .await;
//     //     });
//     // });
//     //
//     // bench("tokio task switch", |mut b| {
//     //     tokio::runtime::Builder::new_current_thread()
//     //         .build()
//     //         .unwrap()
//     //         .block_on(async move {
//     //             b.iter_async(|| async {
//     //                 let (tx, mut rx) = tokio::sync::mpsc::channel(NUMBER_TASKS);
//     //                 for _ in 0..NUMBER_TASKS {
//     //                     let tx = tx.clone();
//     //                     tokio::spawn(async move {
//     //                         for _ in 0..YIELDS_PER_TASK {
//     //                             tokio::task::yield_now().await;
//     //                         }
//     //
//     //                         tx.send(()).await.unwrap();
//     //                     });
//     //                 }
//     //
//     //                 for _ in 0..NUMBER_TASKS {
//     //                     rx.recv().await.unwrap();
//     //                 }
//     //             })
//     //             .await;
//     //         });
//     // });
//
//     bench("smol task switch", |mut b| {
//         future::block_on(smol::LocalExecutor::new().run(async move {
//             b.iter_async(|| async {
//                 let (tx, rx) = smol::channel::bounded(NUMBER_TASKS);
//                 for _ in 0..NUMBER_TASKS {
//                     let tx = tx.clone();
//                     smol::spawn(async move {
//                         for _ in 0..YIELDS_PER_TASK {
//                             future::yield_now().await;
//                         }
//
//                         tx.send(()).await.unwrap();
//                     })
//                     .detach();
//                 }
//
//                 for _ in 0..NUMBER_TASKS {
//                     rx.recv().await.unwrap();
//                 }
//             })
//             .await;
//         }));
//     });
//
//     bench("orengine task switch", |mut b| {
//         init_orengine_cpu_bound();
//         local_executor()
//             .run_and_block_on_local(async move {
//                 b.iter_async(|| async {
//                     for _ in 0..NUMBER_TASKS {
//                         local_executor().exec_local_future(async move {
//                             for _ in 0..YIELDS_PER_TASK {
//                                 orengine::yield_now().await;
//                             }
//                         });
//                     }
//                 })
//                 .await;
//             })
//             .unwrap();
//     });
//
//     // bench("sync task switch", |mut b| {
//     //     b.iter(|| {
//     //         thread::scope(|scope| {
//     //             for _ in 0..NUMBER_TASKS {
//     //                 scope.spawn(move || {
//     //                     for _ in 0..YIELDS_PER_TASK {
//     //                         thread::yield_now();
//     //                     }
//     //                 });
//     //             }
//     //         });
//     //     });
//     // });
// }
//
// fn bench_mutex() {
//     const N: usize = 20_000;
//
//     fn bench_std() {
//         bench("std mutex", |mut b| {
//             b.iter(move || {
//                 let mutex = std::sync::Mutex::new(0);
//                 for _ in 0..N {
//                     let mut guard = mutex.lock().unwrap();
//                     *guard += 1;
//                 }
//             });
//         });
//     }
//
//     fn bench_tokio() {
//         bench("tokio mutex", |mut b| {
//             tokio::runtime::Builder::new_current_thread()
//                 .build()
//                 .unwrap()
//                 .block_on(async move {
//                     b.iter_async(|| async {
//                         let mutex = tokio::sync::Mutex::new(0);
//                         for _ in 0..N {
//                             let mut guard = mutex.lock().await;
//                             *guard += 1;
//                         }
//                     })
//                     .await;
//                 });
//         });
//     }
//
//     fn bench_smol() {
//         bench("smol mutex", |mut b| {
//             future::block_on(smol::LocalExecutor::new().run(async move {
//                 b.iter_async(|| async {
//                     let mutex = smol::lock::Mutex::new(0);
//                     for _ in 0..N {
//                         let mut guard = mutex.lock().await;
//                         *guard += 1;
//                     }
//                 })
//                 .await;
//             }));
//         });
//     }
//
//     fn bench_orengine() {
//         bench("orengine naive mutex", |mut b| {
//             init_orengine_cpu_bound();
//             local_executor()
//                 .run_and_block_on_shared(async move {
//                     b.iter_async(|| async {
//                         let mutex = orengine::sync::NaiveMutex::new(0);
//                         for _ in 0..N {
//                             let mut guard = mutex.lock().await;
//                             *guard += 1;
//                         }
//                     })
//                     .await;
//                     stop_all_executors();
//                 })
//                 .unwrap();
//         });
//
//         bench("orengine mutex", |mut b| {
//             init_orengine_cpu_bound();
//             local_executor()
//                 .run_and_block_on_shared(async move {
//                     b.iter_async(|| async {
//                         let mutex = orengine::sync::Mutex::new(0);
//                         for _ in 0..N {
//                             let mut guard = mutex.lock().await;
//                             *guard += 1;
//                         }
//                     })
//                     .await;
//                     stop_all_executors();
//                 })
//                 .unwrap();
//         });
//     }
//
//     bench_std();
//     bench_tokio();
//     bench_smol();
//     bench_orengine();
// }
//
// fn bench_rw_lock() {
//     const N: usize = 20_000;
//
//     bench("std rwlock - read", |mut b| {
//         b.iter(move || {
//             let rw_lock = std::sync::RwLock::new(0);
//             for _ in 0..N {
//                 let guard = rw_lock.read().unwrap();
//                 black_box(*guard);
//             }
//         });
//     });
//
//     bench("std rwlock - write", |mut b| {
//         b.iter(move || {
//             let rw_lock = std::sync::RwLock::new(0);
//             for _ in 0..N {
//                 let mut guard = rw_lock.write().unwrap();
//                 *guard += 1;
//             }
//         });
//     });
//
//     // bench("tokio rwlock - read", |mut b| {
//     //     tokio::runtime::Builder::new_current_thread()
//     //         .build()
//     //         .unwrap()
//     //         .block_on(async move {
//     //             b.iter_async(|| async {
//     //                 let rw_lock = tokio::sync::RwLock::new(0);
//     //                 for _ in 0..N {
//     //                     let guard = rw_lock.read().await;
//     //                     black_box(*guard);
//     //                 }
//     //             })
//     //                 .await;
//     //         });
//     // });
//     //
//     // bench("tokio rwlock - write", |mut b| {
//     //     tokio::runtime::Builder::new_current_thread()
//     //         .build()
//     //         .unwrap()
//     //         .block_on(async move {
//     //             b.iter_async(|| async {
//     //                 let rw_lock = tokio::sync::RwLock::new(0);
//     //                 for _ in 0..N {
//     //                     let mut guard = rw_lock.write().await;
//     //                     *guard += 1;
//     //                 }
//     //             })
//     //                 .await;
//     //         });
//     // });
//     //
//     // bench("smol rwlock - read", |mut b| {
//     //     future::block_on(smol::LocalExecutor::new().run(async move {
//     //         b.iter_async(|| async {
//     //             let rw_lock = smol::lock::RwLock::new(0);
//     //             for _ in 0..N {
//     //                 let guard = rw_lock.read().await;
//     //                 black_box(*guard);
//     //             }
//     //         })
//     //             .await;
//     //     }));
//     // });
//     //
//     // bench("smol rwlock - write", |mut b| {
//     //     future::block_on(smol::LocalExecutor::new().run(async move {
//     //         b.iter_async(|| async {
//     //             let rw_lock = smol::lock::RwLock::new(0);
//     //             for _ in 0..N {
//     //                 let mut guard = rw_lock.write().await;
//     //                 *guard += 1;
//     //             }
//     //         })
//     //             .await;
//     //     }));
//     // });
//
//     bench("orengine rwlock - read", |mut b| {
//         init_orengine_cpu_bound();
//         local_executor()
//             .run_and_block_on_shared(async move {
//                 b.iter_async(|| async {
//                     let rw_lock = orengine::sync::RWLock::new(0);
//                     for _ in 0..N {
//                         let guard = rw_lock.read().await;
//                         black_box(*guard);
//                     }
//                 })
//                     .await;
//                 stop_all_executors();
//             })
//             .unwrap();
//     });
//
//     bench("orengine rwlock - write", |mut b| {
//         init_orengine_cpu_bound();
//         local_executor()
//             .run_and_block_on_shared(async move {
//                 b.iter_async(|| async {
//                     let rw_lock = orengine::sync::RWLock::new(0);
//                     for _ in 0..N {
//                         let mut guard = rw_lock.write().await;
//                         *guard += 1;
//                     }
//                 })
//                     .await;
//                 stop_all_executors();
//             })
//             .unwrap();
//     });
//
//     bench("orengine rwlock - try_read", |mut b| {
//         init_orengine_cpu_bound();
//         local_executor()
//             .run_and_block_on_shared(async move {
//                 b.iter_async(|| async {
//                     let rw_lock = orengine::sync::RWLock::new(0);
//                     for _ in 0..N {
//                         let guard = rw_lock.try_read().expect("try_read should succeed in non-contended benchmark");
//                         black_box(*guard);
//                     }
//                 })
//                     .await;
//                 stop_all_executors();
//             })
//             .unwrap();
//     });
//
//     bench("orengine rwlock - try_write", |mut b| {
//         init_orengine_cpu_bound();
//         local_executor()
//             .run_and_block_on_shared(async move {
//                 b.iter_async(|| async {
//                     let rw_lock = orengine::sync::RWLock::new(0);
//                     for _ in 0..N {
//                         let mut guard = rw_lock.try_write().expect("try_write should succeed in non-contended benchmark");
//                         *guard += 1;
//                     }
//                 })
//                     .await;
//                 stop_all_executors();
//             })
//             .unwrap();
//     });
// }

// TODO remove the whole file
fn main() {
    // bench_create_task_and_yield();
    // bench_mutex();
    // bench_yield_task();
    // bench_rw_lock();

    TokioRuntime::bench_and_print();

    thread::sleep(Duration::from_secs(2));

    OrengineRuntime::bench_and_print();
}

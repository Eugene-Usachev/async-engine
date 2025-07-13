//! This module provides tests for the epoch GC (including stress tests).
use crate as orengine;
use crate::runtime::epoch_gc::{EpochGCLocalManager, local_epoch_gc};
use crate::sync::{AsyncCondVar, AsyncMutex, AsyncWaitGroup, CondVar, Mutex, WaitGroup};
use crate::test::sched_future;
use crate::utils::droppable_element::DroppableElement;
use crate::{local_executor, yield_now};
use std::sync::{Arc, Mutex as SyncMutex};
use std::time::Duration;

#[orengine::test::test_local(timeout_ms = 10000, exclusive_in = "*")]
fn test_epoch_gc_basic_deallocation() {
    let value = 42;
    let size = size_of_val(&value);
    let ptr = Box::into_raw(Box::new(value));
    let initial_bytes = EpochGCLocalManager::bytes_deallocated();

    unsafe {
        local_epoch_gc().schedule_deallocate(ptr);
    }

    assert_eq!(initial_bytes, EpochGCLocalManager::bytes_deallocated());

    local_epoch_gc().wait_new_epoch().await;

    assert_eq!(
        initial_bytes + size,
        EpochGCLocalManager::bytes_deallocated()
    );
}

#[orengine::test::test_local(timeout_ms = 20000, exclusive_in = "*")]
fn test_epoch_gc_slice_deallocation() {
    let initial_bytes = EpochGCLocalManager::bytes_deallocated();
    let len = 5;
    let slice = vec![0; len].into_boxed_slice();
    let ptr = Box::into_raw(slice) as *const i32;
    let expected_bytes = size_of::<i32>() * len;

    unsafe {
        local_epoch_gc().schedule_deallocate_slice(ptr, len);
    }

    assert_eq!(EpochGCLocalManager::bytes_deallocated(), initial_bytes);

    local_epoch_gc().wait_new_epoch().await;

    assert_eq!(
        EpochGCLocalManager::bytes_deallocated(),
        initial_bytes + expected_bytes
    );
}

#[orengine::test::test_local(timeout_ms = 10000, exclusive_in = "*")]
fn test_epoch_gc_drop_function() {
    let slice = Arc::new(SyncMutex::new(Vec::new()));
    let elem = DroppableElement::new(0, slice.clone());

    unsafe {
        local_epoch_gc().schedule_drop(move || {
            drop(elem);
        });
    }

    assert_eq!(slice.lock().unwrap().len(), 0);

    local_epoch_gc().wait_new_epoch().await;

    assert_eq!(slice.lock().unwrap().len(), 1);
}

#[orengine::test::test_shared(timeout_ms = 30000, exclusive_in = "*")]
fn test_epoch_gc_concurrent() {
    for _ in 0..3 {
        let was_finished = Arc::new(CondVar::new(Mutex::new(false)));
        let slice = Arc::new(SyncMutex::new(Vec::new()));
        let droppable_elem = DroppableElement::new(0, slice.clone());
        let value = 42;
        let size = size_of_val(&value);
        let ptr = Box::into_raw(Box::new(value));
        let initial_bytes = EpochGCLocalManager::bytes_deallocated();

        unsafe {
            local_epoch_gc().schedule_deallocate(ptr);
            local_epoch_gc().schedule_drop(move || {
                drop(droppable_elem);
            });
        }

        let slice_clone = slice.clone();
        let was_finished_clone = was_finished.clone();
        sched_future(async move {
            unsafe {
                local_epoch_gc().schedule_drop(move || {
                    drop(DroppableElement::new(1, slice_clone));
                });
            }

            let mut guard = was_finished_clone.lock().await;

            while !*guard {
                guard = was_finished_clone.wait(guard).await;
            }
        });

        std::thread::sleep(Duration::from_millis(40));

        assert_eq!(initial_bytes, EpochGCLocalManager::bytes_deallocated());
        assert_eq!(slice.lock().unwrap().len(), 0);

        local_epoch_gc().wait_new_epoch().await;

        yield_now().await;

        assert_eq!(
            initial_bytes + size,
            EpochGCLocalManager::bytes_deallocated()
        );
        assert_eq!(slice.lock().unwrap().len(), 2);

        let mut guard = was_finished.lock().await;

        *guard = true;
        was_finished.notify_all(guard);
    }
}

mod limited_allocator {
    use crate::runtime::epoch_gc::EpochGCLocalManager;
    use crate::sleep;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[cfg(not(debug_assertions))]
    const MAX_MEMORY_ALLOCATED: usize = 10 * 1024 * 1024;
    #[cfg(debug_assertions)]
    const MAX_MEMORY_ALLOCATED: usize = 2 * 1024 * 1024;

    pub(crate) struct Allocator {
        memory_used_from_start: AtomicUsize,
        memory_deallocated_at_start: AtomicUsize,
    }

    impl Allocator {
        pub(crate) const fn new() -> Self {
            Self {
                memory_used_from_start: AtomicUsize::new(0),
                memory_deallocated_at_start: AtomicUsize::new(0),
            }
        }

        pub(crate) fn reset(&self) {
            self.memory_used_from_start.store(0, Ordering::Release);
            self.memory_deallocated_at_start
                .store(EpochGCLocalManager::bytes_deallocated(), Ordering::Release);

            assert_eq!(self.memory_used_now(), 0);
        }

        fn memory_used_now(&self) -> usize {
            self.memory_used_from_start.load(Ordering::Acquire)
                + self.memory_deallocated_at_start.load(Ordering::Acquire)
                - EpochGCLocalManager::bytes_deallocated()
        }

        pub(crate) async fn allocate<T>(&self) -> *mut T {
            let layout = std::alloc::Layout::new::<T>();

            loop {
                let memory_used = self.memory_used_now();

                if memory_used + layout.size() > MAX_MEMORY_ALLOCATED {
                    sleep(Duration::from_millis(10)).await;

                    continue;
                }

                self.memory_used_from_start
                    .fetch_add(layout.size(), Ordering::AcqRel);

                break unsafe { std::alloc::alloc(layout).cast() };
            }
        }
    }

    static LIMITED_ALLOCATOR: Allocator = Allocator::new();

    pub(crate) fn limited_allocator() -> &'static Allocator {
        &LIMITED_ALLOCATOR
    }
}

mod lock_free_stack {
    use super::limited_allocator::limited_allocator;
    use std::panic::UnwindSafe;
    use std::ptr;
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicPtr, Ordering};

    pub(crate) struct Node<T> {
        data: T,
        next: AtomicPtr<Node<T>>,
    }

    async fn allocate_node<T>(data: T) -> *mut Node<T> {
        let node_ptr = limited_allocator().allocate().await;

        unsafe {
            ptr::write(
                node_ptr,
                Node {
                    data,
                    next: AtomicPtr::new(null_mut()),
                },
            );
        };

        node_ptr
    }

    pub struct LockFreeStack<T, D: Fn(*mut Node<T>)> {
        head: AtomicPtr<Node<T>>,
        drop_fn: D,
    }

    impl<T, D: Fn(*mut Node<T>)> LockFreeStack<T, D> {
        pub fn new(drop_fn: D) -> Self {
            Self {
                head: AtomicPtr::new(null_mut()),
                drop_fn,
            }
        }

        #[allow(clippy::future_not_send, reason = "It is a test.")]
        pub async fn push(&self, data: T) {
            let new_node = allocate_node(data).await;
            let mut old_head = self.head.load(Ordering::Acquire);

            loop {
                *(unsafe { &mut *new_node }.next.get_mut()) = old_head;

                match self.head.compare_exchange(
                    old_head,
                    new_node,
                    Ordering::Release,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        break;
                    }
                    Err(current_head) => {
                        old_head = current_head;
                    }
                }
            }
        }

        pub fn pop(&self) -> Option<T> {
            let mut old_head = self.head.load(Ordering::Acquire);
            loop {
                if old_head.is_null() {
                    return None;
                }

                let node_ref = unsafe { &*old_head };
                let next = node_ref.next.load(Ordering::Acquire);

                match self.head.compare_exchange(
                    old_head,
                    next,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        let data = unsafe { ptr::read(&node_ref.data) };

                        (self.drop_fn)(old_head);

                        return Some(data);
                    }
                    Err(current_head) => {
                        old_head = current_head;
                    }
                }
            }
        }

        pub fn clear(&self) {
            while self.pop().is_some() {}
        }
    }

    impl<T, D: Fn(*mut Node<T>)> UnwindSafe for LockFreeStack<T, D> {}

    impl<T, D: Fn(*mut Node<T>)> Drop for LockFreeStack<T, D> {
        fn drop(&mut self) {
            self.clear();
        }
    }
}

const SIZE: usize = 128;

type StackForTests =
    lock_free_stack::LockFreeStack<[u8; SIZE], fn(*mut lock_free_stack::Node<[u8; SIZE]>)>;

#[cfg(test)]
async fn stress_test_epoch_gc_lock_free_stack<Creator>(creator: Creator)
where
    Creator: Fn() -> StackForTests,
{
    const TRIES: usize = 3;
    const PAR: usize = 2;
    #[cfg(not(debug_assertions))]
    const N: usize = 100_000;
    #[cfg(debug_assertions)]
    const N: usize = 20_000;
    const COUNT_PER_THREAD: usize = N / PAR;
    const TASKS: usize = 10;
    const COUNT_PER_TASK: usize = COUNT_PER_THREAD / TASKS;

    for _ in 0..TRIES {
        async fn work_with_stack(stack: Arc<StackForTests>, wg: Arc<WaitGroup>) {
            for _ in 0..TASKS {
                let wg = wg.clone();
                let stack = stack.clone();
                let stack_clone = stack.clone();
                let insert_remove_wg = Arc::new(WaitGroup::new_with_count(2));
                let insert_remove_wg_clone = insert_remove_wg.clone();

                local_executor().spawn_shared(async move {
                    for j in 0..COUNT_PER_TASK {
                        stack_clone.push([0u8; SIZE]).await;

                        if j % 100 == 0 {
                            yield_now().await;
                        }
                    }

                    insert_remove_wg_clone.done().await;
                });

                let insert_remove_wg_clone = insert_remove_wg.clone();

                local_executor().spawn_shared(async move {
                    for _ in 0..COUNT_PER_TASK {
                        while stack.pop().is_none() {
                            yield_now().await;
                        }
                    }

                    insert_remove_wg_clone.done().await;
                });

                insert_remove_wg.wait().await;

                wg.done().await;
            }
        }

        let stack = Arc::new(creator());
        let wg = Arc::new(WaitGroup::new());

        wg.add(PAR * TASKS).await;

        for _ in 0..PAR - 1 {
            let wg = wg.clone();
            let stack = stack.clone();

            sched_future(async move {
                work_with_stack(stack, wg).await;
            });
        }

        work_with_stack(stack, wg.clone()).await;

        wg.wait().await;
    }

    let start_epoch = local_epoch_gc().current_epoch();

    while local_epoch_gc().current_epoch() < start_epoch + 3 {
        yield_now().await;
    }
}

#[orengine::test::test_shared(timeout_ms = 10000, exclusive_in = "*")]
fn stress_test_epoch_gc_lock_free_stack_deallocate() {
    limited_allocator::limited_allocator().reset();

    stress_test_epoch_gc_lock_free_stack(|| {
        lock_free_stack::LockFreeStack::new(|ptr| unsafe {
            local_epoch_gc().schedule_deallocate(ptr);
        })
    })
    .await;
}

#[orengine::test::test_shared(timeout_ms = 10000, exclusive_in = "*")]
fn stress_test_epoch_gc_lock_free_stack_drop() {
    limited_allocator::limited_allocator().reset();

    stress_test_epoch_gc_lock_free_stack(|| {
        lock_free_stack::LockFreeStack::new(|ptr| unsafe {
            local_epoch_gc().schedule_drop(move || {
                local_epoch_gc().schedule_deallocate(ptr);
            });
        })
    })
    .await;
}

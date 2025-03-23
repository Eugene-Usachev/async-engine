use crate::local_executor;
use crate::runtime::task::task_data::TaskData;
use crate::runtime::{Locality, Task};
use ahash::AHashMap;
use std::future::Future;
use std::mem::size_of;

/// A pool of tasks.
#[derive(Default)]
pub(crate) struct TaskPool {
    /// Key is a size.
    storage: AHashMap<usize, Vec<Task>>,
}

impl TaskPool {
    /// Returns a [`Task`] with the given future.
    #[inline]
    pub(crate) fn acquire<F: Future<Output = ()>>(future: F, locality: Locality) -> Task {
        let executor = local_executor();
        let size = size_of::<F>();
        #[cfg(debug_assertions)]
        let executor_id = if cfg!(test) {
            usize::MAX
        } else {
            executor.id()
        };

        let pool = executor.task_pool().storage.entry(size).or_default();
        if let Some(mut task) = pool.pop() {
            let future_ptr: *mut F = task.future_ptr().cast();

            unsafe {
                future_ptr.write(future);

                task.data = TaskData::new(future_ptr as *mut _, locality);
            };

            #[cfg(debug_assertions)]
            unsafe {
                *task.is_executing.as_mut().get_mut() = false;

                task.executor_id = executor_id;
            }

            task
        } else {
            #[allow(unused_unsafe, reason = "False positive")]
            let future_ptr: *mut F = unsafe { &raw mut *(Box::into_raw(Box::new(future))) };
            Task {
                data: TaskData::new(future_ptr as *mut _, locality),
                #[cfg(debug_assertions)]
                executor_id,
                #[cfg(debug_assertions)]
                is_executing: crate::utils::Ptr::move_to_heap(std::sync::atomic::AtomicBool::new(
                    false,
                )),
            }
        }
    }

    /// Puts a task into the pool.
    #[inline]
    pub fn put(&mut self, task: Task) {
        let size = size_of_val(unsafe { &*task.future_ptr() });
        if let Some(pool) = self.storage.get_mut(&size) {
            pool.push(task);
            return;
        }

        // A task that have been allocated in another thread ended up here

        self.storage.insert(size, vec![task]);
    }
}

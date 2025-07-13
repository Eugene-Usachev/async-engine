// TODO docs

use crate::runtime::Task;
use crate::utils::{
    LockFreeTaskStack, NeverWaitLock, TaskVecFromPool, acquire_task_vec_from_pool, assert_hint,
    likely,
};
use std::mem::MaybeUninit;
use std::{ptr, thread};
// TODO impl 1
// pub struct SyncTaskList {
//     stack: LockFreeTaskStack,
// }
//
// impl SyncTaskList {
//     pub fn new() -> Self {
//         Self {
//             stack: LockFreeTaskStack::new(),
//         }
//     }
//
//     pub fn push(&self, task: Task) {
//         self.stack.push(task);
//     }
//
//     pub fn try_pop(&self) -> Option<Task> {
//         self.stack.try_pop()
//     }
//
//     pub unsafe fn busy_pop(&self) -> Task {
//         #[cfg(debug_assertions)]
//         let start = std::time::Instant::now();
//         let backoff = Backoff::new();
//
//         loop {
//             if let Some(task) = self.try_pop() {
//                 return task;
//             }
//
//             backoff.snooze();
//
//             #[cfg(debug_assertions)]
//             {
//                 if backoff.is_completed() && start.elapsed() > std::time::Duration::from_millis(100) {
//                     panic!("busy_pop timed out");
//                 }
//             }
//         }
//     }
//
//     pub unsafe fn busy_pop_many(&self, dst: &mut [MaybeUninit<Task>]) {
//         let mut to_pop = dst.len();
//         let backoff = Backoff::new();
//
//         while to_pop > 0 {
//             if let Some(task) = self.try_pop() {
//                 to_pop -= 1;
//
//                 dst[to_pop] = MaybeUninit::new(task);
//             }
//
//             backoff.spin();
//         }
//     }
//
//     pub fn try_pop_many(&self, dst: &mut [MaybeUninit<Task>]) -> usize {
//         let mut popped = 0;
//
//         while popped != dst.len() {
//             if let Some(task) = self.try_pop() {
//                 dst[popped] = MaybeUninit::new(task);
//
//                 popped += 1;
//             } else {
//                 break;
//             }
//         }
//
//         popped
//     }
// }

// TODO 2 impl
pub struct SyncTaskList {
    fast_list: NeverWaitLock<TaskVecFromPool>,
    slow_list: LockFreeTaskStack,
}

impl SyncTaskList {
    pub fn new() -> Self {
        Self {
            fast_list: NeverWaitLock::new(acquire_task_vec_from_pool()),
            slow_list: LockFreeTaskStack::new(),
        }
    }

    pub fn push(&self, task: Task) {
        if let Some(mut fast_list) = self.fast_list.try_lock_with_spinning() {
            fast_list.push(task);
        } else {
            self.slow_list.push(task);
        }
    }

    pub fn try_pop(&self) -> Option<Task> {
        self.fast_list
            .try_lock_with_spinning()
            .map_or_else(|| self.slow_list.try_pop(), |mut fast_list| fast_list.pop())
    }

    pub unsafe fn busy_pop(&self) -> Task {
        fn fast_list_starvation_case(this: &SyncTaskList) -> Task {
            println!("case 1"); // TODO r

            loop {
                for _ in 0..4 {
                    if let Some(mut fast_list) = this.fast_list.try_lock_with_spinning() {
                        if let Some(task) = fast_list.pop() {
                            return task;
                        }
                    }
                }

                if let Some(task) = this.slow_list.try_pop() {
                    return task;
                }

                // It looks like another thread has locked the fast_list and was preempted by OS.
                // Unlikely, but we have to handle it.

                thread::yield_now();
            }
        }

        let mut step = 0;

        loop {
            if let Some(task) = self.try_pop() {
                // with inner spinning
                return task;
            }

            if step == 2 {
                // Maybe we have fast_list starvation.
                // From here we can't correct it, but we can at least reduce slow_list polling.

                return fast_list_starvation_case(self);
            }

            step += 1;
        }
    }

    pub unsafe fn busy_pop_many(&self, dst: &mut [MaybeUninit<Task>]) {
        fn fast_list_starvation_case(
            this: &SyncTaskList,
            dst: &mut [MaybeUninit<Task>],
            mut popped: usize,
        ) {
            println!("case 2"); // TODO r

            loop {
                for _ in 0..4 {
                    if let Some(mut fast_list) = this.fast_list.try_lock_with_spinning() {
                        let fast_len = fast_list.len();
                        let want = dst.len() - popped;

                        if fast_len > 0 {
                            let take = fast_len.min(want);
                            let start = fast_len - take;

                            unsafe {
                                ptr::copy_nonoverlapping(
                                    fast_list.as_ptr().add(start),
                                    dst.as_mut_ptr().add(popped).cast(),
                                    take,
                                );
                                fast_list.set_len(start);
                            }

                            popped += take;

                            if popped == dst.len() {
                                return;
                            }
                        }
                    }
                }

                if let Some(task) = this.slow_list.try_pop() {
                    dst[popped] = MaybeUninit::new(task);
                    popped += 1;

                    if popped == dst.len() {
                        return;
                    }
                }

                // It's possible a thread is holding the lock and got preempted
                thread::yield_now();
            }
        }

        let mut step = 0;
        let mut popped = 0;

        loop {
            if let Some(mut fast_list) = self.fast_list.try_lock() {
                let fast_len = fast_list.len();
                let want = dst.len() - popped;

                if fast_len > 0 {
                    let take = fast_len.min(want);
                    let start = fast_len - take;

                    unsafe {
                        ptr::copy_nonoverlapping(
                            fast_list.as_ptr().add(start),
                            dst.as_mut_ptr().add(popped).cast(),
                            take,
                        );
                        fast_list.set_len(start);
                    }

                    popped += take;
                }
            }

            if likely(popped == dst.len()) {
                return;
            }

            assert_hint(dst.len() > popped, "dst.len() <= popped");

            if let Some(task) = self.slow_list.try_pop() {
                dst[popped] = MaybeUninit::new(task);

                popped += 1;
            }

            if likely(popped == dst.len()) {
                return;
            }

            if step == 2 {
                return fast_list_starvation_case(self, dst, popped);
            }

            step += 1;
        }
    }

    pub fn try_pop_many(&self, dst: &mut [MaybeUninit<Task>]) -> usize {
        assert_hint(!dst.is_empty(), "dst.len() == 0");

        let mut popped = 0;

        if let Some(mut fast_list) = self.fast_list.try_lock() {
            let fast_len = fast_list.len();
            let want = dst.len() - popped;

            if fast_len > 0 {
                let take = fast_len.min(want);
                let start = fast_len - take;

                unsafe {
                    ptr::copy_nonoverlapping(
                        fast_list.as_ptr().add(start),
                        dst.as_mut_ptr().add(popped).cast(),
                        take,
                    );
                    fast_list.set_len(start);
                }

                popped += take;

                if popped == dst.len() {
                    return popped;
                }
            }
        }

        crate::utils::hints::cold_path();

        // We can be here if the fast list is locked or if it doesn't have enough elements.
        // But this method should be called in loop, so we can try to pop some elements from
        // the slow list and return in any case.

        assert_hint(dst.len() > popped, "dst.len() <= popped");

        if let Some(task) = self.slow_list.try_pop() {
            dst[popped] = MaybeUninit::new(task);
            popped += 1;
        }

        popped
    }
}

impl Default for SyncTaskList {
    fn default() -> Self {
        Self::new()
    }
}

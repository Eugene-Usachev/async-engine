use crate::bug_message::BUG_MESSAGE;
#[cfg(test)]
use crate::runtime::Task;
use crate::runtime::epoch_gc::{Deferred, GLOBAL_EPOCH_GC};
use crate::runtime::get_local_executor_ref;
// TODO docs
use crate::utils::{
    LockFreeStackNode, OrengineInstant, clear_with, likely, unlikely, unwrap_or_bug_hint,
};
use std::alloc::{Layout, dealloc};
use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Release;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::{mem, thread};

thread_local! {
    static IS_DEREGISTERING: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
}

thread_local! {
    static LOCK_FREE_STACK_NODE_POOL: UnsafeCell<Vec<*mut LockFreeStackNode>> = const { UnsafeCell::new(Vec::new()) };
}

fn get_lock_free_stack_node_pool() -> &'static mut Vec<*mut LockFreeStackNode> {
    LOCK_FREE_STACK_NODE_POOL.with(|p| unsafe { &mut *p.get() })
}

struct Storage {
    to_deallocate: Vec<(*mut u8, Layout)>,
    to_drop: Vec<Deferred>,
    to_reuse_node: Vec<*mut LockFreeStackNode>,
}

impl Storage {
    const fn new() -> Self {
        Self {
            to_deallocate: Vec::new(),
            to_drop: Vec::new(),
            to_reuse_node: Vec::new(),
        }
    }

    fn clear(&mut self) {
        clear_with(&mut self.to_deallocate, |(ptr, layout)| unsafe {
            #[cfg(test)]
            {
                DEALLOCATED_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            }

            dealloc(ptr, layout);
        });

        clear_with(&mut self.to_drop, |f| {
            f.call();
        });

        clear_with(&mut self.to_reuse_node, |ptr| {
            get_lock_free_stack_node_pool().push(ptr);
        });
    }

    fn append(&mut self, other: &mut Self) {
        self.to_deallocate.append(&mut other.to_deallocate);
        self.to_drop.append(&mut other.to_drop);
        self.to_reuse_node.append(&mut other.to_reuse_node);
    }
}

#[allow(
    clippy::non_send_fields_in_send_ty,
    reason = "We guarantee that it is `Send`"
)]
unsafe impl Send for Storage {}
unsafe impl Sync for Storage {}

#[cfg(test)]
static DEALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);

pub struct EpochGCLocalManager {
    current_epoch: usize,
    this_epoch_start: OrengineInstant,
    was_passed_epoch: bool,

    prev_storage: Storage,
    current_storage: Storage,

    #[cfg(test)]
    prev_waiting_tasks_for_new_epoch: Vec<Task>,
    #[cfg(test)]
    current_waiting_tasks_for_new_epoch: Vec<Task>,
}

impl EpochGCLocalManager {
    pub(crate) fn from_current_epoch(current_epoch: usize) -> Self {
        IS_DEREGISTERING.with(|is_deregistering| {
            while is_deregistering.load(Ordering::Acquire) {
                thread::sleep(Duration::from_micros(100));
            }
        });

        Self {
            current_epoch,
            this_epoch_start: unsafe { MaybeUninit::zeroed().assume_init() },
            was_passed_epoch: false,

            prev_storage: Storage::new(),
            current_storage: Storage::new(),

            #[cfg(test)]
            prev_waiting_tasks_for_new_epoch: Vec::new(),
            #[cfg(test)]
            current_waiting_tasks_for_new_epoch: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn bytes_deallocated() -> usize {
        DEALLOCATED_BYTES.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) async fn wait_new_epoch(&mut self) {
        let task = unsafe { Task::get_current().await };

        self.current_waiting_tasks_for_new_epoch.push(task);

        unsafe { Task::park_current_task().await };
    }

    pub fn current_epoch(&self) -> usize {
        self.current_epoch
    }

    pub unsafe fn schedule_deallocate<T>(&mut self, ptr: *const T) {
        self.current_storage
            .to_deallocate
            .push((ptr.cast::<u8>().cast_mut(), Layout::new::<T>()));
    }

    pub unsafe fn schedule_deallocate_slice<T>(&mut self, ptr: *const T, len: usize) {
        self.current_storage.to_deallocate.push((
            ptr.cast::<u8>().cast_mut(),
            Layout::array::<T>(len).unwrap(),
        ));
    }

    pub unsafe fn schedule_drop<F: FnOnce()>(&mut self, func: F) {
        self.current_storage.to_drop.push(Deferred::new(func));
    }

    pub(crate) unsafe fn schedule_push_to_node_pool(&mut self, node: *mut LockFreeStackNode) {
        self.current_storage.to_reuse_node.push(node);
    }

    #[allow(
        clippy::unused_self,
        reason = "This behavior can be changed in the future"
    )]
    pub(crate) fn get_task_node(&self) -> Option<*mut LockFreeStackNode> {
        get_lock_free_stack_node_pool().pop()
    }

    pub(crate) fn collect_garbage(&mut self) {
        self.prev_storage.clear();
    }

    fn react_to_epoch_change(&mut self, global_epoch: usize, now: OrengineInstant) {
        debug_assert_eq!(global_epoch, self.current_epoch + 1);

        self.current_epoch = global_epoch;
        self.this_epoch_start = now;
        self.was_passed_epoch = false;

        self.collect_garbage();

        #[cfg(test)]
        {
            clear_with(&mut self.prev_waiting_tasks_for_new_epoch, |task| {
                crate::local_executor().exec_task(task);
            });

            mem::swap(
                &mut self.prev_waiting_tasks_for_new_epoch,
                &mut self.current_waiting_tasks_for_new_epoch,
            );
        }
    }

    pub(crate) fn maybe_pass_epoch(&mut self, now: OrengineInstant) {
        #[cfg(not(test))]
        const EXPECTED_EPOCH_DURATION: Duration = Duration::from_millis(10);

        #[cfg(test)]
        const EXPECTED_EPOCH_DURATION: Duration = Duration::from_micros(100);

        if likely(now - self.this_epoch_start < EXPECTED_EPOCH_DURATION) {
            return;
        }

        let global_epoch = GLOBAL_EPOCH_GC.current_epoch();

        if unlikely(self.current_epoch < global_epoch) {
            debug_assert!(self.was_passed_epoch);

            self.react_to_epoch_change(global_epoch, now);

            return;
        }

        debug_assert_eq!(self.current_epoch, global_epoch);

        if likely(self.was_passed_epoch) {
            return;
        }

        self.was_passed_epoch = true;

        debug_assert!(
            self.prev_storage.to_drop.is_empty() && self.prev_storage.to_deallocate.is_empty()
        );
        mem::swap(&mut self.prev_storage, &mut self.current_storage);

        let was_changed = GLOBAL_EPOCH_GC.executor_passed_epoch();
        if unlikely(was_changed) {
            self.react_to_epoch_change(global_epoch + 1, now);
        }
    }

    pub(crate) unsafe fn deregister(&mut self) {
        struct DeregisterInNewEpochArgs {
            epoch_at_start: usize,
            storage: Storage,
            is_already_in_new_thread: bool,
            is_deregistering: Arc<AtomicBool>,
        }

        fn deregister_in_new_epoch(mut args: DeregisterInNewEpochArgs) {
            fn wait_new_epoch_and_clear(mut storage: Storage, epoch_at_start: usize) {
                while GLOBAL_EPOCH_GC.current_epoch() == epoch_at_start {
                    thread::sleep(Duration::from_millis(1));
                }

                storage.clear();
            }

            let is_new_epoch = GLOBAL_EPOCH_GC.deregister_executor();

            if is_new_epoch {
                debug_assert_ne!(args.epoch_at_start, GLOBAL_EPOCH_GC.current_epoch());

                // The executor passed the current epoch and has been deregistered
                args.storage.clear();

                args.is_deregistering.store(false, Release);

                return;
            }

            if args.is_already_in_new_thread {
                wait_new_epoch_and_clear(args.storage, args.epoch_at_start);

                args.is_deregistering.store(false, Release);
            } else {
                thread::spawn(move || {
                    wait_new_epoch_and_clear(args.storage, args.epoch_at_start);

                    args.is_deregistering.store(false, Release);
                });
            }
        }

        let epoch_at_start = self.current_epoch;
        let is_deregistering = IS_DEREGISTERING.with(|is_deregistering| is_deregistering.clone());
        let mut full_storage = mem::replace(&mut self.current_storage, Storage::new());

        full_storage.append(&mut self.prev_storage);

        // Maybe we still have not passed the current epoch
        if !self.was_passed_epoch {
            deregister_in_new_epoch(DeregisterInNewEpochArgs {
                epoch_at_start,
                storage: full_storage,
                is_already_in_new_thread: false,
                is_deregistering,
            });

            LOCAL_MANAGER.with(|local_manager_| unsafe {
                *local_manager_.get() = None;
            });

            return;
        }

        let mut args = DeregisterInNewEpochArgs {
            epoch_at_start,
            storage: full_storage,
            is_already_in_new_thread: true,
            is_deregistering,
        };

        LOCAL_MANAGER.with(|local_manager_| unsafe {
            *local_manager_.get() = None;
        });

        thread::spawn(move || {
            while GLOBAL_EPOCH_GC.current_epoch() == args.epoch_at_start {
                thread::sleep(Duration::from_millis(1));
            }

            args.epoch_at_start += 1;

            deregister_in_new_epoch(args);
        });
    }
}

thread_local! {
    static LOCAL_MANAGER: UnsafeCell<Option<EpochGCLocalManager>> = const { UnsafeCell::new(None) };
}

pub(crate) fn register_local_epoch_gc() {
    LOCAL_MANAGER.with(|local_manager_| {
        let local_manager = unsafe { &mut *local_manager_.get() };

        assert!(
            local_manager
                .replace(GLOBAL_EPOCH_GC.register_new_executor())
                .is_none(),
            "{BUG_MESSAGE}"
        );
    });
}

pub fn local_epoch_gc() -> &'static mut EpochGCLocalManager {
    debug_assert!(
        get_local_executor_ref().is_some(),
        "Executor should be registered in a thread that uses epoch GC"
    );

    LOCAL_MANAGER
        .with(|local_manager| unsafe { unwrap_or_bug_hint((*local_manager.get()).as_mut()) })
}

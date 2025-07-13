// TODO docs

use crate::runtime::Task;
use crate::runtime::epoch_gc::local_epoch_gc;
use std::panic::UnwindSafe;
use std::ptr;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};

pub(crate) struct LockFreeStackNode {
    task: Task,
    next: AtomicPtr<LockFreeStackNode>,
}

fn get_node(task: Task) -> *mut LockFreeStackNode {
    if let Some(node) = local_epoch_gc().get_task_node() {
        unsafe {
            ptr::write(&mut (*node).task, task);
        }

        node
    } else {
        Box::into_raw(Box::new(LockFreeStackNode {
            task,
            next: AtomicPtr::new(null_mut()),
        }))
    }
}

pub struct LockFreeTaskStack {
    head: AtomicPtr<LockFreeStackNode>,
}

impl LockFreeTaskStack {
    pub const fn new() -> Self {
        Self {
            head: AtomicPtr::new(null_mut()),
        }
    }

    #[allow(clippy::future_not_send, reason = "It is a test.")]
    pub fn push(&self, task: Task) {
        let new_node = get_node(task);
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

    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire).is_null()
    }

    pub fn try_pop(&self) -> Option<Task> {
        let mut old_head = self.head.load(Ordering::Acquire);

        loop {
            if old_head.is_null() {
                return None;
            }

            let node_ref = unsafe { &*old_head };
            let next = node_ref.next.load(Ordering::Acquire);

            match self
                .head
                .compare_exchange(old_head, next, Ordering::Acquire, Ordering::Relaxed)
            {
                Ok(_) => {
                    let data = unsafe { ptr::read(&node_ref.task) };

                    unsafe { local_epoch_gc().schedule_push_to_node_pool(old_head) };

                    return Some(data);
                }
                Err(current_head) => {
                    old_head = current_head;
                }
            }
        }
    }

    pub fn clear(&self) {
        while self.try_pop().is_some() {}
    }
}

impl Default for LockFreeTaskStack {
    fn default() -> Self {
        Self::new()
    }
}

impl UnwindSafe for LockFreeTaskStack {}

impl Drop for LockFreeTaskStack {
    fn drop(&mut self) {
        if cfg!(debug_assertions) {
            assert!(
                self.is_empty(),
                "LockFreeTaskStack must be empty when dropped to avoid task leaks."
            );
        }
    }
}

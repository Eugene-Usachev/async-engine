//! Forked [`SegQueue`](crossbeam::queue::SegQueue) from the `crossbeam-queue` crate
//! but modified:
//!
//! - It reuses the memory of destroyed blocks.
//! - It doesn't use [`CachePadded`](crossbeam::utils::CachePadded)
//!   to avoid false sharing, because [`SegTaskQueue`] is used as a fallback when
//!   a [`NeverWaitLock`](crate::utils::never_wait_lock::NeverWaitLock) is locked.
//! - The [`LAP`] is set to 3 to reduce memory usage because of the same reason as above.
use crate::runtime::Task;
use crate::utils::Backoff;
use crate::utils::unwrap_or_bug_hint;
use core::cell::UnsafeCell;
use core::fmt;
use core::mem::MaybeUninit;
use core::panic::{RefUnwindSafe, UnwindSafe};
use core::ptr;
use core::sync::atomic::{self, AtomicPtr, AtomicUsize, Ordering};
use std::alloc::{Layout, alloc_zeroed};

// Bits indicating the state of a slot:
// * If a value has been written into the slot, `WRITE` is set.
// * If a value has been read from the slot, `READ` is set.
// * If the block is being destroyed, `DESTROY` is set.
const WRITE: usize = 1;
const READ: usize = 2;
const DESTROY: usize = 4;

// Each block covers one "lap" of indices.
const LAP: usize = 3;
// The maximum number of values a block can hold.
const BLOCK_CAP: usize = LAP - 1;
// Indicates how many lower bits are reserved for metadata.
const SHIFT: usize = 1;
// Indicates that the block is not the last one.
const HAS_NEXT: usize = 1;

/// A slot in a block.
struct Slot {
    /// The value.
    value: UnsafeCell<MaybeUninit<Task>>,

    /// The state of the slot.
    state: AtomicUsize,
}

impl Slot {
    /// Waits until a value is written into the slot.
    fn wait_write(&self) {
        let backoff = Backoff::new();
        while self.state.load(Ordering::Acquire) & WRITE == 0 {
            backoff.snooze();
        }
    }
}

/// A block in a linked list.
///
/// Each block in the list can hold up to `BLOCK_CAP` values.
struct Block {
    /// The next block in the linked list.
    next: AtomicPtr<Block>,

    /// Slots for values.
    slots: [Slot; BLOCK_CAP],
}

thread_local! {
    static BLOCK_POOL: UnsafeCell<Vec<*mut Block>> = const { UnsafeCell::new(Vec::new()) };
}

fn block_pool() -> &'static mut Vec<*mut Block> {
    BLOCK_POOL.with(|p| unsafe { &mut *p.get() })
}

fn get_block_from_pool() -> *mut Block {
    /// Creates an empty block.
    fn new_block() -> *mut Block {
        unsafe { alloc_zeroed(Block::LAYOUT) }.cast()
    }

    block_pool().pop().unwrap_or_else(new_block).cast()
}

impl Block {
    const LAYOUT: Layout = {
        let layout = Layout::new::<Self>();

        assert!(
            layout.size() != 0,
            "Block should never be zero-sized, as it has an AtomicPtr field"
        );

        layout
    };

    /// Waits until the next pointer is set.
    fn wait_next(&self) -> *mut Self {
        let backoff = Backoff::new();
        loop {
            let next = self.next.load(Ordering::Acquire);
            if !next.is_null() {
                return next;
            }
            backoff.snooze();
        }
    }

    /// Sets the `DESTROY` bit in slots starting from `start` and destroys the block.
    unsafe fn destroy(this: *mut Self, start: usize) {
        // It is not necessary to set the `DESTROY` bit in the last slot because that slot has
        // begun destruction of the block.
        for i in start..BLOCK_CAP - 1 {
            let slot = unsafe { &(*this).slots.get_unchecked(i) };

            // Mark the `DESTROY` bit if a thread is still using the slot.
            if slot.state.load(Ordering::Acquire) & READ == 0
                && slot.state.fetch_or(DESTROY, Ordering::AcqRel) & READ == 0
            {
                // If a thread is still using the slot, it will continue destruction of the block.
                return;
            }
        }

        // No thread is using the block, now it is safe to destroy it.

        let pool = block_pool();
        if pool.len() < 32 * 1024 * 1024 * 1024 / size_of::<Self>() {
            block_pool().push(this);

            return;
        }

        unsafe { drop(Box::from_raw(this)) };
    }
}

/// A position in a queue.
struct Position {
    /// The index in the queue.
    index: AtomicUsize,

    /// The block in the linked list.
    block: AtomicPtr<Block>,
}

/// Read `crossbeam_queue::SegQueue` for more details.
pub struct SegTaskQueue {
    /// The head of the queue.
    head: Position,

    /// The tail of the queue.
    tail: Position,
}

unsafe impl Send for SegTaskQueue {}
unsafe impl Sync for SegTaskQueue {}

impl UnwindSafe for SegTaskQueue {}
impl RefUnwindSafe for SegTaskQueue {}

impl SegTaskQueue {
    /// Read `crossbeam_queue::SegQueue` for more details.
    pub const fn new() -> Self {
        Self {
            head: Position {
                block: AtomicPtr::new(ptr::null_mut()),
                index: AtomicUsize::new(0),
            },
            tail: Position {
                block: AtomicPtr::new(ptr::null_mut()),
                index: AtomicUsize::new(0),
            },
        }
    }

    /// Read `crossbeam_queue::SegQueue` for more details.
    pub fn push(&self, value: Task) {
        let backoff = Backoff::new();
        let mut tail = self.tail.index.load(Ordering::Acquire);
        let mut block = self.tail.block.load(Ordering::Acquire);
        let mut next_block = None;

        loop {
            // Calculate the offset of the index into the block.
            let offset = (tail >> SHIFT) % LAP;

            // If we reached the end of the block, wait until the next one is installed.
            if offset == BLOCK_CAP {
                backoff.snooze();

                tail = self.tail.index.load(Ordering::Acquire);
                block = self.tail.block.load(Ordering::Acquire);

                continue;
            }

            // If we're going to have to install the next block, allocate it in advance to
            // make the wait for other threads as short as possible.
            if offset + 1 == BLOCK_CAP && next_block.is_none() {
                next_block = Some(get_block_from_pool());
            }

            // If this is the first push operation, we need to allocate the first block.
            if block.is_null() {
                let new = get_block_from_pool();

                if self
                    .tail
                    .block
                    .compare_exchange(block, new, Ordering::Release, Ordering::Relaxed)
                    .is_ok()
                {
                    self.head.block.store(new, Ordering::Release);
                    block = new;
                } else {
                    next_block = Some(new);
                    tail = self.tail.index.load(Ordering::Acquire);
                    block = self.tail.block.load(Ordering::Acquire);

                    continue;
                }
            }

            let new_tail = tail + (1 << SHIFT);

            // Try advancing the tail forward.
            match self.tail.index.compare_exchange_weak(
                tail,
                new_tail,
                Ordering::SeqCst,
                Ordering::Acquire,
            ) {
                Ok(_) => unsafe {
                    // If we've reached the end of the block, install the next one.
                    if offset + 1 == BLOCK_CAP {
                        let next_block = unwrap_or_bug_hint(next_block);
                        let next_index = new_tail.wrapping_add(1 << SHIFT);

                        self.tail.block.store(next_block, Ordering::Release);
                        self.tail.index.store(next_index, Ordering::Release);
                        (*block).next.store(next_block, Ordering::Release);
                    }

                    // Write the value into the slot.
                    let slot = (*block).slots.get_unchecked(offset);
                    slot.value.get().write(MaybeUninit::new(value));
                    slot.state.fetch_or(WRITE, Ordering::Release);

                    return;
                },
                Err(t) => {
                    tail = t;
                    block = self.tail.block.load(Ordering::Acquire);
                    backoff.snooze();
                }
            }
        }
    }

    /// Read `crossbeam_queue::SegQueue` for more details.
    pub fn pop(&self) -> Option<Task> {
        let backoff = Backoff::new();
        let mut head = self.head.index.load(Ordering::Acquire);
        let mut block = self.head.block.load(Ordering::Acquire);

        loop {
            // Calculate the offset of the index into the block.
            let offset = (head >> SHIFT) % LAP;

            // If we reached the end of the block, wait until the next one is installed.
            if offset == BLOCK_CAP {
                backoff.snooze();
                head = self.head.index.load(Ordering::Acquire);
                block = self.head.block.load(Ordering::Acquire);
                continue;
            }

            let mut new_head = head + (1 << SHIFT);

            if new_head & HAS_NEXT == 0 {
                atomic::fence(Ordering::SeqCst);
                let tail = self.tail.index.load(Ordering::Relaxed);

                // If the tail equals the head, that means the queue is empty.
                if head >> SHIFT == tail >> SHIFT {
                    return None;
                }

                // If head and tail are not in the same block, set `HAS_NEXT` in the head.
                if (head >> SHIFT) / LAP != (tail >> SHIFT) / LAP {
                    new_head |= HAS_NEXT;
                }
            }

            // The block can be null here only if the first push operation is in progress. In that
            // case, wait until it gets initialized.
            if block.is_null() {
                backoff.snooze();
                head = self.head.index.load(Ordering::Acquire);
                block = self.head.block.load(Ordering::Acquire);
                continue;
            }

            // Try moving the head index forward.
            match self.head.index.compare_exchange_weak(
                head,
                new_head,
                Ordering::SeqCst,
                Ordering::Acquire,
            ) {
                Ok(_) => unsafe {
                    // If we've reached the end of the block, move to the next one.
                    if offset + 1 == BLOCK_CAP {
                        let next = (*block).wait_next();
                        let mut next_index = (new_head & !HAS_NEXT).wrapping_add(1 << SHIFT);
                        if !(*next).next.load(Ordering::Relaxed).is_null() {
                            next_index |= HAS_NEXT;
                        }

                        self.head.block.store(next, Ordering::Release);
                        self.head.index.store(next_index, Ordering::Release);
                    }

                    // Read the value.
                    let slot = (*block).slots.get_unchecked(offset);
                    slot.wait_write();
                    let value = slot.value.get().read().assume_init();

                    // Destroy the block if we've reached the end or if another thread wanted to
                    // destroy but couldn't because we were busy reading from the slot.
                    if offset + 1 == BLOCK_CAP {
                        Block::destroy(block, 0);
                    } else if slot.state.fetch_or(READ, Ordering::AcqRel) & DESTROY != 0 {
                        Block::destroy(block, offset + 1);
                    }

                    return Some(value);
                },
                Err(h) => {
                    head = h;
                    block = self.head.block.load(Ordering::Acquire);
                    backoff.snooze();
                }
            }
        }
    }

    /// Read `crossbeam_queue::SegQueue` for more details.
    pub fn is_empty(&self) -> bool {
        let head = self.head.index.load(Ordering::SeqCst);
        let tail = self.tail.index.load(Ordering::SeqCst);

        head >> SHIFT == tail >> SHIFT
    }

    /// Read `crossbeam_queue::SegQueue` for more details.
    pub fn len(&self) -> usize {
        loop {
            // Load the tail index, then load the head index.
            let mut tail = self.tail.index.load(Ordering::SeqCst);
            let mut head = self.head.index.load(Ordering::SeqCst);

            // If the tail index didn't change, we've got consistent indices to work with.
            if self.tail.index.load(Ordering::SeqCst) == tail {
                // Erase the lower bits.
                tail &= !((1 << SHIFT) - 1);
                head &= !((1 << SHIFT) - 1);

                // Fix up indices if they fall onto block ends.
                if (tail >> SHIFT) & (LAP - 1) == LAP - 1 {
                    tail = tail.wrapping_add(1 << SHIFT);
                }
                if (head >> SHIFT) & (LAP - 1) == LAP - 1 {
                    head = head.wrapping_add(1 << SHIFT);
                }

                // Rotate indices so that the head falls into the first block.
                let lap = (head >> SHIFT) / LAP;
                tail = tail.wrapping_sub((lap * LAP) << SHIFT);
                head = head.wrapping_sub((lap * LAP) << SHIFT);

                // Remove the lower bits.
                tail >>= SHIFT;
                head >>= SHIFT;

                // Return the difference minus the number of blocks between tail and head.
                return tail - head - tail / LAP;
            }
        }
    }
}

impl Drop for SegTaskQueue {
    fn drop(&mut self) {
        if cfg!(debug_assertions) {
            assert!(
                self.is_empty(),
                "SegQueue must be empty when dropped to avoid task leaks."
            );
        }
    }
}

impl fmt::Debug for SegTaskQueue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad("SegQueue { .. }")
    }
}

impl Default for SegTaskQueue {
    fn default() -> Self {
        Self::new()
    }
}

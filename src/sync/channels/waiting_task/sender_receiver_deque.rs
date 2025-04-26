// TODO docs

use crate::sync::channels::waiting_task::waiting_task::WaitingTask;
use std::alloc::{alloc, dealloc, Layout};
use std::ptr;
use std::ptr::NonNull;

const RECEIVER_DELTA: isize = 1;
const SENDER_DELTA: isize = -1;

#[repr(C)]
pub(crate) struct SenderReceiverQueue<T = ()> {
    ptr: NonNull<WaitingTask<T>>,
    capacity: usize,
    /// __0__ for none, __>0__ for receivers, __<0__ for senders
    number_of_senders_or_receivers: isize,
    head: usize,
}

#[derive(Eq, PartialEq)]
pub(crate) enum SenderReceiverQueueOption {
    Empty,
    Sender,
    Receiver,
}

impl<T> SenderReceiverQueue<T> {
    #[inline(always)]
    fn new_layout_for_ptr(capacity: usize) -> Layout {
        unsafe {
            Layout::from_size_align_unchecked(
                size_of::<WaitingTask<T>>() * capacity,
                align_of::<WaitingTask<T>>(),
            )
        }
    }

    fn new_ptr(capacity: usize) -> NonNull<WaitingTask<T>> {
        let ptr = unsafe { alloc(Self::new_layout_for_ptr(capacity)) };

        unsafe { NonNull::new_unchecked(ptr.cast()) }
    }

    fn deallocate_ptr(ptr: NonNull<WaitingTask<T>>, capacity: usize) {
        unsafe {
            dealloc(ptr.as_ptr().cast(), Self::new_layout_for_ptr(capacity));
        }
    }

    pub(crate) fn new() -> Self {
        const DEFAULT_CAP: usize = 2;

        Self {
            ptr: Self::new_ptr(DEFAULT_CAP),
            capacity: DEFAULT_CAP,
            number_of_senders_or_receivers: 0,
            head: 0,
        }
    }

    #[inline]
    pub(crate) fn option(&self) -> SenderReceiverQueueOption {
        match self.number_of_senders_or_receivers.cmp(&0) {
            std::cmp::Ordering::Equal => SenderReceiverQueueOption::Empty,
            std::cmp::Ordering::Greater => SenderReceiverQueueOption::Receiver,
            std::cmp::Ordering::Less => SenderReceiverQueueOption::Sender,
        }
    }

    #[inline]
    pub(crate) fn number_of_senders_or_receivers(&self) -> isize {
        self.number_of_senders_or_receivers
    }

    #[inline]
    pub(crate) fn capacity(&self) -> usize {
        self.capacity
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.number_of_senders_or_receivers.unsigned_abs()
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.number_of_senders_or_receivers == 0
    }

    #[inline]
    fn to_physical_idx(&self, idx: usize) -> usize {
        let logical_index = self.head + idx;

        debug_assert!(
            logical_index < self.capacity || (logical_index - self.capacity) < self.capacity
        );
        if logical_index >= self.capacity {
            logical_index - self.capacity
        } else {
            logical_index
        }
    }

    fn push_back<const DELTA: isize>(&mut self, task: WaitingTask<T>) {
        let len = self.len();

        if len < self.capacity {
            unsafe { self.ptr.add(self.to_physical_idx(len)).write(task) };
            self.number_of_senders_or_receivers += DELTA;

            return;
        }

        self.capacity = {
            debug_assert_ne!(self.capacity, 0);

            match self.capacity {
                ..128 => self.capacity * 2,
                128..1024 => self.capacity * 3 / 2,
                1024..4096 => self.capacity * 5 / 4,
                _ => self.capacity * 8 / 7,
            }
        };
        let old_ptr = self.ptr.as_ptr();
        self.ptr = Self::new_ptr(self.capacity);

        // H = head
        // T = tail
        // From any of:
        //    H              T
        // 1: [o o o o o o o o ]
        //         T H
        // 2: [o o o o o o o o ]
        //
        // To:
        //    H             T
        //   [o o o o o o o o . . . ]

        // `self.head` is an old head, `self.ptr` is a new ptr, `self.capacity` is a new capacity.

        if self.head == 0 {
            // 1
            // We can just copy from start to end
            unsafe {
                ptr::copy_nonoverlapping(old_ptr, self.ptr.as_ptr(), len);
            }
        } else {
            // 2
            // We need to copy from head to end first, then from start to head
            unsafe {
                ptr::copy_nonoverlapping(
                    old_ptr.add(self.head),
                    self.ptr.as_ptr(),
                    len - self.head,
                );
                ptr::copy_nonoverlapping(
                    old_ptr,
                    self.ptr.as_ptr().add(len - self.head),
                    self.head,
                );
            }
        }

        self.head = 0;

        // Here all data starts at `self.ptr`, so we can write new data after.

        unsafe {
            self.ptr.add(len).write(task);
        }

        self.number_of_senders_or_receivers += DELTA;

        Self::deallocate_ptr(unsafe { NonNull::new_unchecked(old_ptr) }, len);
    }

    pub(crate) fn push_receiver(&mut self, task: WaitingTask<T>) {
        debug_assert!(self.number_of_senders_or_receivers > -1);

        self.push_back::<RECEIVER_DELTA>(task);
    }

    pub(crate) fn push_sender(&mut self, task: WaitingTask<T>) {
        debug_assert!(self.number_of_senders_or_receivers < 1);

        self.push_back::<SENDER_DELTA>(task);
    }

    unsafe fn pop_front<const DELTA: isize>(&mut self) -> WaitingTask<T> {
        debug_assert!(!self.is_empty());
        let old_head = self.head;
        let len = self.len();

        self.head = self.to_physical_idx(1);
        self.number_of_senders_or_receivers -= DELTA;

        let res = unsafe { self.ptr.add(old_head).read() };
        let must_shrink = (len * 3 < self.capacity) && len > 4;

        if !must_shrink {
            return res;
        }

        let old_ptr = self.ptr.as_ptr();
        let old_capacity = self.capacity;
        let tail = self.to_physical_idx(len - 1);
        self.capacity = (self.capacity >> 1) + 2;
        self.ptr = Self::new_ptr(self.capacity);

        if self.head < tail {
            unsafe {
                ptr::copy_nonoverlapping(
                    old_ptr.add(self.head),
                    self.ptr.as_ptr(),
                    tail - self.head,
                );
            }
        } else {
            unsafe {
                ptr::copy_nonoverlapping(
                    old_ptr.add(self.head),
                    self.ptr.as_ptr(),
                    old_capacity - self.head,
                );
                ptr::copy_nonoverlapping(
                    old_ptr,
                    self.ptr.as_ptr().add(old_capacity - self.head),
                    tail,
                );
            }
        }

        self.head = 0;

        Self::deallocate_ptr(unsafe { NonNull::new_unchecked(old_ptr) }, old_capacity);

        res
    }

    pub(crate) fn pop_receiver(&mut self) -> Option<WaitingTask<T>> {
        if self.number_of_senders_or_receivers < 1 {
            None
        } else {
            Some(unsafe { self.pop_front::<RECEIVER_DELTA>() })
        }
    }

    pub(crate) fn pop_sender(&mut self) -> Option<WaitingTask<T>> {
        if self.number_of_senders_or_receivers > -1 {
            None
        } else {
            Some(unsafe { self.pop_front::<SENDER_DELTA>() })
        }
    }
}

impl<T> Drop for SenderReceiverQueue<T> {
    fn drop(&mut self) {
        debug_assert_eq!(self.number_of_senders_or_receivers, 0);

        Self::deallocate_ptr(self.ptr, self.capacity);
    }
}

// TODO docs

use crate::sync::channels::waiting_task::waiting_task::WaitingTask;
use std::alloc::{alloc, dealloc, Layout};
use std::ptr;
use std::ptr::NonNull;

const RECEIVER_DELTA: isize = 1;
const SENDER_DELTA: isize = -1;

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
    fn new_ptr(capacity: usize) -> NonNull<WaitingTask<T>> {
        let ptr = unsafe {
            alloc(Layout::from_size_align_unchecked(
                size_of::<WaitingTask<T>>() * capacity,
                align_of::<WaitingTask<T>>(),
            ))
        };

        unsafe { NonNull::new_unchecked(ptr.cast()) }
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
        //    H              L
        // 1: [o o o o o o o o ]
        //         L H
        // 2: [o o o o o o o o ]
        //
        // To:
        //    H             L
        //   [o o o o o o o o . . . ]

        // Here self.head is an old head, self.ptr is a new ptr, self.capacity is a new capacity.

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

        unsafe { Box::from_raw(old_ptr) };

        self.head = 0;

        // Here all data starts at `self.ptr`, so we can write new data after.

        unsafe {
            self.ptr.add(len).write(task);
        }

        self.number_of_senders_or_receivers += DELTA;
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

        self.head = self.to_physical_idx(1);
        self.number_of_senders_or_receivers -= DELTA;

        unsafe { self.ptr.add(old_head).read() }
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

        unsafe {
            dealloc(
                self.ptr.as_ptr().cast(),
                Layout::from_size_align_unchecked(
                    size_of::<WaitingTask<T>>() * self.capacity,
                    align_of::<WaitingTask<T>>(),
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SenderReceiverQueue, SenderReceiverQueueOption};
    use crate as orengine;
    use crate::sync::channels::waiting_task::waiting_task::WaitingTask;

    #[orengine::test::test_local]
    fn test_sender_receiver_queue() {
        const N: usize = 10_000;

        let mut queue = SenderReceiverQueue::<usize>::new();

        for i in 0..N {
            queue.push_sender(WaitingTask::new_with_usize_for_tests(i));
        }

        assert!(matches!(queue.option(), SenderReceiverQueueOption::Sender));

        for i in 0..N {
            assert_eq!(queue.pop_sender().unwrap().extract_usize_for_tests(), i);
        }

        assert!(queue.pop_sender().is_none());

        for i in 0..N {
            queue.push_receiver(WaitingTask::new_with_usize_for_tests(i));
        }

        assert!(matches!(
            queue.option(),
            SenderReceiverQueueOption::Receiver
        ));

        for i in 0..N {
            assert_eq!(queue.pop_receiver().unwrap().extract_usize_for_tests(), i);
        }

        assert!(queue.pop_receiver().is_none());

        assert!(matches!(queue.option(), SenderReceiverQueueOption::Empty));
    }
}

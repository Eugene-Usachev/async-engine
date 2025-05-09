// TODO docs

use crate::sync::channels::waiting_task::waiting_task::WaitingTask;
use crate::utils::{assert_hint, likely, unlikely};
use std::alloc::{Layout, alloc, dealloc};
use std::ops::{Range, RangeBounds};
use std::ptr::NonNull;
use std::{ops, ptr};

const RECEIVER_DELTA: isize = 1;
const SENDER_DELTA: isize = -1;

#[repr(C)]
pub(crate) struct SenderReceiverQueue<T = ()> {
    ptr: NonNull<WaitingTask<T>>,
    capacity: usize,
    /// __0__ for none, __>0__ for receivers, __<0__ for senders
    number_of_senders_or_receivers: isize,
    head: usize,
    number_of_special_tasks: usize,
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
            number_of_special_tasks: 0,
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

    fn set_len(&mut self, len: usize) {
        match self.option() {
            SenderReceiverQueueOption::Sender => {
                self.number_of_senders_or_receivers = -(len as isize);
            }
            SenderReceiverQueueOption::Receiver => {
                self.number_of_senders_or_receivers = len as isize;
            }
            SenderReceiverQueueOption::Empty => {}
        }
    }

    /// Returns a slice pointer into the buffer.
    /// `range` must lie inside `0..self.capacity()`.
    #[inline]
    unsafe fn buffer_range(&self, range: Range<usize>) -> *mut [WaitingTask<T>] {
        unsafe {
            ptr::slice_from_raw_parts_mut(
                self.ptr.add(range.start).as_ptr(),
                range.end - range.start,
            )
        }
    }

    /// Given a range into the logical buffer of the deque, this function
    /// returns two ranges into the physical buffer that correspond to
    /// the given range. The `len` parameter should usually just be `self.len`;
    /// the reason it's passed explicitly is that if the deque is wrapped in
    /// a `Drain`, then `self.len` is not actually the length of the deque.
    ///
    /// # Safety
    ///
    /// This function is always safe to call. For the resulting ranges to be valid
    /// ranges into the physical buffer, the caller must ensure that the result of
    /// calling `slice::range(range, ..len)` represents a valid range into the
    /// logical buffer, and that all elements in that range are initialized.
    #[inline]
    fn slice_ranges<R>(&self, range: R, len: usize) -> (Range<usize>, Range<usize>)
    where
        R: RangeBounds<usize>,
    {
        fn get_range<R>(range: R, bounds: ops::RangeTo<usize>) -> Range<usize>
        where
            R: RangeBounds<usize>,
        {
            let len = bounds.end;

            let start = match range.start_bound() {
                ops::Bound::Included(&start) => start,
                ops::Bound::Excluded(start) => start.checked_add(1).unwrap_or_else(|| {
                    panic!("attempted to index slice from after maximum usize");
                }),
                ops::Bound::Unbounded => 0,
            };

            let end = match range.end_bound() {
                ops::Bound::Included(end) => end.checked_add(1).unwrap_or_else(|| {
                    panic!("attempted to index slice up to maximum usize");
                }),
                ops::Bound::Excluded(&end) => end,
                ops::Bound::Unbounded => len,
            };

            if start > end {
                panic!("attempted to index slice from after maximum usize");
            }
            if end > len {
                panic!("attempted to index slice up to maximum usize");
            }

            Range { start, end }
        }

        let Range { start, end } = get_range(range, ..len);
        let len = end - start;

        if len == 0 {
            (0..0, 0..0)
        } else {
            // `slice::range` guarantees that `start <= end <= len`.
            // because `len != 0`, we know that `start < end`, so `start < len`
            // and the indexing is valid.
            let wrapped_start = self.to_physical_idx(start);

            // this subtraction can never overflow because `wrapped_start` is
            // at most `self.capacity()` (and if `self.capacity != 0`, then `wrapped_start` is strictly less
            // than `self.capacity`).
            let head_len = self.capacity() - wrapped_start;

            if head_len >= len {
                // we know that `len + wrapped_start <= self.capacity <= usize::MAX`, so this addition can't overflow
                (wrapped_start..wrapped_start + len, 0..0)
            } else {
                // can't overflow because of the if condition
                let tail_len = len - head_len;
                (wrapped_start..self.capacity(), 0..tail_len)
            }
        }
    }

    #[inline]
    fn swap(&mut self, i: usize, j: usize) {
        assert_hint(i < self.len(), "index out of bounds");
        assert_hint(j < self.len(), "index out of bounds");

        let ri = self.to_physical_idx(i);
        let rj = self.to_physical_idx(j);
        unsafe { ptr::swap(self.ptr.add(ri).as_ptr(), self.ptr.add(rj).as_ptr()) }
    }

    #[inline]
    fn as_mut_slices(&mut self) -> (&mut [WaitingTask<T>], &mut [WaitingTask<T>]) {
        let (a_range, b_range) = self.slice_ranges(.., self.len());
        // SAFETY: `slice_ranges` always returns valid ranges into
        // the physical buffer.
        unsafe {
            (
                &mut *self.buffer_range(a_range),
                &mut *self.buffer_range(b_range),
            )
        }
    }

    fn truncate(&mut self, len: usize) {
        /// Runs the destructor for all items in the slice when it gets dropped (normally or
        /// during unwinding).
        struct Dropper<'a, T>(&'a mut [T]);

        impl<T> Drop for Dropper<'_, T> {
            fn drop(&mut self) {
                unsafe {
                    ptr::drop_in_place(self.0);
                }
            }
        }

        let new_len;

        // Safe because:
        //
        // * Any slice passed to `drop_in_place` is valid; the second case has
        //   `len <= front.len()` and returning on `len > self.len()` ensures
        //   `begin <= back.len()` in the first case
        // * The head of the VecDeque is moved before calling `drop_in_place`,
        //   so no value is dropped twice if `drop_in_place` panics
        unsafe {
            if len >= self.len() {
                return;
            }

            let (front, back) = self.as_mut_slices();
            if len > front.len() {
                let begin = len - front.len();
                let drop_back = back.get_unchecked_mut(begin..) as *mut _;

                new_len = len;

                ptr::drop_in_place(drop_back);
            } else {
                let drop_back = back as *mut _;
                let drop_front = front.get_unchecked_mut(len..) as *mut _;

                new_len = len;

                // Make sure the second half is dropped even when a destructor
                // in the first one panics.
                let _back_dropper = Dropper(&mut *drop_back);

                ptr::drop_in_place(drop_front);
            }
        }

        self.set_len(new_len);
    }

    fn retain<F: FnMut(&mut WaitingTask<T>) -> bool>(&mut self, mut f: F) {
        // Forked from std::collections::VecDeque::retain. For detail read it.

        let len = self.len();
        let mut len_to_process = len;
        let mut success = 0;
        let mut failure = 0;
        let mut idx = 0;
        let mut cur = 0;

        // Stage 1: All values are retained.
        while cur < len_to_process {
            if !f(unsafe { self.ptr.add(cur).as_mut() }) {
                cur += 1;

                break;
            }
            cur += 1;
            idx += 1;

            if (success + failure) == 32 && failure > 12 {
                len_to_process = (len_to_process / 3).min(len);
                success = 0;
                failure = 0;
            }
        }
        // Stage 2: Swap retained value into current idx.
        while cur < len_to_process {
            if !f(unsafe { self.ptr.add(cur).as_mut() }) {
                cur += 1;

                continue;
            }

            self.swap(idx, cur);
            cur += 1;
            idx += 1;
        }
        // Stage 3: Truncate all values after idx.
        if cur != idx {
            self.truncate(idx);
        }
    }

    #[cold]
    fn maybe_free_special_tasks(&mut self) {
        let mut delta = 0;

        self.retain(|task| {
            if task.can_be_freed() {
                delta += 1;

                return false;
            }

            true
        });

        self.number_of_special_tasks -= delta;
    }

    fn push_back<const DELTA: isize>(&mut self, task: WaitingTask<T>) {
        if !matches!(&task, WaitingTask::Common(..)) {
            self.number_of_special_tasks += 1;

            if self.number_of_special_tasks.trailing_zeros() >= 10 {
                self.maybe_free_special_tasks();
            }
        }

        let len = self.len();

        if likely(len < self.capacity) {
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
        if !matches!(&res, WaitingTask::Common(..)) {
            self.number_of_special_tasks -= 1;
        }

        let must_shrink = (len * 3 < self.capacity) && len > 4;

        if unlikely(!must_shrink) {
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

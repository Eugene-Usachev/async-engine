//! This module contains the [`ArrayDeque`].
use crate::utils::assert_hint;
use std::mem;
use std::mem::{ManuallyDrop, MaybeUninit};
use std::ops::{Deref, DerefMut};
use std::ptr::drop_in_place;

/// `ArrayDeque` is a deque, but it uses an array on stack and can't be resized.
pub struct ArrayDeque<T, const N: usize> {
    stack: ManuallyDrop<[T; N]>,
    len: usize,
    head: usize,
}

impl<T, const N: usize> ArrayDeque<T, N> {
    /// Creates new `ArrayDeque`.
    pub fn new() -> Self {
        #[allow(
            clippy::uninit_assumed_init,
            reason = "We guarantee that the array is initialized, when reading from it"
        )]
        {
            Self {
                stack: ManuallyDrop::new(unsafe { MaybeUninit::uninit().assume_init() }),
                len: 0,
                head: 0,
            }
        }
    }

    /// Returns the number of elements in the deque.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the deque is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns an index of the underlying array for the provided index.
    #[inline]
    fn to_physical_idx(&self, idx: usize) -> usize {
        let logical_index = self.head + idx;

        debug_assert!(logical_index < N || (logical_index - N) < N);
        if logical_index >= N {
            logical_index - N
        } else {
            logical_index
        }
    }

    /// Appends an element to the back of the deque.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the stack is not full.
    pub unsafe fn push_back(&mut self, value: T) {
        assert_hint(self.len() < N, "Tried to push to a full array stack");

        let idx = self.to_physical_idx(self.len());

        self.stack[idx] = value;
        self.len += 1;
    }

    /// Removes the first element and returns it, or None if the deque is empty
    pub fn pop_front(&mut self) -> Option<T> {
        if !self.is_empty() {
            self.len -= 1;

            let idx = self.head;
            self.head = self.to_physical_idx(1);

            assert_hint(
                self.stack.len() >= idx,
                &format!("idx: {}, len: {}", idx, self.stack.len()),
            );

            Some(unsafe { (&raw mut self.stack[idx]).read() })
        } else {
            None
        }
    }

    /// Drops all elements in the deque and set the length to 0.
    pub fn clear(&mut self) {
        if mem::needs_drop::<T>() {
            for i in 0..self.len() {
                unsafe {
                    drop_in_place(&raw mut self.stack[i]);
                };
            }
        }

        self.len = 0;
    }
}

impl<T, const N: usize> Deref for ArrayDeque<T, N> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &*self.stack
    }
}

impl<T, const N: usize> AsRef<[T]> for ArrayDeque<T, N> {
    fn as_ref(&self) -> &[T] {
        &*self.stack
    }
}

impl<T, const N: usize> DerefMut for ArrayDeque<T, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.stack
    }
}

impl<T, const N: usize> AsMut<[T]> for ArrayDeque<T, N> {
    fn as_mut(&mut self) -> &mut [T] {
        &mut *self.stack
    }
}

impl<T, const N: usize> Default for ArrayDeque<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> From<[T; N]> for ArrayDeque<T, N> {
    fn from(array: [T; N]) -> Self {
        Self {
            stack: ManuallyDrop::new(array),
            len: N,
            head: 0,
        }
    }
}

impl<T, const N: usize> Drop for ArrayDeque<T, N> {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_array_deque() {
        let mut deque = ArrayDeque::<u32, 4>::new();

        unsafe {
            deque.push_back(1);
            assert_eq!(deque.len(), 1);

            deque.push_back(2);
            assert_eq!(deque.len(), 2);

            deque.push_back(3);
            assert_eq!(deque.len(), 3);

            assert_eq!(deque.pop_front(), Some(1));
            assert_eq!(deque.len(), 2);

            deque.push_back(4);
            assert_eq!(deque.len(), 3);

            deque.push_back(5);
            assert_eq!(deque.len(), 4);

            assert_eq!(deque.pop_front(), Some(2));
            assert_eq!(deque.pop_front(), Some(3));
            assert_eq!(deque.pop_front(), Some(4));
            assert_eq!(deque.pop_front(), Some(5));
            assert_eq!(deque.pop_front(), None);
        }
    }
}

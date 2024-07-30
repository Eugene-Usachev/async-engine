use std::fmt::Debug;
use std::intrinsics::unlikely;
use std::alloc::{alloc, dealloc, Layout};
use std::cmp::{max};
use std::ops::{Deref, DerefMut, Index, IndexMut};
use std::ptr::{NonNull, slice_from_raw_parts_mut};
use std::slice::SliceIndex;
use crate::buf::buf_pool::buf_pool;
use crate::buf::buffer;
use crate::cfg::config_buf_len;

/// Buffer for data transfer. Buffer is allocated in heap.
///
/// Buffer has `len` field.
///
/// - `len` is how many bytes have been written into the buffer.
/// 
/// # About pool
/// 
/// For get from [`BufPool`] call [`buffer`](crate::buf::buffer).
/// If you can use [`BufPool`], use it, to have better performance.
///
/// If it was gotten from [`BufPool`] it will come back after drop.
///
/// # Buffer representation
///
/// ```text
/// +---+---+---+---+---+---+---+---+
/// | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |
/// +---+---+---+---+---+---+---+---+
/// | X | X | X | X | X |   |   |   |
/// +---+---+---+---+---+---+---+---+
///                     ^           ^
///                    len         cap
///
/// len = 5 (from 1 to 5 inclusive)
/// 5 blocks occupied (X), 3 blocks free (blank)
/// ```
pub struct Buffer {
    pub(crate) slice: NonNull<[u8]>,
    len: usize
}

impl Buffer {
    /// Creates raw slice with given capacity. This code avoids checking zero capacity.
    ///
    /// # Safety
    ///
    /// capacity > 0
    #[inline(always)]
    fn raw_slice(capacity: usize) -> NonNull<[u8]> {
        let layout = match Layout::array::<u8>(capacity) {
            Ok(layout) => layout,
            Err(_) => panic!("Cannot create slice with capacity {capacity}. Capacity overflow."),
        };
        unsafe {
            NonNull::new_unchecked(slice_from_raw_parts_mut(alloc(layout), capacity))
        }
    }

    /// Creates new buffer with given size. This buffer will not be put to the pool.
    /// So, use it only for creating a buffer with specific size.
    ///
    /// # Safety
    /// - size > 0
    #[inline(always)]
    pub fn new(size: usize) -> Self {
        if unlikely(size == 0) {
            panic!("Cannot create Buffer with size 0. Size must be > 0.");
        }

        Buffer {
            slice: Self::raw_slice(size),
            len: 0
        }
    }

    /// Creates a new buffer from a pool with the given size.
    #[inline(always)]
    pub(crate) fn new_from_pool(size: usize) -> Self {
        Buffer {
            slice: Self::raw_slice(size),
            len: 0
        }
    }

    /// Returns `true` if the buffer is empty.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns how many bytes have been written into the buffer.
    /// So, it is [`len`](#field.len).
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Increases [`len`](#field.len) on diff
    #[inline(always)]
    pub fn add_len(&mut self, diff: usize) {
        self.len += diff;
    }

    /// Sets [`len`](#field.len).
    #[inline(always)]
    pub fn set_len(&mut self, len: usize) {
        self.len = len;
    }

    /// Sets [`len`](#field.len) to [`real_cap`](#method.real_cap).
    #[inline(always)]
    pub fn set_len_to_cap(&mut self) {
        self.len = self.cap();
    }

    /// Returns a real capacity of the buffer.
    #[inline(always)]
    pub fn cap(&self) -> usize {
        self.slice.len()
    }

    /// Returns `true` if the buffer is full.
    #[inline(always)]
    pub fn is_full(&self) -> bool {
        self.cap() == self.len
    }

    #[inline(always)]
    pub fn resize(&mut self, new_size: usize) {
        if new_size < self.len {
            self.len = new_size;
        }

        let mut new_buf = if config_buf_len() == new_size {
            buffer()
        } else {
            Buffer::new(new_size)
        };

        new_buf.len = self.len;
        new_buf.as_mut()[..self.len].copy_from_slice(&self.as_ref()[..self.len]);

        *self = new_buf;
    }

    /// Appends data to the buffer. If a capacity is not enough, the buffer will be resized and will not be put to the pool.
    #[inline(always)]
    pub fn append(&mut self, buf: &[u8]) {
        let len = buf.len();
        if unlikely(len > self.slice.len() - self.len) {
            self.resize(max(self.len + len, self.cap() * 2));
        }

        unsafe { self.slice.as_mut()[self.len..self.len + len].copy_from_slice(buf); }
        self.len += len;
    }

    /// Returns a pointer to the buffer.
    #[inline(always)]
    pub fn as_ptr(&self) -> *const u8 {
        self.slice.as_ptr().as_mut_ptr()
    }

    /// Returns a mutable pointer to the buffer.
    #[inline(always)]
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.slice.as_mut_ptr()
    }

    /// Clears the buffer.
    #[inline(always)]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Puts the buffer to the pool. You can not to use it, and then this method will be called automatically by drop.
    #[inline(always)]
    pub fn release(self) {
        buf_pool().put(self);
    }

    /// Puts the buffer to the pool without checking for a size.
    ///
    /// # Safety
    /// - [`buf.real_cap`](#method.real_cap)() == [`config_buf_len`](config_buf_len)()
    #[inline(always)]
    pub unsafe fn release_unchecked(self) {
        unsafe { buf_pool().put_unchecked(self) };
    }
}

impl<'a> Deref for Buffer {
    type Target = [u8];

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl<'a> DerefMut for Buffer {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut()
    }
}

impl<I: SliceIndex<[u8]>> Index<I> for Buffer {
    type Output = I::Output;

    #[inline(always)]
    fn index(&self, index: I) -> &Self::Output {
        self.as_ref().index(index)
    }
}

impl<I: SliceIndex<[u8]>> IndexMut<I> for Buffer {
    #[inline(always)]
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        self.as_mut().index_mut(index)
    }
}

impl AsRef<[u8]> for Buffer {
    fn as_ref(&self) -> &[u8] {
        unsafe { &self.slice.as_ref()[0..self.len] }
    }
}

impl AsMut<[u8]> for Buffer {
    fn as_mut(&mut self) -> &mut [u8] {
        unsafe { &mut self.slice.as_mut()[0..self.len] }
    }
}

impl PartialEq<&[u8]> for Buffer {
    fn eq(&self, other: &&[u8]) -> bool {
        self.as_ref() == *other
    }
}

impl Debug for Buffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.as_ref())
    }
}

impl From<Box<[u8]>> for Buffer {
    fn from(slice: Box<[u8]>) -> Self {
        Buffer {
            len: slice.len(),
            slice: NonNull::from(Box::leak(slice)),
        }
    }
}

impl<const N: usize> From<Box<[u8; N]>> for Buffer {
    fn from(slice: Box<[u8; N]>) -> Self {
        Buffer {
            len: slice.len(),
            slice: NonNull::from(Box::leak(slice)),
        }
    }
}

impl From<Vec<u8>> for Buffer {
    fn from(mut slice: Vec<u8>) -> Self {
        let l = slice.len();
        unsafe { slice.set_len(slice.capacity()) }

        Buffer {
            len: l,
            slice: NonNull::from(slice.leak()),
        }
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        let pool = buf_pool();
        if self.cap() == pool.buffer_len() {
            let buf = Buffer {
                slice: self.slice,
                len: self.len
            };
            unsafe { pool.put_unchecked(buf) };
        } else {
            unsafe { dealloc(self.slice.as_mut_ptr(), Layout::array::<u8>(self.cap()).unwrap_unchecked())}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let buf = Buffer::new(1);
        assert_eq!(buf.cap(), 1);
    }

    #[test]
    #[should_panic(expected = "Cannot create Buffer with size 0. Size must be > 0.")]
    fn test_new_panic() {
        Buffer::new(0);
    }

    #[test]
    fn test_add_len_and_set_len_to_cap() {
        let mut buf = Buffer::new(100);
        assert_eq!(buf.len(), 0);

        buf.add_len(10);
        assert_eq!(buf.len(), 10);

        buf.add_len(20);
        assert_eq!(buf.len(), 30);

        buf.set_len_to_cap();
        assert_eq!(buf.len(), buf.cap());
    }

    #[test]
    fn test_len_and_cap_() {
        let mut buf = Buffer::new(100);
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.cap(), 100);

        buf.set_len(10);
        assert_eq!(buf.len(), 10);
        assert_eq!(buf.cap(), 100);
    }

    #[test]
    fn test_resize() {
        let mut buf = Buffer::new(100);
        buf.append(&[1, 2, 3]);

        buf.resize(200);
        assert_eq!(buf.cap(), 200);
        assert_eq!(buf.as_ref(), &[1, 2, 3]);

        buf.resize(50);
        assert_eq!(buf.cap(), 50);
        assert_eq!(buf.as_ref(), &[1, 2, 3]);
    }

    #[test]
    fn test_append_and_clear() {
        let mut buf = Buffer::new(5);

        buf.append(&[1, 2, 3]);
        // This code checks written
        assert_eq!(buf.as_ref(), &[1, 2, 3]);
        assert_eq!(buf.cap(), 5);

        buf.append(&[4, 5, 6]);
        assert_eq!(buf.as_ref(), &[1, 2, 3, 4, 5, 6]);
        assert_eq!(buf.cap(), 10);

        buf.clear();
        assert_eq!(buf.as_ref(), &[]);
        assert_eq!(buf.cap(), 10);
    }

    #[test]
    fn test_is_empty_and_is_full() {
        let mut buf = Buffer::new(5);
        assert!(buf.is_empty());
        buf.append(&[1, 2, 3]);
        assert!(!buf.is_empty());
        buf.clear();
        assert!(buf.is_empty());

        let mut buf = Buffer::new(5);
        assert!(!buf.is_full());
        buf.append(&[1, 2, 3, 4, 5]);
        assert!(buf.is_full());
        buf.clear();
        assert!(!buf.is_full());
    }

    #[test]
    fn test_index() {
        let mut buf = Buffer::new(5);
        buf.append(&[1, 2, 3]);
        assert_eq!(buf[0], 1);
        assert_eq!(buf[1], 2);
        assert_eq!(buf[2], 3);
        assert_eq!(&buf[1..=2], &[2,3]);
        assert_eq!(&buf[..3], &[1, 2, 3]);
        assert_eq!(&buf[2..], &[3]);
        assert_eq!(&buf[..], &[1, 2, 3]);
    }

    #[test]
    fn test_from() {
        let b = Box::new([1, 2, 3]);
        let buf = Buffer::from(b);
        assert_eq!(buf.as_ref(), &[1, 2, 3]);
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.cap(), 3);

        let mut v = vec![1, 2, 3];
        v.reserve(7);
        let buf = Buffer::from(v);
        assert_eq!(buf.as_ref(), &[1, 2, 3]);
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.cap(), 10);

        let mut v = vec![1, 2, 3];
        let buf = Buffer::from(v.into_boxed_slice());
        assert_eq!(buf.as_ref(), &[1, 2, 3]);
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.cap(), 3);
    }
}
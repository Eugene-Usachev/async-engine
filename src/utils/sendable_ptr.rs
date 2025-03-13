use std::ops::Deref;
use std::ptr::NonNull;

// TODO docs
pub struct SendableNonNull<T> {
    ptr: NonNull<T>,
}

impl<T> From<NonNull<T>> for SendableNonNull<T> {
    fn from(ptr: NonNull<T>) -> Self {
        Self { ptr }
    }
}

impl<T> From<&mut T> for SendableNonNull<T> {
    fn from(ptr: &mut T) -> Self {
        Self {
            ptr: NonNull::from(ptr),
        }
    }
}

impl<T> From<&T> for SendableNonNull<T> {
    fn from(ptr: &T) -> Self {
        Self {
            ptr: NonNull::from(ptr),
        }
    }
}

impl<T> Deref for SendableNonNull<T> {
    type Target = NonNull<T>;

    fn deref(&self) -> &Self::Target {
        &self.ptr
    }
}

impl<T> Clone for SendableNonNull<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for SendableNonNull<T> {}

unsafe impl<T: Send> Send for SendableNonNull<T> {}
unsafe impl<T: Sync> Sync for SendableNonNull<T> {}

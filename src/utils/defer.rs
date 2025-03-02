use std::mem::ManuallyDrop;

/// A container for a function to be deferred. It will be called when the container is dropped.
///
/// Use [`defer`] to create a container.
pub(crate) struct DeferContainer<F: FnOnce()> {
    f: ManuallyDrop<F>,
}

impl<F: FnOnce()> Drop for DeferContainer<F> {
    #[inline]
    fn drop(&mut self) {
        (unsafe { ManuallyDrop::take(&mut self.f) })();
    }
}

/// Defers a function to be called when this scope ends.
#[inline]
pub(crate) fn defer<F: FnOnce()>(f: F) -> DeferContainer<F> {
    DeferContainer {
        f: ManuallyDrop::new(f),
    }
}

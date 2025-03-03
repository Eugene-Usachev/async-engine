/// `OnceState` is used to indicate whether the `Once` has been called or not.
///
/// # Variants
///
/// * `NotCalled` - The `Once` has not been called.
/// * `Called` - The `Once` has been called.
#[derive(Eq, PartialEq, Copy, Clone, Debug)]
pub enum OnceState {
    NotCalled = 0,
    Called = 1,
}

impl OnceState {
    /// Returns the `OnceState` as an `isize`.
    pub const fn not_called() -> isize {
        0
    }

    /// Returns the `OnceState` as an `isize`.
    pub const fn called() -> isize {
        1
    }
}

impl From<OnceState> for isize {
    fn from(state: OnceState) -> Self {
        state as Self
    }
}

impl TryFrom<isize> for OnceState {
    type Error = ();

    fn try_from(value: isize) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::NotCalled),
            1 => Ok(Self::Called),
            _ => Err(()),
        }
    }
}

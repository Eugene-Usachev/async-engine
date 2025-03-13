use std::ops::Deref;
use std::ptr::NonNull;

/// `CallState` is a state for recv/send futures.
/// This is used to improve performance.
///
/// [`WaitSend`]: channels::WaitSend
/// [`WaitLocalSend`]: channels::WaitLocalSend
pub enum CallState {
    /// Default state.
    FirstCall,
    /// Receiver writes the value associated with this task.
    WokenToReturnReady,
    /// This task was enqueued, now it is woken by close.
    WokenByClose,
}

impl CallState {
    /// Returns whether the state is [`CallState::WokenByClose`].
    pub fn is_closed(&self) -> bool {
        matches!(*self, Self::WokenByClose)
    }
}

// TODO docs
#[derive(Copy, Clone)]
pub struct CallStatePtr(NonNull<CallState>);

impl CallStatePtr {
    pub fn new(ptr: &mut CallState) -> Self {
        Self(NonNull::from(ptr))
    }

    pub fn set_to_closed(&self) {
        unsafe {
            self.0.write(CallState::WokenByClose);
        }
    }

    pub fn write(&self, value: CallState) {
        unsafe {
            self.0.write(value);
        }
    }
}

impl Deref for CallStatePtr {
    type Target = CallState;

    fn deref(&self) -> &Self::Target {
        unsafe { self.0.as_ref() }
    }
}

unsafe impl Sync for CallStatePtr {}
unsafe impl Send for CallStatePtr {}

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
        matches!(*self, CallState::WokenByClose)
    }
}

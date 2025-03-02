use std::ptr::NonNull;

/// `SendCallState` is a state machine for [`WaitSend`] or [`WaitLocalSend`].
/// This is used to improve performance.
///
/// [`WaitSend`]: channels::WaitSend
/// [`WaitLocalSend`]: channels::WaitLocalSend
pub(super) enum SendCallState {
    /// Default state.
    FirstCall,
    /// Receiver writes the value associated with this task.
    WokenToReturnReady,
    /// This task was enqueued, now it is woken by close.
    WokenByClose,
}

/// `RecvCallState` is a state machine for [`WaitRecv`] or [`WaitLocalRecv`].
/// This is used to improve performance.
///
/// [`WaitRecv`]: channels::WaitRecv
/// [`WaitLocalRecv`]: channels::WaitLocalRecv
pub(super) enum RecvCallState {
    /// Default state.
    FirstCall,
    /// This task was enqueued, now it is woken for return [`Poll::Ready`],
    /// because a [`WaitSend`] or [`WaitLocalSend`] has written to the slot already.
    ///
    /// [`WaitSend`]: channels::WaitSend
    /// [`WaitLocalSend`]: channels::WaitLocalSend
    WokenToReturnReady,
    /// This task was enqueued, now it is woken by close.
    WokenByClose,
}

// TODO docs
#[derive(Copy, Clone)]
pub union PtrToCallState {
    send: NonNull<SendCallState>,
    recv: NonNull<RecvCallState>,
    unsafe_any: NonNull<()>,
    uninit: (),
}

impl PtrToCallState {
    pub fn uninit() -> Self {
        PtrToCallState { uninit: () }
    }

    pub(super) unsafe fn cast<State>(&self) -> NonNull<State> {
        self.unsafe_any.cast()
    }

    pub unsafe fn as_send_and_is_closed(&self) -> bool {
        matches!(unsafe { self.send.as_ref() }, SendCallState::WokenByClose)
    }

    pub unsafe fn as_recv_and_is_closed(&self) -> bool {
        matches!(unsafe { self.recv.as_ref() }, RecvCallState::WokenByClose)
    }

    pub unsafe fn as_send_and_set_closed(&mut self) {
        *unsafe { self.send.as_mut() } = SendCallState::WokenByClose;
    }

    pub unsafe fn as_recv_and_set_closed(&mut self) {
        *unsafe { self.recv.as_mut() } = RecvCallState::WokenByClose;
    }
}

impl From<NonNull<SendCallState>> for PtrToCallState {
    fn from(send: NonNull<SendCallState>) -> Self {
        PtrToCallState { send }
    }
}

impl From<NonNull<RecvCallState>> for PtrToCallState {
    fn from(recv: NonNull<RecvCallState>) -> Self {
        PtrToCallState { recv }
    }
}

impl From<&mut SendCallState> for PtrToCallState {
    fn from(state: &mut SendCallState) -> Self {
        PtrToCallState {
            send: NonNull::from(state),
        }
    }
}

impl From<&mut RecvCallState> for PtrToCallState {
    fn from(state: &mut RecvCallState) -> Self {
        PtrToCallState {
            recv: NonNull::from(state),
        }
    }
}

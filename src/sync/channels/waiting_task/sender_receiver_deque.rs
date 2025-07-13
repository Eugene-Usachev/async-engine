//! This module contains the [`SenderReceiverQueue`].
use crate::runtime::waiting_task::WaitingTask;
use crate::utils::{likely, unlikely, unwrap_or_bug_hint};
use std::collections::VecDeque;

/// Each receiver adds 1 to the counter of [`SenderReceiverQueue`]
const RECEIVER_DELTA: isize = 1;
/// Each sender subtracts 1 from the counter of [`SenderReceiverQueue`]
const SENDER_DELTA: isize = -1;

/// Current state of [`SenderReceiverQueue`].
///
/// Read [`SenderReceiverQueueOption::Empty`], [`SenderReceiverQueueOption::Sender`],
/// [`SenderReceiverQueueOption::Receiver`] for more details.
#[derive(Eq, PartialEq, Copy, Clone)]
pub(crate) enum SenderReceiverQueueOption {
    /// The [`SenderReceiverQueue`] doesn't contain any waiting tasks.
    Empty,
    /// The [`SenderReceiverQueue`] contains senders.
    Sender,
    /// The [`SenderReceiverQueue`] contains receivers.
    Receiver,
}

/// [`SenderReceiverQueue`] is a deque of [`WaitingTask`].
///
/// It is optimized to contain only receivers or only senders
/// and to count special tasks and release them.
///
/// The provided type `T` doesn't matter because it uses only pointers.
#[repr(C)]
pub(crate) struct SenderReceiverQueue<T = ()> {
    deque: VecDeque<WaitingTask<T>>,
    state: SenderReceiverQueueOption,
    number_of_special_tasks: usize,
}

impl<T> SenderReceiverQueue<T> {
    /// Returns a new [`SenderReceiverQueue`].
    pub(crate) fn new() -> Self {
        const DEFAULT_CAP: usize = 2;

        Self {
            deque: VecDeque::with_capacity(DEFAULT_CAP),
            state: SenderReceiverQueueOption::Empty,
            number_of_special_tasks: 0,
        }
    }

    /// Returns the current state of [`SenderReceiverQueue`].
    ///
    /// Read [`SenderReceiverQueueOption`] for more details.
    #[inline]
    pub(crate) fn option(&self) -> SenderReceiverQueueOption {
        self.state
    }

    /// Returns the capacity of [`SenderReceiverQueue`].
    #[inline]
    pub(crate) fn capacity(&self) -> usize {
        self.deque.capacity()
    }

    /// Returns the length of [`SenderReceiverQueue`].
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.deque.len()
    }

    /// Returns `true` if [`SenderReceiverQueue`] is empty.
    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.deque.is_empty()
    }

    /// Releases special tasks if it is possible.
    #[cold]
    fn maybe_free_special_tasks(&mut self) {
        let mut delta = 0;

        self.deque.retain(|task| {
            if task.can_be_freed() {
                delta += 1;

                return false;
            }

            true
        });

        self.number_of_special_tasks -= delta;
    }

    /// Pushes the provided task to the queue.
    ///
    /// It uses `DELTA` to set the number of senders/receivers.
    ///
    /// As `DELTA` it can accept only [`SENDER_DELTA`] or [`RECEIVER_DELTA`].
    fn push_back<const IS_RECEIVER: bool>(&mut self, task: WaitingTask<T>) {
        if IS_RECEIVER {
            debug_assert!(self.state != SenderReceiverQueueOption::Sender);

            self.state = SenderReceiverQueueOption::Receiver;
        } else {
            debug_assert!(self.state != SenderReceiverQueueOption::Receiver);

            self.state = SenderReceiverQueueOption::Sender;
        }

        if !matches!(&task, WaitingTask::Common(..)) {
            self.number_of_special_tasks += 1;

            if unlikely(self.number_of_special_tasks.trailing_zeros() >= 10) {
                self.maybe_free_special_tasks();
            }
        }

        self.deque.push_back(task);
    }

    /// Pushes the provided task to the queue and stores it as a receiver.
    pub(crate) fn push_receiver(&mut self, task: WaitingTask<T>) {
        debug_assert!(self.option() != SenderReceiverQueueOption::Sender);

        self.push_back::<true>(task);
    }

    /// Pushes the provided task to the queue and stores it as a sender.
    pub(crate) fn push_sender(&mut self, task: WaitingTask<T>) {
        debug_assert!(self.option() != SenderReceiverQueueOption::Receiver);

        self.push_back::<false>(task);
    }

    /// Pops a task from the queue.
    ///
    /// It uses `DELTA` to set the number of senders/receivers.
    ///
    /// As `DELTA` it can accept only [`SENDER_DELTA`] or [`RECEIVER_DELTA`].
    unsafe fn pop_front(&mut self) -> WaitingTask<T> {
        debug_assert!(!self.is_empty());

        let res = unwrap_or_bug_hint(self.deque.pop_front());

        if !matches!(&res, WaitingTask::Common(..)) {
            self.number_of_special_tasks -= 1;
        }

        if self.is_empty() {
            self.state = SenderReceiverQueueOption::Empty;
        }

        let must_shrink = (self.len() * 3 < self.capacity()) && self.len() > 4;

        if likely(!must_shrink) {
            return res;
        }

        self.deque.shrink_to(self.len() / 2);

        res
    }

    /// Pops the provided task to the queue only if it is associated with a receiver.
    pub(crate) fn pop_receiver(&mut self) -> Option<WaitingTask<T>> {
        if self.option() == SenderReceiverQueueOption::Receiver {
            Some(unsafe { self.pop_front() })
        } else {
            None
        }
    }

    /// Pops the provided task to the queue only if it is associated with a sender.
    pub(crate) fn pop_sender(&mut self) -> Option<WaitingTask<T>> {
        if self.option() == SenderReceiverQueueOption::Sender {
            Some(unsafe { self.pop_front() })
        } else {
            None
        }
    }
}

impl<T> Drop for SenderReceiverQueue<T> {
    fn drop(&mut self) {
        debug_assert!(self.is_empty());
    }
}

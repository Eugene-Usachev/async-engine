use std::fmt::Debug;

/// An error of an asynchronous send operation.
///
/// # Variants
///
/// - [`Closed`](SendErr::Closed): The channel was closed before the value could be sent.
pub enum SendErr<T> {
    /// The channel was closed before the value could be sent.
    Closed(T),
}

impl<T> Debug for SendErr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed(_) => write!(f, "SendErr::Closed"),
        }
    }
}

/// An error of a non-blocking send attempt.
///
/// Used for scenarios where the sender attempts to send a value
/// without waiting for the channel to become available.
///
/// # Variants
///
/// - [`Full`](TrySendErr::Full): The channel is full. Contains the value that could not be sent.
///
/// - [`Locked`](TrySendErr::Locked): The channel is locked, indicating temporary unavailability.
///   Contains the value that could not be sent.
///
/// - [`Closed`](TrySendErr::Closed): The channel is closed.
///   Contains the value that could not be sent.
pub enum TrySendErr<T> {
    /// The channel is full. Contains the value that could not be sent.
    Full(T),
    /// The channel is locked, indicating temporary unavailability.
    /// Contains the value that could not be sent.
    Locked(T),
    /// The channel is closed. Contains the value that could not be sent.
    Closed(T),
}

impl<T> Debug for TrySendErr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full(_) => write!(f, "TrySendErr::Full"),
            Self::Locked(_) => write!(f, "TrySendErr::Locked"),
            Self::Closed(_) => write!(f, "TrySendErr::Closed"),
        }
    }
}

/// An error of an asynchronous `receive` operation.
///
/// # Variants
///
/// - [`Closed`](RecvErr::Closed): The channel is closed, and no more values can be received.
pub enum RecvErr {
    /// The channel is closed, and no more values can be received.
    Closed,
}

impl Debug for RecvErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "RecvErr::Closed"),
        }
    }
}

/// An error of a non-blocking receive attempt.
///
/// # Variants
///
/// - [`Empty`](TryRecvErr::Empty): The channel is empty; no values are currently available.
///
/// - [`Locked`](TryRecvErr::Locked): The channel is locked, indicating temporary unavailability.
///
/// - [`Closed`](TryRecvErr::Closed): The channel is closed, and no more values can be received.
pub enum TryRecvErr {
    /// The channel is empty; no values are currently available.
    Empty,
    /// The channel is locked, indicating temporary unavailability.
    Locked,
    /// The channel is closed, and no more values can be received.
    Closed,
}

impl Debug for TryRecvErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "TryRecvErr::Empty"),
            Self::Locked => write!(f, "TryRecvErr::Locked"),
            Self::Closed => write!(f, "TryRecvErr::Closed"),
        }
    }
}

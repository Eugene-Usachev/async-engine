//! This module contains the [`SelectNonBlockingBranchResult`].
//!
//! It is public only for implementation the [`select`](crate::select).
//! If you don't want to write your own `select` or understand how does `Orengine` works,
//! you don't need to read it.

/// Result of [`SelectReceiver::recv_or_subscribe`] or [`SelectSender::send_or_subscribe`].
///
/// For more details read [`SelectNonBlockingBranchResult::Success`],
/// [`SelectNonBlockingBranchResult::AlreadyAcquired`],
/// [`SelectNonBlockingBranchResult::NotReady`].
///
/// [`SelectReceiver::recv_or_subscribe`]: crate::sync::channels::SelectReceiver::recv_or_subscribe
/// [`SelectSender::send_or_subscribe`]: crate::sync::channels::SelectSender::send_or_subscribe
pub enum SelectNonBlockingBranchResult {
    /// The [`TaskInSelectBranch`] was acquired successfully and the value from the provided slot
    /// was written successfully
    /// or a value from a channel was successfully written to the provided slot.
    ///
    /// [`TaskInSelectBranch`]: crate::sync::channels::waiting_task::TaskInSelectBranch
    Success,
    /// The [`TaskInSelectBranch`] was already acquired, and a channel and the provided slot were
    /// not modified.
    ///
    /// [`TaskInSelectBranch`]: crate::sync::channels::waiting_task::TaskInSelectBranch
    AlreadyAcquired,
    /// A channel is still not ready and the [`TaskInSelectBranch`] was subscribed to it.
    ///
    /// [`TaskInSelectBranch`]: crate::sync::channels::waiting_task::TaskInSelectBranch
    NotReady,
}

use crate::sync::AsyncSender;
use crate::sync::channels::select::SelectNonBlockingBranchResult;
use crate::sync::channels::state::CallStatePtr;
use crate::sync::channels::waiting_task::TaskInSelectBranch;
use std::ptr::NonNull;

/// The `SelectSender` trait provides methods for [`select`](crate::select).
pub trait SelectSender: AsyncSender<Self::Data> {
    /// The type of data stored in the `SelectSender`.
    type Data;

    /// Tries to send the provided data to `SelectSender`.
    ///
    /// Returns [`SelectNonBlockingBranchResult`].
    ///
    /// Read [`SelectNonBlockingBranchResult`] for more details.
    fn send_or_subscribe(
        &self,
        data: NonNull<Self::Data>,
        state: CallStatePtr,
        task_in_select_branch: TaskInSelectBranch,
    ) -> SelectNonBlockingBranchResult;
}

impl<G, T> SelectSender for T
where
    G: SelectSender,
    T: std::ops::Deref<Target = G> + AsyncSender<G::Data>,
{
    type Data = G::Data;

    fn send_or_subscribe(
        &self,
        data: NonNull<Self::Data>,
        state: CallStatePtr,
        task_in_select_branch: TaskInSelectBranch,
    ) -> SelectNonBlockingBranchResult {
        (**self).send_or_subscribe(data, state, task_in_select_branch)
    }
}

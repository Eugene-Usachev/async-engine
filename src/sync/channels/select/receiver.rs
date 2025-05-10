use crate::sync::AsyncReceiver;
use crate::sync::channels::select::SelectNonBlockingBranchResult;
use crate::sync::channels::state::CallStatePtr;
use crate::sync::channels::waiting_task::TaskInSelectBranch;
use std::ptr::NonNull;

/// The `SelectReceiver` trait provides methods for [`select`](crate::select).
pub trait SelectReceiver: AsyncReceiver<Self::Data> {
    /// The type of data stored in the `SelectReceiver`.
    type Data;

    /// Tries to receive a value from the `SelectReceiver`.
    ///
    /// Returns [`SelectNonBlockingBranchResult`].
    ///
    /// Read [`SelectNonBlockingBranchResult`] for more details.
    fn recv_or_subscribe(
        &self,
        slot: NonNull<Self::Data>,
        state: CallStatePtr,
        task_in_select_branch: TaskInSelectBranch,
    ) -> SelectNonBlockingBranchResult;
}

impl<G, T> SelectReceiver for T
where
    G: SelectReceiver,
    T: std::ops::Deref<Target = G> + AsyncReceiver<G::Data>,
{
    type Data = G::Data;

    fn recv_or_subscribe(
        &self,
        slot: NonNull<Self::Data>,
        state: CallStatePtr,
        task_in_select_branch: TaskInSelectBranch,
    ) -> SelectNonBlockingBranchResult {
        (**self).recv_or_subscribe(slot, state, task_in_select_branch)
    }
}

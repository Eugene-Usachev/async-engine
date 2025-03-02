use crate::sync::channels::select::SelectNonBlockingBranchResult;
use crate::sync::channels::states::PtrToCallState;
use crate::sync::channels::waiting_task::TaskInSelectBranch;
use crate::sync::AsyncSender;
use std::ptr::NonNull;

// TODO docs and update from `TrySendInSelectErr`
pub trait SelectSender: AsyncSender<Self::Data> {
    /// The type of data stored in the `SelectSender`.
    type Data;

    /// Tries to send the provided data to the `SelectSender`.
    ///
    /// On success returns `Ok(())`.
    ///
    /// Else returns `Err(`[`TrySendInSelectErr`]`)`.
    ///
    /// # Errors meaning
    ///
    /// - [`TrySendInSelectErr::Closed`]: [`select`] executes an associated
    ///   branch with [`SendErr::Closed`];
    ///
    /// - [`TrySendInSelectErr::Locked`]: [`select`] retries if no other branch can be executed;
    ///
    /// - [`TrySendInSelectErr::Full`]: The channel is full, and the [`SelectBranchManager`]
    ///   is subscribed to the `SelectSender`.
    ///
    /// [`select`]: crate::sync::channels::select::select_
    /// [`SendErr::Closed`]: crate::sync::channels::SendErr::Closed
    fn send_or_subscribe(
        &self,
        data: NonNull<Self::Data>,
        state: PtrToCallState,
        task_in_select_branch: TaskInSelectBranch,
        is_all_local: bool,
    ) -> SelectNonBlockingBranchResult;
}

impl<G: SelectSender, T: std::ops::Deref<Target = G> + AsyncSender<G::Data>> SelectSender for T {
    type Data = G::Data;

    fn send_or_subscribe(
        &self,
        data: NonNull<Self::Data>,
        state: PtrToCallState,
        task_in_select_branch: TaskInSelectBranch,
        is_all_local: bool,
    ) -> SelectNonBlockingBranchResult {
        (**self).send_or_subscribe(data, state, task_in_select_branch, is_all_local)
    }
}

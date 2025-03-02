use crate::sync::channels::select::SelectNonBlockingBranchResult;
use crate::sync::channels::states::PtrToCallState;
use crate::sync::channels::waiting_task::TaskInSelectBranch;
use crate::sync::channels::SelectSender;
use crate::sync::AsyncReceiver;
use std::ptr::NonNull;

// TODO docs and update from `TryRecvErr`
pub trait SelectReceiver: AsyncReceiver<Self::Data> {
    /// The type of data stored in the `SelectReceiver`.
    type Data;

    /// Tries to receive a value from the `SelectReceiver`.
    ///
    /// On success returns `Ok(())` and puts the value into `slot`.
    ///
    /// Else returns `Err(`[`TryRecvErr`]`)`.
    ///
    /// It gets a pointer because `slot` can be uninitialized, and it is low-level API.
    ///
    /// # Errors meaning
    ///
    /// - [`TryRecvErr::Closed`]: [`select`] executes an associated branch with [`RecvErr::Closed`];
    ///
    /// - [`TryRecvErr::Locked`]: [`select`] retries if no other branch can be executed;
    ///
    /// - [`TryRecvErr::Empty`]: no data available, and the [`TaskInSelectBranch`]
    ///   is subscribed to the `SelectReceiver`.
    ///
    /// [`select`]: crate::sync::channels::select::select_
    /// [`RecvErr::Closed`]: crate::sync::channels::RecvErr::Closed
    fn recv_or_subscribe(
        &self,
        slot: NonNull<Self::Data>,
        state: PtrToCallState,
        task_in_select_branch: TaskInSelectBranch,
        is_all_local: bool,
    ) -> SelectNonBlockingBranchResult;
}

impl<G: SelectReceiver, T: std::ops::Deref<Target = G> + AsyncReceiver<G::Data>> SelectReceiver
    for T
{
    type Data = G::Data;

    fn recv_or_subscribe(
        &self,
        slot: NonNull<Self::Data>,
        state: PtrToCallState,
        task_in_select_branch: TaskInSelectBranch,
        is_all_local: bool,
    ) -> SelectNonBlockingBranchResult {
        (**self).recv_or_subscribe(slot, state, task_in_select_branch, is_all_local)
    }
}

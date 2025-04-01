use crate::sync::channels::waiting_task::TaskInSelectBranch;

// TODO docs and visibility
pub enum SelectNonBlockingBranchResult {
    Success,
    AlreadyAcquired,
    Locked(TaskInSelectBranch),
    NotReady,
}

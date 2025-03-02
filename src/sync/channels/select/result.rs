// TODO docs and visibility

pub enum SelectNonBlockingBranchResult {
    Success,
    AlreadyAcquired,
    Locked,
    NotReady,
}

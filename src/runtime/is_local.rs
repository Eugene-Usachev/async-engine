/// A trait that indicates whether a struct is `local` or `shared`.
///
/// `Local` structs can work only with `local` tasks, are __not__ [`Send`]
/// and produce __not__ [`Send`]. They never send data or [`Tasks`](crate::runtime::Task)
/// to other threads.
///
/// `Shared` structs can work with both `local` and `shared` tasks. It expects to be [`Send`]
/// and [`Sync`]. THey can send data and [`Tasks`](crate::runtime::Task) to other threads.
///
/// Since at the moment the constant functions in traits are not available, it contains only
/// the [`IS_LOCAL`](IsLocal::IS_LOCAL) constant. But you can use const [`is_local`]
/// and [`is_local_of`] functions.
///
/// # Important
///
/// [`Tasks`](crate::runtime::Task) have its own [`is_local`](crate::runtime::Task::is_local)
/// const function and don't implement this trait.
///
/// # Usage
///
/// For most of the cases you don't need to implement this trait and can use [`Send`] and [`Sync`].
/// `Orengine` uses it to prevent a mixing of `local` and `shared` structs in runtime
/// with `debug_assertions`. You can use it for your own purposes, but should not expect that other
/// developers will use or implement it.
///
/// # Example
///
/// ```rust
/// use orengine::runtime::{IsLocal, is_local, is_local_of};
///
/// struct LocalStruct;
/// struct SharedStruct;
///
/// impl IsLocal for LocalStruct {
///     const IS_LOCAL: bool = true;
/// }
///
/// impl IsLocal for SharedStruct {
///     const IS_LOCAL: bool = false;
/// }
///
/// // By identifier
/// assert!(is_local::<LocalStruct>());
/// assert!(!is_local::<SharedStruct>());
///
/// // By expression
/// let local_struct = LocalStruct;
/// let shared_struct = SharedStruct;
///
/// assert!(is_local_of(&local_struct));
/// assert!(!is_local_of(&shared_struct));
/// ```
pub trait IsLocal {
    /// Signifies whether the struct is `local` or `shared`.
    const IS_LOCAL: bool;
}

/// Returns whether the struct is `local` or `shared`. Read [`IsLocal`] trait for more details.
pub const fn is_local<T: IsLocal>() -> bool {
    T::IS_LOCAL
}

/// Returns whether the struct is `local` or `shared`. Read [`IsLocal`] trait for more details.
pub const fn is_local_of<T: IsLocal>(_: &T) -> bool {
    T::IS_LOCAL
}

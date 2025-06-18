//! This module provides a trait to prevent implementing some traits in other crates.

/// This trait can be implemented only in this crate. So, it is used to prevent implementing
/// some traits in other crates.
pub(crate) trait Sealed {}

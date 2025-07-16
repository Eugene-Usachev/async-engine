//! This module contains the [`SelectReceiver`], [`SelectSender`]
//! and [`SelectNonBlockingBranchResult`].
//!
//! It is public only for implementation the [`select`](crate::select).
//! If you don't want to write your own `select` or understand how does `Orengine` works,
//! you don't need to read it.
pub mod receiver;
mod result;
mod select_macro;
pub mod sender;
#[cfg(test)]
mod test;

pub use receiver::*;
pub use result::*;
pub use sender::*;

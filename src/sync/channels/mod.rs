//! This module contains async channels (e.g. [`std::sync::mpsc::channel`]).
//!
//! It provides the [`AsyncChannel`], [`AsyncSender`] and [`AsyncReceiver`] traits.
//!
//! In `local` context use [`LocalChannel`].
//! In `shared` context use [`Channel`].
//!
//! You can read about the differences between `local` and `shared` contexts in the
//! [`Executor`](crate::Executor).
//!
//! # Examples
//!
//! ## No splitting
//!
//! ```rust
//! use orengine::sync::{AsyncChannel, AsyncReceiver, AsyncSender};
//!
//! async fn foo() {
//!     let channel = orengine::sync::Channel::bounded(1); // capacity = 1
//!
//!     channel.send(1).await.unwrap();
//!
//!     let res = channel.recv().await.unwrap();
//!
//!     assert_eq!(res, 1);
//! }
//! ```
//!
//! ## Splitting
//!
//! You can split the channel by using only one trait of [`AsyncSender`] and [`AsyncReceiver`].
//!
//! ```rust
//! use orengine::sync::{AsyncChannel, AsyncReceiver, AsyncSender};
//! use orengine::local_executor;
//!
//! use std::sync::Arc;
//!
//! async fn return_value(sender: impl AsyncSender<u32>) {
//!     sender.send(1).await.unwrap();
//! }
//!
//! async fn print_value(receiver: impl AsyncReceiver<u32>) {
//!     let res = receiver.recv().await.unwrap();
//!
//!     assert_eq!(res, 1);
//! }
//!
//! async fn foo() {
//!     let channel = Arc::new(orengine::sync::Channel::bounded(1)); // capacity = 1
//!
//!     local_executor().spawn_shared(return_value(channel.clone()));
//!     local_executor().spawn_shared(print_value(channel));
//! }
//! ```
mod async_trait;
mod errors;
mod local;
pub mod select;
mod shared;
mod state;
pub mod waiting_task;

pub use async_trait::*;
pub use errors::*;
pub use local::*;
pub use select::{SelectReceiver, SelectSender};
pub use shared::*;
pub use state::{CallState, CallStatePtr};

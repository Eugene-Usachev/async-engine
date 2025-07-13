// TODO add link to the online documentation
//! This module contains async runtime.
//!
//! You can use the [`Executor`] to run the async runtime in the current thread.
//!
//! To get the current executor, use the [`local_executor`] function.
//!
//! If you want to read more details, read the online documentation.
//!
//! # Example
//!
//! ```rust
//! use orengine::{local_executor, yield_now, Executor};
//!
//! async fn foo() {
//!     println!("Hello from async foo!");
//! }
//!
//! async fn bar() {
//!     println!("Hello from the async runtime!");
//!
//!     local_executor().spawn_local(foo());
//!
//!     yield_now().await; // To allow the foo to complete
//! }
//!
//! Executor::init().run_and_block_on_local(async move {
//!     bar().await;
//! }).unwrap();
//! ```
mod asyncify;
mod call;
pub mod epoch_gc;
pub mod executor;
mod global_state;
#[cfg(not(feature = "disable_send_task_to"))]
mod interaction_between_executors;
mod is_local;
pub(super) mod local_thread_pool;
mod shutdown;
pub mod task;
pub mod waker;

pub use asyncify::*;
pub use call::*;
pub use executor::*;
pub use global_state::{
    executors_ids, stop_all_executors, stop_executor, work_sharing_executors_ids,
};
pub use is_local::*;
pub use task::*;

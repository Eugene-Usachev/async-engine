//! This module provides the [`Executor`], the [`Config`] struct
//! and the [`local_executor`] function.
mod config;
mod end_local_thread_and_write_into_ptr;
mod executor;
pub(crate) mod executors_on_cores_table;
mod sleeping_manager;

pub use config::*;
pub use executor::*;
pub(crate) use executors_on_cores_table::get_core_id_for_executor;

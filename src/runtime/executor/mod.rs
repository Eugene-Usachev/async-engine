pub mod config;
mod end_local_thread_and_write_into_ptr;
pub mod executor;
pub mod executors_on_cores_table;

pub use config::*;
pub use executor::*;
pub use executors_on_cores_table::get_core_id_for_executor;

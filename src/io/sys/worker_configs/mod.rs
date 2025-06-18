//! This module contains IO worker configuration structs: [`IOUringConfig`] and [`FallbackConfig`].
mod fallback_config;
mod io_uring_config;

pub use fallback_config::*;
pub use io_uring_config::*;

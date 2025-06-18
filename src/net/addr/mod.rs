//! This module contains traits to generalize and socket address representation:
//!
//! [`FromSockAddr`], [`IntoSockAddr`], [`ToSockAddrs`].

mod from_sock_addr;
mod into_sock_addr;
mod to_sock_addrs;

pub use from_sock_addr::*;
pub use into_sock_addr::*;
pub use to_sock_addrs::*;

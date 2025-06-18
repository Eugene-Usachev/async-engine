//! This module provides traits that generalize common socket operations and structs
//! that implement them.
//!
//! # Traits
//!
//! - [`Socket`]
//! - [`Listener`]
//! - [`Stream`]
//! - [`Datagram`]
//! - [`ConnectedDatagram`]
//!
//! # TCP
//!
//! - [`TcpListener`]
//! - [`TcpStream`]
//!
//! # UDP
//!
//! - [`UdpSocket`]
//! - [`UdpConnectedSocket`]
//!
//! # Unix
//!
//! - [`UnixListener`]
//! - [`UnixStream`]
//! - [`UnixDatagram`]
//! - [`UnixConnectedDatagram`]
pub use addr::*;
pub use bind_config::{BindConfig, ReusePort};
pub use connected_datagram::ConnectedDatagram;
pub use datagram::Datagram;
pub use listener::Listener;
pub use socket::Socket;
pub use stream::Stream;
pub use tcp::{TcpListener, TcpStream};
pub use udp::{UdpConnectedSocket, UdpSocket};
#[cfg(unix)]
pub use unix::{UnixConnectedDatagram, UnixDatagram, UnixListener, UnixStream};
pub(crate) use unsupport::new_unix_unsupported_error;

mod addr;
mod bind_config;
mod connected_datagram;
pub(crate) mod creators_of_sockets;
mod datagram;
mod listener;
mod socket;
mod stream;
pub mod tcp;
pub mod udp;
#[cfg(unix)]
pub mod unix;
pub(crate) mod unsupport;

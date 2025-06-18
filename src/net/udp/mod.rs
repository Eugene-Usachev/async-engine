//! This module contains [`UdpConnectedSocket`] and [`UdpSocket`] structs.
mod connected_socket;
mod socket;

pub use connected_socket::UdpConnectedSocket;
pub use socket::UdpSocket;

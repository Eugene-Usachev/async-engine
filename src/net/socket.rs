use crate::io::sys::{AsSocket, FromRawSocket, IntoRawSocket};
use crate::io::{AsyncPollSocket, AsyncSocketClose};
use std::io;
use std::io::Error;
use std::net::SocketAddr;

/// The `Socket` trait defines common socket-related operations and is intended to be implemented
/// for types that represent network sockets.
///
/// It provides methods for querying and configuring
/// socket settings, such as TTL and obtaining the local address
/// or pending socket errors.
///
/// # Implemented traits
///
/// - [`AsyncPollSocket`]
/// - [`AsyncSocketClose`]
/// - [`IntoRawSocket`]
/// - [`FromRawSocket`]
/// - [`AsSocket`]
/// - [`AsyncPollSocket`]
pub trait Socket:
    IntoRawSocket + FromRawSocket + AsSocket + AsyncPollSocket + AsyncSocketClose
{
    /// Returns the local socket address that the socket is bound to.
    ///
    /// # Example
    ///
    /// ```rust
    /// use orengine::io::AsyncBind;
    /// use orengine::net::Socket;
    ///
    /// # async fn foo() -> std::io::Result<()> {
    /// let socket = orengine::net::UdpSocket::bind("127.0.0.1:8080").await?;
    /// let addr = socket.local_addr()?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        let borrow_socket = AsSocket::as_socket(self);
        let socket_ref = socket2::SockRef::from(&borrow_socket);
        socket_ref
            .local_addr()?
            .as_socket()
            .ok_or_else(|| Error::new(io::ErrorKind::Other, "failed to get local address"))
    }

    /// Sets the TTL (Time-To-Live) value for outgoing packets.
    /// The TTL value determines how many hops (routers) a packet can pass through
    /// before being discarded.
    #[inline(always)]
    fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        let borrow_socket = AsSocket::as_socket(self);
        let socket_ref = socket2::SockRef::from(&borrow_socket);
        socket_ref.set_ttl(ttl)
    }

    /// Returns the current TTL value for outgoing packets.
    #[inline(always)]
    fn ttl(&self) -> io::Result<u32> {
        let borrow_socket = AsSocket::as_socket(self);
        let socket_ref = socket2::SockRef::from(&borrow_socket);
        socket_ref.ttl()
    }

    /// Retrieves and clears the pending socket error, if any.
    /// This can be used to check if the socket has encountered any errors while
    /// performing operations like sending or receiving data.
    #[inline(always)]
    fn take_error(&self) -> io::Result<Option<Error>> {
        let borrow_socket = AsSocket::as_socket(self);
        let socket_ref = socket2::SockRef::from(&borrow_socket);
        socket_ref.take_error()
    }
}

//! `TCPSocket < IPSocket` -- a connected TCP stream. `TCPSocket.new(host, port)`
//! connects (via `std::net`, Gvl-released) and wraps the fd; the read/write/gets
//! surface comes from `IO`, and `addr`/`peeraddr`/`send`/`recv`/... from the
//! `IPSocket`/`BasicSocket` ancestors, so this file only mints the socket.

use std::os::unix::io::IntoRawFd;

use super::{host_port, map_io_err, resolve_one};
use crate::builtins::arity;
use crate::builtins::io::socket_from_raw_fd;
use zeo_abi::TCPSOCKET_CLASS;
use zeo_macros::ruby_class;

ruby_class! {
    TCPSocket = zeo_abi::TCPSOCKET_CLASS < zeo_abi::IP_SOCKET_CLASS;

    // `TCPSocket.new(host, port)` -- connect; the result reads/writes as an IO.
    def self."new" | "open"(_recv, args, _block) {
        arity!(args, 2);
        let (host, port) = host_port(args, "127.0.0.1")?;
        let addr = resolve_one(&host, port)?;
        // Gvl-released: connect(2) blocks until the peer answers.
        let stream = crate::gvl::without_gvl(|| std::net::TcpStream::connect(addr))
            .map_err(|e| map_io_err(&e, "connect(2)"))?;
        // SAFETY: `into_raw_fd` yields a fresh, solely-owned descriptor.
        Ok(unsafe { socket_from_raw_fd(stream.into_raw_fd(), TCPSOCKET_CLASS) })
    }
}

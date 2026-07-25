//! `TCPServer < TCPSocket` -- a listening TCP socket. `TCPServer.new(host, port)`
//! binds and listens (via `std::net`, which sets `SO_REUSEADDR` and listens),
//! then `#accept` mints a connected `TCPSocket`. Bound/local address queries
//! (`#addr`, `#getsockname`, `#local_address`) come from the `IPSocket`/
//! `BasicSocket` ancestors over the same fd.

use std::os::fd::RawFd;
use std::os::unix::io::IntoRawFd;

use super::{errno_error, host_port, map_io_err};
use crate::builtins::arity;
use crate::builtins::io::{socket_from_raw_fd, socket_raw_fd};
use crate::builtins::io_error;
use crate::{RubyValue, Signal};
use zeo_abi::{TCPSERVER_CLASS, TCPSOCKET_CLASS};
use zeo_macros::ruby_class;

fn fd_of(recv: &RubyValue) -> Result<RawFd, Signal> {
    socket_raw_fd(recv).ok_or_else(|| io_error!("closed stream"))
}

ruby_class! {
    TCPServer = zeo_abi::TCPSERVER_CLASS < zeo_abi::TCPSOCKET_CLASS;

    // `TCPServer.new([host, ] port)` -- bind + listen (a nil/omitted host binds
    // all interfaces). Port 0 lets the kernel pick; read it back via `#addr`.
    def self."new" | "open"(_recv, args, _block) {
        arity!(args, 1..=2);
        let (host, port) = host_port(args, "0.0.0.0")?;
        let listener = std::net::TcpListener::bind((host.as_str(), port))
            .map_err(|e| map_io_err(&e, "bind(2)"))?;
        // SAFETY: `into_raw_fd` yields a fresh, solely-owned descriptor.
        Ok(unsafe { socket_from_raw_fd(listener.into_raw_fd(), TCPSERVER_CLASS) })
    }

    // `#accept` -- block for a client, answering a connected `TCPSocket`. The
    // blocking accept(2) is Gvl-released so a peer about to connect isn't
    // stalled by an armed Gvl holder parked here.
    def "accept"(recv, args, _block) {
        arity!(args, 0);
        let fd = fd_of(recv)?;
        // SAFETY: accept(2) on an owned listening fd; the peer address is
        // discarded here (available via the returned socket's #peeraddr).
        let nfd = crate::gvl::without_gvl(|| unsafe {
            libc::accept(fd, std::ptr::null_mut(), std::ptr::null_mut())
        });
        if nfd < 0 {
            return Err(errno_error("accept(2)"));
        }
        // SAFETY: `nfd` is a fresh, solely-owned descriptor.
        Ok(unsafe { socket_from_raw_fd(nfd, TCPSOCKET_CLASS) })
    }
    // `#listen(backlog)` -- a std-bound listener already listens, so this
    // re-applies the backlog and answers 0.
    def "listen"(recv, args, _block) {
        arity!(args, 1);
        let fd = fd_of(recv)?;
        let backlog = crate::builtins::convert::to_index(&args[0])? as libc::c_int;
        // SAFETY: listen(2) on an owned fd.
        if unsafe { libc::listen(fd, backlog) } != 0 {
            return Err(errno_error("listen(2)"));
        }
        Ok(RubyValue::Int(0))
    }
}

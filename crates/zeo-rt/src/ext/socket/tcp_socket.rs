//! `TCPSocket < IPSocket` -- a connected TCP stream. `TCPSocket.new(host, port)`
//! connects (via `std::net`, Gvl-released) and wraps the fd; the read/write/gets
//! surface comes from `IO`, and `addr`/`peeraddr`/`send`/`recv`/... from the
//! `IPSocket`/`BasicSocket` ancestors, so this file only mints the socket.

use std::net::{SocketAddr, TcpStream};
use std::os::unix::io::{FromRawFd, IntoRawFd};

use super::{
    errno_error, host_port, kw_strip, kwarg_secs, map_io_err, resolve_one, socketaddr_to_raw,
};
use crate::builtins::io::socket_from_raw_fd;
use crate::builtins::{arity, convert};
use crate::{RubyValue, Signal};
use zeo_abi::TCPSOCKET_CLASS;
use zeo_macros::ruby_class;

/// The optional `local_host, local_port` pair: a source address to bind
/// before connecting. `nil`/absent for either means "let the kernel pick",
/// which is the whole pair absent.
fn local_bind(args: &[RubyValue]) -> Result<Option<SocketAddr>, Signal> {
    let host = match args.get(2) {
        None | Some(RubyValue::Nil) => return Ok(None),
        Some(v) => convert::to_rstr(v)?.lock().to_utf8_lossy().into_owned(),
    };
    let port = match args.get(3) {
        None | Some(RubyValue::Nil) => 0,
        Some(v) => super::port_of(v)?,
    };
    Ok(Some(resolve_one(&host, port)?))
}

/// Connect from a specific source address: socket(2) + bind(2) + connect(2)
/// by hand, since `TcpStream::connect` owns the whole sequence.
fn connect_bound(local: SocketAddr, remote: SocketAddr) -> Result<TcpStream, Signal> {
    let domain = if remote.is_ipv6() {
        libc::AF_INET6
    } else {
        libc::AF_INET
    };
    let fd = unsafe { libc::socket(domain, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(errno_error("socket(2)"));
    }
    // SAFETY: `fd` is a fresh descriptor this call solely owns, so the
    // stream closes it on every exit path below.
    let stream = unsafe { TcpStream::from_raw_fd(fd) };
    let one: libc::c_int = 1;
    unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            std::ptr::from_ref(&one).cast(),
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );
    }
    let (storage, len) = socketaddr_to_raw(&local);
    if unsafe { libc::bind(fd, std::ptr::from_ref(&storage).cast(), len) } < 0 {
        return Err(errno_error("bind(2)"));
    }
    let (storage, len) = socketaddr_to_raw(&remote);
    let rc = crate::gvl::without_gvl(|| unsafe {
        libc::connect(fd, std::ptr::from_ref(&storage).cast(), len)
    });
    if rc < 0 {
        return Err(errno_error("connect(2)"));
    }
    Ok(stream)
}

ruby_class! {
    TCPSocket = zeo_abi::TCPSOCKET_CLASS < zeo_abi::IP_SOCKET_CLASS;

    // `TCPSocket.new(host, port, local_host = nil, local_port = nil,
    // connect_timeout: nil, open_timeout: nil)` -- connect; the result
    // reads/writes as an IO. The timeout keywords are accepted (net/http
    // passes `open_timeout:`) and bound to the connect itself.
    def self."new" | "open" arity -1 (_recv, args, _block) {
        let positional = kw_strip(args);
        arity!(positional, 1..=4);
        let timeout = kwarg_secs(args, "open_timeout")?
            .or(kwarg_secs(args, "connect_timeout")?);
        let (host, port) = host_port(&positional[..positional.len().min(2)], "127.0.0.1")?;
        let addr = resolve_one(&host, port)?;
        let stream = match local_bind(positional)? {
            Some(local) => connect_bound(local, addr)?,
            None => match timeout {
                // Gvl-released: connect(2) blocks until the peer answers.
                None => crate::gvl::without_gvl(|| TcpStream::connect(addr))
                    .map_err(|e| map_io_err(&e, "connect(2)"))?,
                Some(t) => crate::gvl::without_gvl(|| TcpStream::connect_timeout(&addr, t))
                    .map_err(|e| map_io_err(&e, "connect(2)"))?,
            },
        };
        // SAFETY: `into_raw_fd` yields a fresh, solely-owned descriptor.
        Ok(unsafe { socket_from_raw_fd(stream.into_raw_fd(), TCPSOCKET_CLASS) })
    }
}

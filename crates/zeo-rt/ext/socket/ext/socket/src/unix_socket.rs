//! `UNIXSocket < BasicSocket` -- an AF_UNIX stream socket. `UNIXSocket.new(path)`
//! connects to a listening `UNIXServer`; `#addr`/`#peeraddr` answer the
//! `["AF_UNIX", path]` array, `#path` the peer's path. Reading/writing and the
//! raw ops come from the `IO`/`BasicSocket` ancestors.

use std::os::fd::RawFd;

use super::{errno_error, pack_unix_sockaddr, parse_unix_sockaddr};
use crate::builtins::io::{socket_from_raw_fd, socket_raw_fd};
use crate::builtins::io_error;
use crate::{RubyValue, Signal, string_new};
use zeo_abi::UNIX_SOCKET_CLASS;
use zeo_macros::ruby_class;

fn fd_of(recv: &RubyValue) -> Result<RawFd, Signal> {
    socket_raw_fd(recv).ok_or_else(|| io_error!("closed stream"))
}

/// The AF_UNIX path from `getsockname`/`getpeername` (`f`).
pub(crate) fn name_path(
    fd: RawFd,
    ctx: &str,
    f: unsafe extern "C" fn(RawFd, *mut libc::sockaddr, *mut libc::socklen_t) -> libc::c_int,
) -> Result<String, Signal> {
    // SAFETY: zeroed storage is valid; `len` bounds the write; the family is
    // AF_UNIX for a UNIXSocket receiver.
    unsafe {
        let mut storage: libc::sockaddr_storage = std::mem::zeroed();
        let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        if f(fd, &mut storage as *mut _ as *mut libc::sockaddr, &mut len) != 0 {
            return Err(errno_error(ctx));
        }
        Ok(parse_unix_sockaddr(&storage))
    }
}

/// The `["AF_UNIX", path]` array a UNIX socket's `#addr`/`#peeraddr` answer.
fn unix_addr_array(path: String) -> RubyValue {
    RubyValue::Array(crate::array_new(vec![
        RubyValue::Str(string_new("AF_UNIX".to_string())),
        RubyValue::Str(string_new(path)),
    ]))
}

/// Open an AF_UNIX stream socket and connect it to `path` (shared with the
/// `.new` path here; `UNIXServer` binds instead).
pub(crate) fn connect_unix(path: &str) -> Result<RawFd, Signal> {
    // SAFETY: a plain socket(2) call.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(errno_error("socket(2)"));
    }
    let (storage, len) = pack_unix_sockaddr(path)?;
    // SAFETY: `storage`/`len` describe an initialized sockaddr_un.
    let rc = crate::gvl::without_gvl(|| unsafe {
        libc::connect(fd, &storage as *const _ as *const libc::sockaddr, len)
    });
    if rc != 0 {
        let err = errno_error("connect(2)");
        // SAFETY: close the fd we just failed to connect.
        unsafe { libc::close(fd) };
        return Err(err);
    }
    Ok(fd)
}

ruby_class! {
    UNIXSocket = zeo_abi::UNIX_SOCKET_CLASS < zeo_abi::BASIC_SOCKET_CLASS;

    // `UNIXSocket.new(path)` -- connect to a listening AF_UNIX socket.
    def self."new" | "open" cfunc (_recv, arg) {
        let path = crate::builtins::file::path_arg(arg, "new")?;
        // SAFETY: `connect_unix` yields a fresh, solely-owned descriptor.
        Ok(unsafe { socket_from_raw_fd(connect_unix(&path)?, UNIX_SOCKET_CLASS) })
    }
    // `UNIXSocket.pair(type = SOCK_STREAM)` (aka `socketpair`) -- a connected
    // pair of AF_UNIX sockets.
    def self."pair" | "socketpair"(_recv, arg1?, _arg2?) {
        let ty = match arg1 {
            None | Some(RubyValue::Nil) => libc::SOCK_STREAM,
            Some(RubyValue::Int(n)) => *n as libc::c_int,
            Some(v) => crate::builtins::convert::to_index(v)? as libc::c_int,
        };
        let mut fds = [0 as libc::c_int; 2];
        // SAFETY: socketpair fills the two-element `fds` array.
        if unsafe { libc::socketpair(libc::AF_UNIX, ty, 0, fds.as_mut_ptr()) } != 0 {
            return Err(errno_error("socketpair(2)"));
        }
        // SAFETY: each fd is a fresh, solely-owned descriptor.
        let (a, b) = unsafe {
            (
                socket_from_raw_fd(fds[0], UNIX_SOCKET_CLASS),
                socket_from_raw_fd(fds[1], UNIX_SOCKET_CLASS),
            )
        };
        Ok(RubyValue::Array(crate::array_new(vec![a, b])))
    }

    // `#addr` -- `["AF_UNIX", local_path]`.
    def "addr"(recv) {
        Ok(unix_addr_array(name_path(fd_of(recv)?, "getsockname(2)", libc::getsockname)?))
    }
    // `#peeraddr` -- `["AF_UNIX", peer_path]`.
    def "peeraddr"(recv) {
        Ok(unix_addr_array(name_path(fd_of(recv)?, "getpeername(2)", libc::getpeername)?))
    }
    // `#path` -- this socket's OWN (local) path via getsockname. A connected
    // client is unnamed, so its path is "" (CRuby's behaviour); a bound server
    // reports the path it listens on.
    def "path"(recv) {
        Ok(RubyValue::Str(string_new(
            name_path(fd_of(recv)?, "getsockname(2)", libc::getsockname)?,
        )))
    }
}

//! `UNIXServer < UNIXSocket` -- a listening AF_UNIX socket. `UNIXServer.new(path)`
//! creates the socket file, binds, and listens; `#accept` answers a connected
//! `UNIXSocket`. `#addr` (the bound `["AF_UNIX", path]`) comes from the
//! `UNIXSocket` ancestor.

use std::os::fd::RawFd;

use super::{errno_error, pack_unix_sockaddr};
use crate::builtins::io::{socket_from_raw_fd, socket_raw_fd};
use crate::builtins::{arity, io_error};
use crate::{RubyValue, Signal};
use zeo_abi::{UNIX_SERVER_CLASS, UNIX_SOCKET_CLASS};
use zeo_macros::ruby_class;

fn fd_of(recv: &RubyValue) -> Result<RawFd, Signal> {
    socket_raw_fd(recv).ok_or_else(|| io_error!("closed stream"))
}

ruby_class! {
    UNIXServer = zeo_abi::UNIX_SERVER_CLASS < zeo_abi::UNIX_SOCKET_CLASS;

    // `UNIXServer.new(path)` -- create + bind + listen on an AF_UNIX socket. The
    // path must not already exist (bind fails with EADDRINUSE otherwise, exactly
    // as CRuby).
    def self."new" | "open"(_recv, args, _block) {
        arity!(args, 1);
        let path = crate::builtins::file::path_arg(&args[0], "new")?;
        // SAFETY: a plain socket(2) call.
        let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
        if fd < 0 {
            return Err(errno_error("socket(2)"));
        }
        let (storage, len) = pack_unix_sockaddr(&path)?;
        // SAFETY: `storage`/`len` describe an initialized sockaddr_un; on any
        // failure we close the fd we just opened.
        let rc = unsafe { libc::bind(fd, &storage as *const _ as *const libc::sockaddr, len) };
        if rc != 0 || unsafe { libc::listen(fd, 5) } != 0 {
            let err = errno_error("bind(2)");
            unsafe { libc::close(fd) };
            return Err(err);
        }
        // SAFETY: `fd` is a fresh, solely-owned descriptor.
        Ok(unsafe { socket_from_raw_fd(fd, UNIX_SERVER_CLASS) })
    }

    // `#accept` -- block for a client, answering a connected `UNIXSocket`.
    def "accept"(recv, args, _block) {
        arity!(args, 0);
        let fd = fd_of(recv)?;
        // SAFETY: accept(2) on an owned listening fd.
        let nfd = crate::gvl::without_gvl(|| unsafe {
            libc::accept(fd, std::ptr::null_mut(), std::ptr::null_mut())
        });
        if nfd < 0 {
            return Err(errno_error("accept(2)"));
        }
        // SAFETY: `nfd` is a fresh, solely-owned descriptor.
        Ok(unsafe { socket_from_raw_fd(nfd, UNIX_SOCKET_CLASS) })
    }
    // `#accept_nonblock(exception: true)` -- accept only a client already
    // waiting; see `TCPServer#accept_nonblock`.
    def "accept_nonblock"(recv, args, _block) {
        let raises = crate::builtins::io::nonblock_raises(args);
        arity!(crate::builtins::io::kw_strip(args), 0);
        let Some((nfd, _, _)) = super::accept_nonblock_fd(fd_of(recv)?)? else {
            return crate::builtins::io::would_block(false, raises, "accept(2)");
        };
        // SAFETY: `nfd` is a fresh, solely-owned descriptor.
        Ok(unsafe { socket_from_raw_fd(nfd, UNIX_SOCKET_CLASS) })
    }
    // `#listen(backlog)` -- already listening; re-apply and answer 0.
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

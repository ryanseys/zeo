//! `BasicSocket < IO` -- the raw-descriptor operations every socket shares
//! (`Socket`, `TCPSocket`, `UDPSocket`, `UNIXSocket`, ...). Each works on the
//! receiver's fd (reached through `io::socket_raw_fd`) via `libc`, so a single
//! implementation serves the whole hierarchy through the ancestor walk.
//!
//! Oracle-verified against ruby 4.0.6 over a loopback pair: `#getsockname`/
//! `#getpeername` answer packed `sockaddr` Strings; `#local_address`/
//! `#remote_address` the matching `Addrinfo`; `#setsockopt` sets an int option;
//! `#send`/`#recv` move bytes; `#shutdown`/`#close_read`/`#close_write` half-
//! close; `#getsockopt` answers a `Socket::Option` (see `option.rs`).

use std::os::fd::RawFd;

use super::{errno_error, raw_to_socketaddr};
use crate::builtins::{arity, io_error, not_impl_error, type_error};
use crate::{RubyValue, Signal};
use zeo_macros::ruby_class;

/// The receiver's open socket fd, or an `IOError` on a closed/non-socket IO.
fn fd_of(recv: &RubyValue) -> Result<RawFd, Signal> {
    crate::builtins::io::socket_raw_fd(recv).ok_or_else(|| io_error!("closed stream"))
}

/// Read a filled `sockaddr` back to packed bytes (what `getsockname`/
/// `getpeername` answer), via `f` (`libc::getsockname`/`getpeername`).
fn name_bytes(
    fd: RawFd,
    ctx: &str,
    f: unsafe extern "C" fn(RawFd, *mut libc::sockaddr, *mut libc::socklen_t) -> libc::c_int,
) -> Result<Vec<u8>, Signal> {
    // SAFETY: a zeroed sockaddr_storage is valid; `len` bounds what the kernel
    // writes, and we copy only the `len` bytes it reports back.
    unsafe {
        let mut storage: libc::sockaddr_storage = std::mem::zeroed();
        let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        if f(fd, &mut storage as *mut _ as *mut libc::sockaddr, &mut len) != 0 {
            return Err(errno_error(ctx));
        }
        Ok(std::slice::from_raw_parts(&storage as *const _ as *const u8, len as usize).to_vec())
    }
}

/// The receiver's local/peer `SocketAddr` (for `#local_address`/`#remote_address`).
fn name_socketaddr(
    fd: RawFd,
    ctx: &str,
    f: unsafe extern "C" fn(RawFd, *mut libc::sockaddr, *mut libc::socklen_t) -> libc::c_int,
) -> Result<Option<std::net::SocketAddr>, Signal> {
    // SAFETY: as name_bytes.
    unsafe {
        let mut storage: libc::sockaddr_storage = std::mem::zeroed();
        let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        if f(fd, &mut storage as *mut _ as *mut libc::sockaddr, &mut len) != 0 {
            return Err(errno_error(ctx));
        }
        Ok(raw_to_socketaddr(&storage))
    }
}

/// The socktype (`SOCK_STREAM`/`SOCK_DGRAM`) of a socket fd, so
/// `#local_address` tags its `Addrinfo` the way CRuby does.
pub(crate) fn socktype_of(fd: RawFd) -> i32 {
    // SAFETY: SO_TYPE writes a single int into `ty`.
    unsafe {
        let mut ty: libc::c_int = 0;
        let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        if libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            &mut ty as *mut _ as *mut libc::c_void,
            &mut len,
        ) == 0
        {
            ty
        } else {
            0
        }
    }
}

/// The address family of a socket fd, which tags the `Socket::Option` its
/// `#getsockopt` answers. AF_UNSPEC when the kernel will not say.
fn family_of(fd: RawFd) -> i32 {
    // SAFETY: a zeroed sockaddr_storage is valid; `len` bounds the write.
    unsafe {
        let mut storage: libc::sockaddr_storage = std::mem::zeroed();
        let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        if libc::getsockname(fd, &mut storage as *mut _ as *mut libc::sockaddr, &mut len) == 0 {
            storage.ss_family as i32
        } else {
            libc::AF_UNSPEC
        }
    }
}

/// The protocol implied by a socktype, for an `Addrinfo` tag (kernels don't
/// expose the protocol via a portable getsockopt).
fn protocol_for(socktype: i32) -> i32 {
    if socktype == libc::SOCK_STREAM {
        libc::IPPROTO_TCP
    } else if socktype == libc::SOCK_DGRAM {
        libc::IPPROTO_UDP
    } else {
        0
    }
}

/// A `#setsockopt` value: an int (from Integer/true/false) or raw bytes.
fn optval_bytes(v: &RubyValue) -> Result<Vec<u8>, Signal> {
    match v {
        RubyValue::Int(n) => Ok((*n as libc::c_int).to_ne_bytes().to_vec()),
        RubyValue::Bool(b) => Ok((*b as libc::c_int).to_ne_bytes().to_vec()),
        RubyValue::Str(s) => Ok(s.lock().bytes().to_vec()),
        other => Err(type_error!(
            "no implicit conversion of {} into Integer",
            crate::builtins::class_name_of(other)
        )),
    }
}

fn int_arg(v: &RubyValue) -> Result<libc::c_int, Signal> {
    Ok(crate::builtins::convert::to_index(v)? as libc::c_int)
}

ruby_class! {
    BasicSocket = zeo_abi::BASIC_SOCKET_CLASS < zeo_abi::IO_CLASS;

    // `#getsockname` -- the packed local `sockaddr` bytes.
    def "getsockname"(recv, args, _block) {
        arity!(args, 0);
        Ok(super::binary_string(name_bytes(fd_of(recv)?, "getsockname(2)", libc::getsockname)?))
    }
    // `#getpeername` -- the packed peer `sockaddr` bytes.
    def "getpeername"(recv, args, _block) {
        arity!(args, 0);
        Ok(super::binary_string(name_bytes(fd_of(recv)?, "getpeername(2)", libc::getpeername)?))
    }
    // `#local_address` -- an `Addrinfo` for the bound (local) address.
    def "local_address"(recv, args, _block) {
        arity!(args, 0);
        let fd = fd_of(recv)?;
        let st = socktype_of(fd);
        match name_socketaddr(fd, "getsockname(2)", libc::getsockname)? {
            Some(a) => Ok(super::addrinfo::from_socketaddr(a, st, protocol_for(st))),
            None => Err(not_impl_error!("local_address for this socket family is not supported")),
        }
    }
    // `#remote_address` -- an `Addrinfo` for the connected peer.
    def "remote_address"(recv, args, _block) {
        arity!(args, 0);
        let fd = fd_of(recv)?;
        let st = socktype_of(fd);
        match name_socketaddr(fd, "getpeername(2)", libc::getpeername)? {
            Some(a) => Ok(super::addrinfo::from_socketaddr(a, st, protocol_for(st))),
            None => Err(not_impl_error!("remote_address for this socket family is not supported")),
        }
    }
    // `#setsockopt(level, optname, value)`, or `#setsockopt(socket_option)` --
    // set an int/bytes socket option.
    def "setsockopt"(recv, args, _block) {
        arity!(args, 1..=3);
        let fd = fd_of(recv)?;
        let (level, optname, val) = match super::option::parts(&args[0]) {
            Some(parts) => parts,
            None => {
                arity!(args, 3);
                let level = super::option::opt_int(&args[0], None)?;
                let optname = super::option::opt_int(&args[1], Some(level))?;
                (level, optname, optval_bytes(&args[2])?)
            }
        };
        // SAFETY: `val`'s pointer/len describe an initialized buffer.
        let rc = unsafe {
            libc::setsockopt(
                fd,
                level,
                optname,
                val.as_ptr() as *const libc::c_void,
                val.len() as libc::socklen_t,
            )
        };
        if rc != 0 {
            return Err(errno_error("setsockopt(2)"));
        }
        Ok(RubyValue::Int(0))
    }
    // `#getsockopt(level, optname)` -- the option's current value as a
    // `Socket::Option`, tagged with the socket's own address family.
    def "getsockopt"(recv, args, _block) {
        arity!(args, 2);
        let fd = fd_of(recv)?;
        let level = super::option::opt_int(&args[0], None)?;
        let optname = super::option::opt_int(&args[1], Some(level))?;
        // 256 bytes holds every option the kernel answers here; `len` reports
        // how many it actually wrote.
        let mut buf = vec![0u8; 256];
        let mut len = buf.len() as libc::socklen_t;
        // SAFETY: `buf` is `len` writable bytes, which bounds the write.
        let rc = unsafe {
            libc::getsockopt(fd, level, optname, buf.as_mut_ptr() as *mut libc::c_void, &mut len)
        };
        if rc != 0 {
            return Err(errno_error("getsockopt(2)"));
        }
        buf.truncate(len as usize);
        Ok(super::option::from_raw(family_of(fd), level, optname, buf))
    }
    // `#send(mesg, flags = 0[, dest])` -- write bytes, answering the count.
    // A destination address (for unconnected datagram sockets) is out of scope.
    def "send"(recv, args, _block) {
        arity!(args, 1..=3);
        let fd = fd_of(recv)?;
        let data = crate::builtins::convert::to_rstr(&args[0])?.lock().bytes().to_vec();
        let flags = match args.get(1) {
            None | Some(RubyValue::Nil) => 0,
            Some(v) => int_arg(v)?,
        };
        // SAFETY: `data` is an initialized buffer of `data.len()` bytes.
        let n = crate::gvl::without_gvl(|| unsafe {
            libc::send(fd, data.as_ptr() as *const libc::c_void, data.len(), flags)
        });
        if n < 0 {
            return Err(errno_error("send(2)"));
        }
        Ok(RubyValue::Int(n as i64))
    }
    // `#recv(maxlen, flags = 0)` -- read up to `maxlen` bytes (ASCII-8BIT).
    def "recv"(recv, args, _block) {
        arity!(args, 1..=2);
        let fd = fd_of(recv)?;
        let maxlen = int_arg(&args[0])?.max(0) as usize;
        let flags = match args.get(1) {
            None | Some(RubyValue::Nil) => 0,
            Some(v) => int_arg(v)?,
        };
        let mut buf = vec![0u8; maxlen];
        // SAFETY: `buf` is `maxlen` writable bytes.
        let n = crate::gvl::without_gvl(|| unsafe {
            libc::recv(fd, buf.as_mut_ptr() as *mut libc::c_void, maxlen, flags)
        });
        if n < 0 {
            return Err(errno_error("recv(2)"));
        }
        buf.truncate(n as usize);
        Ok(super::binary_string(buf))
    }
    // `#recv_nonblock(maxlen, flags = 0, exception: true)` -- read only what
    // has already arrived; see `IO#read_nonblock`.
    def "recv_nonblock"(recv, args, _block) {
        let raises = crate::builtins::io::nonblock_raises(args);
        let positional = crate::builtins::io::kw_strip(args);
        arity!(positional, 1..=2);
        let fd = fd_of(recv)?;
        crate::builtins::io::set_fd_nonblock(fd, true)?;
        let maxlen = int_arg(&positional[0])?.max(0) as usize;
        let flags = match positional.get(1) {
            None | Some(RubyValue::Nil) => 0,
            Some(v) => int_arg(v)?,
        };
        let mut buf = vec![0u8; maxlen];
        // SAFETY: `buf` is `maxlen` writable bytes.
        let n = unsafe { libc::recv(fd, buf.as_mut_ptr() as *mut libc::c_void, maxlen, flags) };
        if n < 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EAGAIN) {
                return crate::builtins::io::would_block(false, raises, "recvfrom(2)");
            }
            return Err(errno_error("recv(2)"));
        }
        buf.truncate(n as usize);
        Ok(super::binary_string(buf))
    }
    // `#shutdown(how = SHUT_RDWR)` -- disable further sends and/or receives.
    def "shutdown"(recv, args, _block) {
        arity!(args, 0..=1);
        let fd = fd_of(recv)?;
        let how = match args.first() {
            None | Some(RubyValue::Nil) => libc::SHUT_RDWR,
            Some(v) => int_arg(v)?,
        };
        // SAFETY: a plain syscall on an owned fd.
        if unsafe { libc::shutdown(fd, how) } != 0 {
            return Err(errno_error("shutdown(2)"));
        }
        Ok(RubyValue::Int(0))
    }
    // `#close_read` -- shut down the receive half (SHUT_RD).
    def "close_read"(recv, args, _block) {
        arity!(args, 0);
        // SAFETY: a plain syscall on an owned fd.
        unsafe { libc::shutdown(fd_of(recv)?, libc::SHUT_RD) };
        Ok(RubyValue::Nil)
    }
    // `#close_write` -- shut down the send half (SHUT_WR).
    def "close_write"(recv, args, _block) {
        arity!(args, 0);
        // SAFETY: a plain syscall on an owned fd.
        unsafe { libc::shutdown(fd_of(recv)?, libc::SHUT_WR) };
        Ok(RubyValue::Nil)
    }
    // Reverse DNS is never performed here, so this is effectively always true;
    // the setter is accepted and ignored.
    def "do_not_reverse_lookup"(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(true))
    }
    def "do_not_reverse_lookup="(_recv, args, _block) {
        arity!(args, 1);
        Ok(args[0].clone())
    }
}

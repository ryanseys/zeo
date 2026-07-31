//! `UDPSocket < IPSocket` -- a datagram socket. `UDPSocket.new` opens an unbound
//! socket; `#bind`/`#connect` associate a local/peer address; `#send` writes a
//! datagram (optionally to an explicit `host, port`); `#recvfrom` (inherited
//! from `IPSocket`) answers `[mesg, [family, port, host, ip]]`.

use std::os::fd::RawFd;

use super::{errno_error, resolve_one, socketaddr_to_raw};
use crate::builtins::io::{socket_from_raw_fd, socket_raw_fd};
use crate::builtins::{arity, io_error};
use crate::{RubyValue, Signal};
use zeo_abi::UDP_SOCKET_CLASS;
use zeo_macros::ruby_class;

fn fd_of(recv: &RubyValue) -> Result<RawFd, Signal> {
    socket_raw_fd(recv).ok_or_else(|| io_error!("closed stream"))
}

/// `libc::bind`/`connect` the fd to a resolved `host:port`.
fn associate(
    fd: RawFd,
    host: &str,
    port: u16,
    ctx: &str,
    f: unsafe extern "C" fn(RawFd, *const libc::sockaddr, libc::socklen_t) -> libc::c_int,
) -> Result<(), Signal> {
    let addr = resolve_one(host, port)?;
    let (storage, len) = socketaddr_to_raw(&addr);
    // SAFETY: `storage`/`len` describe an initialized sockaddr.
    if unsafe { f(fd, &storage as *const _ as *const libc::sockaddr, len) } != 0 {
        return Err(errno_error(ctx));
    }
    Ok(())
}

ruby_class! {
    UDPSocket = zeo_abi::UDP_SOCKET_CLASS < zeo_abi::IP_SOCKET_CLASS;

    // `UDPSocket.new(family = AF_INET)` -- an unbound datagram socket.
    def self."new" | "open"(_recv, *args, &_block) {
        arity!(args, 0..=1);
        let family = match args.first() {
            None | Some(RubyValue::Nil) => libc::AF_INET,
            Some(RubyValue::Int(n)) => *n as libc::c_int,
            Some(v) => crate::builtins::convert::to_index(v)? as libc::c_int,
        };
        // SAFETY: a plain socket(2) call.
        let fd = unsafe { libc::socket(family, libc::SOCK_DGRAM, 0) };
        if fd < 0 {
            return Err(errno_error("socket(2)"));
        }
        // SAFETY: `fd` is a fresh, solely-owned descriptor.
        Ok(unsafe { socket_from_raw_fd(fd, UDP_SOCKET_CLASS) })
    }

    // `#bind(host, port)` -- associate a local address (port 0 = kernel-chosen).
    def "bind"(recv, *args, &_block) {
        arity!(args, 2);
        let (host, port) = (args[0].to_display_string(), super::port_of(&args[1])?);
        associate(fd_of(recv)?, &host, port, "bind(2)", libc::bind)?;
        Ok(RubyValue::Int(0))
    }
    // `#connect(host, port)` -- set the default peer for `#send`/`IO#read`.
    def "connect"(recv, *args, &_block) {
        arity!(args, 2);
        let (host, port) = (args[0].to_display_string(), super::port_of(&args[1])?);
        associate(fd_of(recv)?, &host, port, "connect(2)", libc::connect)?;
        Ok(RubyValue::Int(0))
    }
    // `#send(mesg, flags[, host, port])` -- a datagram, to the connected peer or
    // (with host+port) an explicit destination. Answers the byte count.
    def "send"(recv, *args, &_block) {
        arity!(args, 2..=4);
        let fd = fd_of(recv)?;
        let data = crate::builtins::convert::to_rstr(&args[0])?.lock().bytes().to_vec();
        let flags = crate::builtins::convert::to_index(&args[1])? as libc::c_int;
        let n = if args.len() >= 4 {
            let (host, port) = (args[2].to_display_string(), super::port_of(&args[3])?);
            let addr = resolve_one(&host, port)?;
            let (storage, len) = socketaddr_to_raw(&addr);
            // SAFETY: `data` and `storage` are initialized buffers.
            crate::gvl::without_gvl(|| unsafe {
                libc::sendto(
                    fd,
                    data.as_ptr() as *const libc::c_void,
                    data.len(),
                    flags,
                    &storage as *const _ as *const libc::sockaddr,
                    len,
                )
            })
        } else {
            // SAFETY: `data` is an initialized buffer.
            crate::gvl::without_gvl(|| unsafe {
                libc::send(fd, data.as_ptr() as *const libc::c_void, data.len(), flags)
            })
        };
        if n < 0 {
            return Err(errno_error("send(2)"));
        }
        Ok(RubyValue::Int(n as i64))
    }
}

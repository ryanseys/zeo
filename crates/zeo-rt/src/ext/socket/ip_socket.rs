//! `IPSocket < BasicSocket` -- the `addr`/`peeraddr`/`recvfrom` shared by the
//! IP-family sockets (`TCPSocket`, `TCPServer`, `UDPSocket`), reached through
//! the ancestor walk. `#addr`/`#peeraddr` answer the `[family, port, host, ip]`
//! array (host == ip, reverse DNS off); `#recvfrom` answers `[mesg, addr_array]`
//! (unlike `Socket#recvfrom`, whose second element is an `Addrinfo`). The class
//! method `IPSocket.getaddress` resolves a host name without opening a socket.

use std::os::fd::RawFd;

use super::{errno_error, ip_addr_array, raw_to_socketaddr};
use crate::builtins::{io_error};
use crate::{RubyValue, Signal};
use zeo_macros::ruby_class;

fn fd_of(recv: &RubyValue) -> Result<RawFd, Signal> {
    crate::builtins::io::socket_raw_fd(recv).ok_or_else(|| io_error!("closed stream"))
}

/// The local/peer `[family, port, host, ip]` array via `getsockname`/
/// `getpeername` (`f`).
fn addr_array(
    recv: &RubyValue,
    ctx: &str,
    f: unsafe extern "C" fn(RawFd, *mut libc::sockaddr, *mut libc::socklen_t) -> libc::c_int,
) -> Result<RubyValue, Signal> {
    let fd = fd_of(recv)?;
    // SAFETY: a zeroed sockaddr_storage is valid; `len` bounds the write.
    let addr = unsafe {
        let mut storage: libc::sockaddr_storage = std::mem::zeroed();
        let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        if f(fd, &mut storage as *mut _ as *mut libc::sockaddr, &mut len) != 0 {
            return Err(errno_error(ctx));
        }
        raw_to_socketaddr(&storage)
    };
    match addr {
        Some(a) => Ok(ip_addr_array(&a)),
        None => Ok(RubyValue::Nil),
    }
}

ruby_class! {
    IPSocket = zeo_abi::IP_SOCKET_CLASS < zeo_abi::BASIC_SOCKET_CLASS;

    // `IPSocket.getaddress(host)` -- the first address the resolver answers for
    // `host`, as a String. A numeric host resolves to itself.
    def self."getaddress"(_recv, arg) {
        let host = crate::builtins::convert::to_rstr(arg)?.lock().to_utf8_lossy().into_owned();
        Ok(RubyValue::Str(crate::string_new(
            super::resolve_one(&host, 0)?.ip().to_string(),
        )))
    }

    // `#addr` -- `[family, port, hostname, ip]` for the local address.
    // `reverse_lookup` is ignored -- DNS is never done.
    def "addr"(recv, _reverse_lookup?) {
        addr_array(recv, "getsockname(2)", libc::getsockname)
    }
    // `#peeraddr` -- the same array for the connected peer.
    def "peeraddr"(recv, _arg?) {
        addr_array(recv, "getpeername(2)", libc::getpeername)
    }
    // `#recvfrom(maxlen, flags = 0)` -- `[mesg, [family, port, host, ip]]`.
    def "recvfrom" cfunc (recv, arg1, arg2?) {
        let fd = fd_of(recv)?;
        let maxlen = crate::builtins::convert::to_index(arg1)?.max(0) as usize;
        let flags = match arg2 {
            None | Some(RubyValue::Nil) => 0,
            Some(v) => crate::builtins::convert::to_index(v)? as libc::c_int,
        };
        let mut buf = vec![0u8; maxlen];
        // SAFETY: zeroed storage is valid; `alen` bounds the write; `buf` is
        // `maxlen` writable bytes.
        let (n, storage) = unsafe {
            let mut storage: libc::sockaddr_storage = std::mem::zeroed();
            let mut alen = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
            let n = crate::gvl::without_gvl(|| {
                libc::recvfrom(
                    fd,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    maxlen,
                    flags,
                    &mut storage as *mut _ as *mut libc::sockaddr,
                    &mut alen,
                )
            });
            (n, storage)
        };
        if n < 0 {
            return Err(errno_error("recvfrom(2)"));
        }
        buf.truncate(n as usize);
        let sender = match raw_to_socketaddr(&storage) {
            Some(a) => ip_addr_array(&a),
            None => RubyValue::Nil,
        };
        Ok(RubyValue::Array(crate::array_new(vec![
            super::binary_string(buf),
            sender,
        ])))
    }
}

//! `socket` (CRuby's C `socket` extension), implemented over `std::net` and
//! `libc`. `require "socket"` activates the whole hierarchy, mirroring CRuby's:
//!
//! ```text
//! IO
//!  └ BasicSocket        raw-fd ops shared by every socket (basic_socket.rs)
//!     ├ IPSocket        addr/peeraddr/recvfrom for the IP families (ip_socket.rs)
//!     │  ├ TCPSocket    a connected TCP stream (tcp_socket.rs)
//!     │  │  └ TCPServer a listening TCP socket (tcp_server.rs)
//!     │  └ UDPSocket    a datagram socket (udp_socket.rs)
//!     ├ Socket          the generic BSD-sockets API (socket.rs)
//!     └ UNIXSocket      an AF_UNIX stream (unix_socket.rs)
//!        └ UNIXServer   a listening AF_UNIX socket (unix_server.rs)
//! Addrinfo              a resolved address value (addrinfo.rs)
//! Socket::Option        one socket option's value (option.rs)
//! ```
//!
//! One file per class, each a `ruby_class!`. A socket VALUE is an `RIo` over the
//! descriptor (so it inherits IO's read/write/gets), tagged with its socket
//! class; the raw fd is reached through [`crate::builtins::io::socket_raw_fd`].
//! The `(host, port)` argument parsing, address<->`sockaddr` marshalling, and
//! IO-error mapping shared across the classes live here.

pub(crate) mod addrinfo;
pub(crate) mod basic_socket;
pub(crate) mod ip_socket;
pub(crate) mod option;
// The generic `Socket` class file, named for its class -- the inner `socket`
// matching the gem dir is intentional (one-file-per-class), not accidental.
#[allow(clippy::module_inception)]
pub(crate) mod socket;
pub(crate) mod tcp_server;
pub(crate) mod tcp_socket;
pub(crate) mod udp_socket;
pub(crate) mod unix_server;
pub(crate) mod unix_socket;

// `bootstrap::install_core_constants` calls `crate::ext::socket::seed_socket`.
pub use socket::seed_socket;

use std::net::SocketAddr;

use crate::builtins::arg_error;
use crate::dispatch::raise_error;
use crate::{RubyValue, Signal};

/// A socket endpoint: an IP `host:port`, or an AF_UNIX path.
#[derive(Clone, Debug)]
pub(crate) enum Endpoint {
    Ip(SocketAddr),
    Unix(String),
}

/// A seconds-valued keyword (`connect_timeout:`) from the trailing options
/// Hash, as a `Duration`; `None` when absent or nil.
pub(crate) fn kwarg_secs(
    opts: Option<&RubyValue>,
    name: &str,
) -> Result<Option<std::time::Duration>, Signal> {
    let Some(RubyValue::Hash(h)) = opts else {
        return Ok(None);
    };
    let v = crate::collections::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern(name)));
    Ok(match v {
        RubyValue::Nil => None,
        RubyValue::Int(i) => Some(std::time::Duration::from_secs(i.max(0) as u64)),
        RubyValue::Float(f) => Some(std::time::Duration::from_secs_f64(f.max(0.0))),
        other => {
            return Err(crate::builtins::type_error!(
                "no implicit conversion of {} into Numeric",
                crate::builtins::class_name_of(&other)
            ));
        }
    })
}

/// A `(host, port)` pair from a `.new`/`.open` argument list. CRuby accepts
/// `(port)` or `(host, port)`, a nil host meaning `default_host`; the corpus
/// uses the explicit `(host, port)` form. A trailing `local_host`/`local_port`
/// pair (`Socket.tcp`'s bind side) is accepted and ignored -- binding the local
/// end is a separate, rarely-used capability.
pub(crate) fn host_port(args: &[RubyValue], default_host: &str) -> Result<(String, u16), Signal> {
    match args {
        [p] => Ok((default_host.to_string(), port_of(p)?)),
        [h, p] | [h, p, _] | [h, p, _, _] => {
            let host = match h {
                RubyValue::Nil => default_host.to_string(),
                other => other.to_display_string(),
            };
            Ok((host, port_of(p)?))
        }
        _ => Err(arg_error!(
            "wrong number of arguments (given {}, expected 1..2)",
            args.len()
        )),
    }
}

/// A port from an `Integer` or a numeric-looking `String` ("80"). Service-name
/// lookup (`"http"`) is out of scope -- an unparseable name is a
/// `Socket::ResolutionError`, matching CRuby's `getaddrinfo: unknown service`.
pub(crate) fn port_of(v: &RubyValue) -> Result<u16, Signal> {
    match v {
        RubyValue::Int(i) => Ok((*i).clamp(0, u16::MAX as i64) as u16),
        RubyValue::Str(s) => s.lock().to_utf8_lossy().trim().parse::<u16>().map_err(|_| {
            raise_error(
                "Socket::ResolutionError",
                "getaddrinfo: unknown service".to_string(),
            )
        }),
        other => Ok(crate::builtins::convert::to_index(other)? as u16),
    }
}

/// Resolve `(host, port)` to the first matching `SocketAddr` via the platform
/// resolver, or a `Socket::ResolutionError` (CRuby's `getaddrinfo` failure
/// class, a `SocketError`).
pub(crate) fn resolve_one(host: &str, port: u16) -> Result<SocketAddr, Signal> {
    use std::net::ToSocketAddrs;
    (host, port)
        .to_socket_addrs()
        .map_err(|e| raise_error("Socket::ResolutionError", format!("getaddrinfo: {e}")))?
        .next()
        .ok_or_else(|| {
            raise_error(
                "Socket::ResolutionError",
                "getaddrinfo: no address".to_string(),
            )
        })
}

/// One `accept(2)` that never waits: the accepted descriptor plus the peer
/// address the kernel filled, or `None` when nothing is pending. The listening
/// descriptor is marked `O_NONBLOCK` first and left that way, as CRuby leaves
/// it. Shared by every `#accept_nonblock`.
pub(crate) fn accept_nonblock_fd(
    fd: std::os::fd::RawFd,
) -> Result<Option<(std::os::fd::RawFd, libc::sockaddr_storage, libc::socklen_t)>, Signal> {
    crate::builtins::io::set_fd_nonblock(fd, true)?;
    // SAFETY: a zeroed sockaddr_storage is valid; `len` bounds what the kernel
    // writes into it.
    let (nfd, storage, len) = unsafe {
        let mut storage: libc::sockaddr_storage = std::mem::zeroed();
        let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        let nfd = libc::accept(fd, &mut storage as *mut _ as *mut libc::sockaddr, &mut len);
        (nfd, storage, len)
    };
    if nfd >= 0 {
        return Ok(Some((nfd, storage, len)));
    }
    // EWOULDBLOCK is EAGAIN on every platform zeo builds for, so one arm covers
    // both spellings.
    if std::io::Error::last_os_error().raw_os_error() == Some(libc::EAGAIN) {
        return Ok(None);
    }
    Err(errno_error("accept(2)"))
}

pub(crate) fn map_io_err(e: &std::io::Error, ctx: &str) -> Signal {
    raise_error("Errno::ECONNREFUSED", format!("{ctx}: {e}"))
}

/// A `libc` syscall failure as the matching `Errno::*` exception (by raw errno),
/// falling back to `SystemCallError` for an unmapped code -- CRuby's shape.
pub(crate) fn errno_error(ctx: &str) -> Signal {
    let e = std::io::Error::last_os_error();
    let class = match e.raw_os_error() {
        Some(libc::EACCES) => "Errno::EACCES",
        Some(libc::EADDRINUSE) => "Errno::EADDRINUSE",
        Some(libc::EADDRNOTAVAIL) => "Errno::EADDRNOTAVAIL",
        Some(libc::ECONNREFUSED) => "Errno::ECONNREFUSED",
        Some(libc::ECONNRESET) => "Errno::ECONNRESET",
        Some(libc::ECONNABORTED) => "Errno::ECONNABORTED",
        Some(libc::EHOSTUNREACH) => "Errno::EHOSTUNREACH",
        Some(libc::ETIMEDOUT) => "Errno::ETIMEDOUT",
        Some(libc::EINVAL) => "Errno::EINVAL",
        Some(libc::EISCONN) => "Errno::EISCONN",
        Some(libc::ENOTCONN) => "Errno::ENOTCONN",
        Some(libc::EPIPE) => "Errno::EPIPE",
        _ => "SystemCallError",
    };
    raise_error(class, format!("{ctx} - {e}"))
}

// ---------------------------------------------------------------------------
// sockaddr <-> SocketAddr marshalling (libc, platform-exact)
// ---------------------------------------------------------------------------

/// True on platforms whose `sockaddr` carries a leading `sa_len` byte (the BSDs,
/// macOS) -- so the packed `sockaddr_in`/`sockaddr_in6` and `ss_family` handling
/// match the host's C layout exactly.
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
const HAS_SA_LEN: bool = true;
#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
)))]
const HAS_SA_LEN: bool = false;

/// Fill a zeroed `sockaddr_storage` from a `SocketAddr`, answering the used
/// length. The bytes match the host `libc` `sockaddr_in`/`sockaddr_in6` exactly
/// (including the BSD `sin_len`), so `getsockname`/`pack_sockaddr_in` output is
/// byte-for-byte CRuby's.
pub(crate) fn socketaddr_to_raw(addr: &SocketAddr) -> (libc::sockaddr_storage, libc::socklen_t) {
    // SAFETY: an all-zero sockaddr_storage is a valid (AF_UNSPEC) value; each
    // arm then writes only the fields of the family it selects.
    unsafe {
        let mut storage: libc::sockaddr_storage = std::mem::zeroed();
        match addr {
            SocketAddr::V4(a) => {
                let len = std::mem::size_of::<libc::sockaddr_in>();
                let sin = &mut *(&mut storage as *mut _ as *mut libc::sockaddr_in);
                sin.sin_family = libc::AF_INET as libc::sa_family_t;
                sin.sin_port = a.port().to_be();
                sin.sin_addr = libc::in_addr {
                    s_addr: u32::from_ne_bytes(a.ip().octets()),
                };
                if HAS_SA_LEN {
                    // sin_len is the first byte; ss_len aliases it.
                    (&mut storage as *mut _ as *mut u8).write(len as u8);
                }
                (storage, len as libc::socklen_t)
            }
            SocketAddr::V6(a) => {
                let len = std::mem::size_of::<libc::sockaddr_in6>();
                let sin6 = &mut *(&mut storage as *mut _ as *mut libc::sockaddr_in6);
                sin6.sin6_family = libc::AF_INET6 as libc::sa_family_t;
                sin6.sin6_port = a.port().to_be();
                sin6.sin6_addr = libc::in6_addr {
                    s6_addr: a.ip().octets(),
                };
                sin6.sin6_scope_id = a.scope_id();
                if HAS_SA_LEN {
                    (&mut storage as *mut _ as *mut u8).write(len as u8);
                }
                (storage, len as libc::socklen_t)
            }
        }
    }
}

/// Parse a filled `sockaddr_storage` back to a `SocketAddr` (IP families only;
/// `None` for AF_UNIX / unknown).
pub(crate) fn raw_to_socketaddr(storage: &libc::sockaddr_storage) -> Option<SocketAddr> {
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6};
    // SAFETY: we read only the family we matched, and the storage is large
    // enough for either sockaddr_in/sockaddr_in6.
    unsafe {
        match storage.ss_family as i32 {
            libc::AF_INET => {
                let sin = &*(storage as *const _ as *const libc::sockaddr_in);
                // `s_addr` is already network-order bytes -- read them as octets
                // directly (going through a host-order u32 would byte-swap).
                let ip = Ipv4Addr::from(sin.sin_addr.s_addr.to_ne_bytes());
                Some(SocketAddr::V4(SocketAddrV4::new(
                    ip,
                    u16::from_be(sin.sin_port),
                )))
            }
            libc::AF_INET6 => {
                let sin6 = &*(storage as *const _ as *const libc::sockaddr_in6);
                let ip = Ipv6Addr::from(sin6.sin6_addr.s6_addr);
                Some(SocketAddr::V6(SocketAddrV6::new(
                    ip,
                    u16::from_be(sin6.sin6_port),
                    0,
                    sin6.sin6_scope_id,
                )))
            }
            _ => None,
        }
    }
}

/// The byte offset of `sun_path` within `sockaddr_un` (the fixed header:
/// `sun_len`+`sun_family` on the BSDs, `sun_family` on Linux).
fn sun_path_offset() -> usize {
    // SAFETY: reading a field offset off a zeroed value; no deref of the field.
    unsafe {
        let base: libc::sockaddr_un = std::mem::zeroed();
        let path_ptr = &base.sun_path as *const _ as usize;
        path_ptr - (&base as *const _ as usize)
    }
}

/// Fill a `sockaddr_un` from a filesystem path, answering the used length
/// (`SUN_LEN`: header + path, no trailing NUL). A too-long path is an
/// `ArgumentError`, CRuby's shape.
pub(crate) fn pack_unix_sockaddr(
    path: &str,
) -> Result<(libc::sockaddr_storage, libc::socklen_t), Signal> {
    // SAFETY: a zeroed sockaddr_storage is valid; we write only sun_family and
    // the bounds-checked sun_path bytes.
    unsafe {
        let mut storage: libc::sockaddr_storage = std::mem::zeroed();
        let su = &mut *(&mut storage as *mut _ as *mut libc::sockaddr_un);
        su.sun_family = libc::AF_UNIX as libc::sa_family_t;
        let bytes = path.as_bytes();
        if bytes.len() >= su.sun_path.len() {
            return Err(arg_error!(
                "too long unix socket path ({} bytes given)",
                bytes.len()
            ));
        }
        for (i, &b) in bytes.iter().enumerate() {
            su.sun_path[i] = b as libc::c_char;
        }
        let len = sun_path_offset() + bytes.len();
        if HAS_SA_LEN {
            (&mut storage as *mut _ as *mut u8).write(len as u8);
        }
        Ok((storage, len as libc::socklen_t))
    }
}

/// The path out of a filled AF_UNIX `sockaddr_un` (empty for an unnamed socket).
pub(crate) fn parse_unix_sockaddr(storage: &libc::sockaddr_storage) -> String {
    // SAFETY: only read when the caller knows the family is AF_UNIX.
    unsafe {
        let su = &*(storage as *const _ as *const libc::sockaddr_un);
        let raw: &[u8] =
            std::slice::from_raw_parts(su.sun_path.as_ptr() as *const u8, su.sun_path.len());
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        String::from_utf8_lossy(&raw[..end]).into_owned()
    }
}

/// The packed `sockaddr` bytes for an IP address -- the String `getsockname`,
/// `Addrinfo#to_sockaddr`, and `Socket.pack_sockaddr_in` answer.
pub(crate) fn pack_ip_sockaddr(addr: &SocketAddr) -> Vec<u8> {
    let (storage, len) = socketaddr_to_raw(addr);
    // SAFETY: `len` bytes of `storage` were initialized by socketaddr_to_raw.
    unsafe { std::slice::from_raw_parts(&storage as *const _ as *const u8, len as usize).to_vec() }
}

/// An ASCII-8BIT String value from raw bytes (what the sockaddr accessors
/// answer).
pub(crate) fn binary_string(bytes: Vec<u8>) -> RubyValue {
    RubyValue::Str(crate::string_from_bytes(bytes, crate::encoding::ASCII_8BIT))
}

/// The `[family, port, hostname, ip]` array `IPSocket#addr`/`#peeraddr`/
/// `#recvfrom` answer. Reverse DNS is never performed, so `hostname == ip`
/// (matching CRuby with `do_not_reverse_lookup`).
pub(crate) fn ip_addr_array(addr: &SocketAddr) -> RubyValue {
    let fam = if addr.is_ipv6() {
        "AF_INET6"
    } else {
        "AF_INET"
    };
    let ip = addr.ip().to_string();
    RubyValue::Array(crate::array_new(vec![
        RubyValue::Str(crate::string_new(fam.to_string())),
        RubyValue::Int(addr.port() as i64),
        RubyValue::Str(crate::string_new(ip.clone())),
        RubyValue::Str(crate::string_new(ip)),
    ]))
}

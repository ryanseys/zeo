//! `socket` (CRuby's C `socket` extension). The generic `Socket` class is
//! **scaffolded** -- `require "socket"` activates it and its raw-socket surface
//! (`connect`/`bind`/`send`/`recv`) raises a rescue-able `NotImplementedError`.
//!
//! `TCPServer` and `TCPSocket` ARE implemented over `std::net`: a `TCPSocket`
//! is an `RIo` over the connected socket fd (so it inherits IO's
//! read/write/gets surface, `TCPSocket < IO`), and a `TCPServer` wraps a
//! `TcpListener` whose `#accept` mints `TCPSocket`s. See docs/EXTENSIONS.md.

use std::os::unix::io::{FromRawFd, IntoRawFd};
use std::sync::Arc;

use crate::builtins::{arg_error, arity, builtin_methods, io_error, not_impl_error, type_error};
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::{RubyValue, Signal};
use zeo_abi::{ClassId, TCPSERVER_CLASS, TCPSOCKET_CLASS};

/// The shared "networking not built" error -- a real, `rescue`-able
/// `NotImplementedError` (not a panic).
fn not_implemented(method: &str) -> Signal {
    not_impl_error!("Socket#{method} is not implemented (live networking is out of scope)")
}

builtin_methods! {
    pub(crate) fn lookup;

    "connect" => fn connect(_recv, _args, _block) { Err(not_implemented("connect")) }
    "bind" => fn bind(_recv, _args, _block) { Err(not_implemented("bind")) }
    "send" => fn send(_recv, _args, _block) { Err(not_implemented("send")) }
    "recv" => fn recv(_recv, _args, _block) { Err(not_implemented("recv")) }
    "close" => fn close(_recv, _args, _block) { Err(not_implemented("close")) }
}

builtin_methods! {
    pub(crate) fn lookup_class;

    "new" | "open" => fn new_m(_recv, _args, _block) { Err(not_implemented("new")) }
    "gethostname" => fn gethostname(_recv, _args, _block) { Err(not_implemented("gethostname")) }
    "getaddrinfo" => fn getaddrinfo(_recv, _args, _block) { Err(not_implemented("getaddrinfo")) }
    "pair" => fn pair(_recv, _args, _block) { Err(not_implemented("pair")) }
}

// ---------------------------------------------------------------------------
// TCPServer -- a listening TCP socket. `TCPServer < TCPSocket < IO`.
// ---------------------------------------------------------------------------

/// A listening socket. The `TcpListener` lives behind an `Option` so `#close`
/// can drop it (a closed server accepts nothing further).
pub struct RTcpServer {
    listener: parking_lot::Mutex<Option<std::net::TcpListener>>,
}

impl RubyObject for RTcpServer {
    fn class_id(&self) -> ClassId {
        TCPSERVER_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        false
    }
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    // A listener owns a unique fd; there is no meaningful shallow copy, so a
    // `dup` answers a fresh closed server rather than aliasing the fd.
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(RTcpServer {
            listener: parking_lot::Mutex::new(None),
        })
    }
}

fn recv_server(recv: &RubyValue) -> Result<&RTcpServer, Signal> {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RTcpServer>()
            .ok_or_else(|| type_error!("not a TCPServer")),
        _ => Err(type_error!("not a TCPServer")),
    }
}

/// A `(host, port)` pair from the `.new` arguments. CRuby's `TCPServer.new`
/// accepts `(port)` or `(host, port)`, a nil host meaning "all interfaces";
/// the corpus uses the explicit `(host, port)` form.
fn host_port(args: &[RubyValue], default_host: &str) -> Result<(String, u16), Signal> {
    match args {
        [p] => Ok((default_host.to_string(), port_of(p)?)),
        [h, p] => {
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

fn port_of(v: &RubyValue) -> Result<u16, Signal> {
    match v {
        RubyValue::Int(i) => Ok((*i).clamp(0, u16::MAX as i64) as u16),
        // A service name String ("80") -- parse it as a number (name lookup is
        // out of scope).
        RubyValue::Str(s) => s
            .lock()
            .to_utf8_lossy()
            .trim()
            .parse::<u16>()
            .map_err(|_| raise_error("SocketError", "getaddrinfo: unknown service".to_string())),
        other => Ok(crate::builtins::convert::to_index(other)? as u16),
    }
}

/// `[family, port, hostname, ip]` -- the shape `#addr`/`#peeraddr` answer.
fn socket_addr_array(addr: std::net::SocketAddr) -> RubyValue {
    let ip = addr.ip().to_string();
    let family = if addr.is_ipv6() {
        "AF_INET6"
    } else {
        "AF_INET"
    };
    RubyValue::Array(crate::array_new(vec![
        RubyValue::Str(crate::string_new(family.to_string())),
        RubyValue::Int(addr.port() as i64),
        RubyValue::Str(crate::string_new(ip.clone())),
        RubyValue::Str(crate::string_new(ip)),
    ]))
}

fn map_io_err(e: &std::io::Error, ctx: &str) -> Signal {
    raise_error("Errno::ECONNREFUSED", format!("{ctx}: {e}"))
}

builtin_methods! {
    pub(crate) fn lookup_tcpserver;

    // `#accept` -- block until a client connects, answering a TCPSocket.
    // The blocking accept(2) runs on a dup(2) of the listener with the
    // mutex DROPPED (holding it for the whole wait deadlocked a sibling's
    // `#addr`/`#close` on this very server) and Gvl-released (an armed
    // `ZEO_GVL=1` holder parked here would stall the very sibling that
    // was about to connect).
    "accept" => fn accept(recv, args, _block) {
        arity!(args, 0);
        let server = recv_server(recv)?;
        let listener = {
            let guard = server.listener.lock();
            let Some(listener) = guard.as_ref() else {
                return Err(io_error!("closed stream"));
            };
            listener.try_clone().map_err(|e| map_io_err(&e, "accept(2)"))?
        };
        let stream = crate::gvl::without_gvl(|| listener.accept())
            .map(|(s, _)| s)
            .map_err(|e| map_io_err(&e, "accept(2)"))?;
        // SAFETY: `into_raw_fd` transfers ownership of the fd to the File.
        let file = unsafe { std::fs::File::from_raw_fd(stream.into_raw_fd()) };
        Ok(crate::builtins::io::socket_value(file, TCPSOCKET_CLASS))
    }
    // `#addr` -- `[family, port, hostname, ip]` for the local (bound) address.
    "addr" => fn server_addr(recv, args, _block) {
        arity!(args, 0..=1);
        let server = recv_server(recv)?;
        let guard = server.listener.lock();
        let Some(listener) = guard.as_ref() else {
            return Err(io_error!("closed stream"));
        };
        let local = listener.local_addr().map_err(|e| map_io_err(&e, "getsockname(2)"))?;
        Ok(socket_addr_array(local))
    }
    // `#listen(backlog)` -- a bound listener is already listening, so this is a
    // validated no-op answering 0.
    "listen" => fn listen(recv, args, _block) {
        arity!(args, 1);
        recv_server(recv)?;
        Ok(RubyValue::Int(0))
    }
    // `#close` -- drop the listener; further accepts raise.
    "close" => fn server_close(recv, args, _block) {
        arity!(args, 0);
        recv_server(recv)?.listener.lock().take();
        Ok(RubyValue::Nil)
    }
    "closed?" => fn server_closed(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv_server(recv)?.listener.lock().is_none()))
    }
}

builtin_methods! {
    pub(crate) fn lookup_tcpserver_class;

    // `TCPServer.new([host, ] port)` -- bind + listen (bind already listens).
    "new" | "open" => fn server_new(_recv, args, _block) {
        let (host, port) = host_port(args, "0.0.0.0")?;
        let listener = std::net::TcpListener::bind((host.as_str(), port))
            .map_err(|e| map_io_err(&e, "bind(2)"))?;
        Ok(RubyValue::Object(Arc::new(RTcpServer {
            listener: parking_lot::Mutex::new(Some(listener)),
        })))
    }
}

builtin_methods! {
    pub(crate) fn lookup_tcpsocket_class;

    // `TCPSocket.new(host, port)` -- connect; the result reads/writes as an IO.
    "new" | "open" => fn socket_new(_recv, args, _block) {
        let (host, port) = host_port(args, "127.0.0.1")?;
        // Gvl-released: connect(2) blocks until the peer answers.
        let stream = crate::gvl::without_gvl(|| std::net::TcpStream::connect((host.as_str(), port)))
            .map_err(|e| map_io_err(&e, "connect(2)"))?;
        // SAFETY: `into_raw_fd` transfers ownership of the fd to the File.
        let file = unsafe { std::fs::File::from_raw_fd(stream.into_raw_fd()) };
        Ok(crate::builtins::io::socket_value(file, TCPSOCKET_CLASS))
    }
}

/// Install `Socket`'s address-family / socket-type / protocol / option
/// constants at startup (`Socket::AF_INET6`, `Socket::SOCK_STREAM`, ...), the
/// runtime half of `zeo_abi::SOCKET_CONSTANT_NAMES`. Values come from the host
/// `libc` headers so they match the target platform exactly (Darwin's
/// `AF_INET6 == 30`, Linux's `== 10`), matching what CRuby's C `socket`
/// extension defines via `rb_define_const`. Called from
/// `bootstrap::install_core_constants` when the `socket` feature is built.
pub fn seed_socket() {
    let cid = zeo_abi::SOCKET_CLASS.0;
    for &name in zeo_abi::SOCKET_CONSTANT_NAMES {
        crate::constants::const_set(cid, name, RubyValue::Int(socket_const_value(name)));
    }
}

/// Map a `zeo_abi::SOCKET_CONSTANT_NAMES` entry to its host `libc` value. The
/// `unreachable!` makes an abi name with no value here a loud startup failure
/// rather than a silently-missing constant -- keeping the two halves in sync.
fn socket_const_value(name: &str) -> i64 {
    let v = match name {
        "AF_UNSPEC" => libc::AF_UNSPEC,
        "AF_INET" => libc::AF_INET,
        "AF_INET6" => libc::AF_INET6,
        "AF_UNIX" => libc::AF_UNIX,
        "AF_LOCAL" => libc::AF_UNIX, // AF_LOCAL is the POSIX alias of AF_UNIX.
        "PF_UNSPEC" => libc::PF_UNSPEC,
        "PF_INET" => libc::PF_INET,
        "PF_INET6" => libc::PF_INET6,
        "PF_UNIX" => libc::PF_UNIX,
        "PF_LOCAL" => libc::PF_LOCAL,
        "SOCK_STREAM" => libc::SOCK_STREAM,
        "SOCK_DGRAM" => libc::SOCK_DGRAM,
        "SOCK_RAW" => libc::SOCK_RAW,
        "SOCK_SEQPACKET" => libc::SOCK_SEQPACKET,
        "SOCK_RDM" => libc::SOCK_RDM,
        "IPPROTO_IP" => libc::IPPROTO_IP,
        "IPPROTO_ICMP" => libc::IPPROTO_ICMP,
        "IPPROTO_TCP" => libc::IPPROTO_TCP,
        "IPPROTO_UDP" => libc::IPPROTO_UDP,
        "IPPROTO_IPV6" => libc::IPPROTO_IPV6,
        "IPPROTO_RAW" => libc::IPPROTO_RAW,
        "SOL_SOCKET" => libc::SOL_SOCKET,
        "SO_REUSEADDR" => libc::SO_REUSEADDR,
        "SO_REUSEPORT" => libc::SO_REUSEPORT,
        "SO_KEEPALIVE" => libc::SO_KEEPALIVE,
        "SO_BROADCAST" => libc::SO_BROADCAST,
        "SO_LINGER" => libc::SO_LINGER,
        "SO_SNDBUF" => libc::SO_SNDBUF,
        "SO_RCVBUF" => libc::SO_RCVBUF,
        "SO_ERROR" => libc::SO_ERROR,
        "SO_TYPE" => libc::SO_TYPE,
        "SO_DONTROUTE" => libc::SO_DONTROUTE,
        "SO_OOBINLINE" => libc::SO_OOBINLINE,
        "TCP_NODELAY" => libc::TCP_NODELAY,
        "IP_TTL" => libc::IP_TTL,
        "IP_MULTICAST_TTL" => libc::IP_MULTICAST_TTL,
        "IP_MULTICAST_LOOP" => libc::IP_MULTICAST_LOOP,
        "IP_ADD_MEMBERSHIP" => libc::IP_ADD_MEMBERSHIP,
        "IP_DROP_MEMBERSHIP" => libc::IP_DROP_MEMBERSHIP,
        "IPV6_V6ONLY" => libc::IPV6_V6ONLY,
        "IPV6_MULTICAST_HOPS" => libc::IPV6_MULTICAST_HOPS,
        "IPV6_UNICAST_HOPS" => libc::IPV6_UNICAST_HOPS,
        "AI_PASSIVE" => libc::AI_PASSIVE,
        "AI_CANONNAME" => libc::AI_CANONNAME,
        "AI_NUMERICHOST" => libc::AI_NUMERICHOST,
        "AI_NUMERICSERV" => libc::AI_NUMERICSERV,
        "AI_ADDRCONFIG" => libc::AI_ADDRCONFIG,
        "AI_V4MAPPED" => libc::AI_V4MAPPED,
        "AI_ALL" => libc::AI_ALL,
        "NI_NUMERICHOST" => libc::NI_NUMERICHOST,
        "NI_NUMERICSERV" => libc::NI_NUMERICSERV,
        "NI_NOFQDN" => libc::NI_NOFQDN,
        "NI_NAMEREQD" => libc::NI_NAMEREQD,
        "NI_DGRAM" => libc::NI_DGRAM,
        "SHUT_RD" => libc::SHUT_RD,
        "SHUT_WR" => libc::SHUT_WR,
        "SHUT_RDWR" => libc::SHUT_RDWR,
        "MSG_OOB" => libc::MSG_OOB,
        "MSG_PEEK" => libc::MSG_PEEK,
        "MSG_DONTROUTE" => libc::MSG_DONTROUTE,
        "MSG_WAITALL" => libc::MSG_WAITALL,
        other => unreachable!("socket constant {other} has no libc value in seed_socket"),
    };
    v as i64
}

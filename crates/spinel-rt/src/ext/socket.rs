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

use crate::builtins::{arity, builtin_methods};
use crate::dispatch::{raise_error, RObj, RubyObject};
use crate::{RubyValue, Signal};
use spinel_abi::{ClassId, TCPSERVER_CLASS, TCPSOCKET_CLASS};

/// The shared "networking not built" error -- a real, `rescue`-able
/// `NotImplementedError` (not a panic).
fn not_implemented(method: &str) -> Signal {
    raise_error(
        "NotImplementedError",
        format!("Socket#{method} is not implemented (live networking is out of scope)"),
    )
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
        Arc::new(RTcpServer { listener: parking_lot::Mutex::new(None) })
    }
}

fn recv_server(recv: &RubyValue) -> Result<&RTcpServer, Signal> {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RTcpServer>()
            .ok_or_else(|| raise_error("TypeError", "not a TCPServer".to_string())),
        _ => Err(raise_error("TypeError", "not a TCPServer".to_string())),
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
        _ => Err(raise_error(
            "ArgumentError",
            format!("wrong number of arguments (given {}, expected 1..2)", args.len()),
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
        _ => Err(raise_error("TypeError", "no implicit conversion into Integer".to_string())),
    }
}

/// `[family, port, hostname, ip]` -- the shape `#addr`/`#peeraddr` answer.
fn socket_addr_array(addr: std::net::SocketAddr) -> RubyValue {
    let ip = addr.ip().to_string();
    let family = if addr.is_ipv6() { "AF_INET6" } else { "AF_INET" };
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
    "accept" => fn accept(recv, args, _block) {
        arity!(args, 0);
        let server = recv_server(recv)?;
        let stream = {
            let guard = server.listener.lock();
            let Some(listener) = guard.as_ref() else {
                return Err(raise_error("IOError", "closed stream".to_string()));
            };
            listener.accept().map_err(|e| map_io_err(&e, "accept(2)"))?.0
        };
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
            return Err(raise_error("IOError", "closed stream".to_string()));
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
        let stream = std::net::TcpStream::connect((host.as_str(), port))
            .map_err(|e| map_io_err(&e, "connect(2)"))?;
        // SAFETY: `into_raw_fd` transfers ownership of the fd to the File.
        let file = unsafe { std::fs::File::from_raw_fd(stream.into_raw_fd()) };
        Ok(crate::builtins::io::socket_value(file, TCPSOCKET_CLASS))
    }
}

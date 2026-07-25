//! `Addrinfo` -- a resolved socket address: an endpoint (IP `host:port` or an
//! AF_UNIX path) plus the `socktype`/`protocol` it was resolved for. This is
//! what `Socket.getaddrinfo`-family calls, `BasicSocket#local_address`/
//! `#remote_address`, and `#accept` answer.
//!
//! Semantics oracle-verified against ruby 4.0.5: `Addrinfo.tcp("127.0.0.1", 80)`
//! has `afamily AF_INET`, `socktype SOCK_STREAM`, `protocol IPPROTO_TCP`;
//! `#inspect` is `"#<Addrinfo: 127.0.0.1:80 TCP>"` (v6 bracketed, `:port` shown
//! only when non-zero, the trailing tag `TCP`/`UDP`/`SOCK_STREAM`/... derived
//! from socktype+protocol); `#to_sockaddr` is the packed `sockaddr` bytes.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::{Endpoint, binary_string, pack_ip_sockaddr, resolve_one};
use crate::builtins::{arity, arg_error};
use crate::dispatch::{RObj, RubyObject};
use crate::{ClassId, RubyValue, Signal, string_new};
use zeo_abi::ADDRINFO_CLASS;
use zeo_macros::ruby_class;

pub struct RAddrinfo {
    endpoint: Endpoint,
    /// `SOCK_STREAM`/`SOCK_DGRAM`/0 -- the socket type this address is for.
    socktype: i32,
    /// `IPPROTO_TCP`/`IPPROTO_UDP`/0.
    protocol: i32,
    frozen: AtomicBool,
}

impl RAddrinfo {
    fn new(endpoint: Endpoint, socktype: i32, protocol: i32) -> Arc<RAddrinfo> {
        Arc::new(RAddrinfo {
            endpoint,
            socktype,
            protocol,
            frozen: AtomicBool::new(false),
        })
    }
}

impl RubyObject for RAddrinfo {
    fn class_id(&self) -> ClassId {
        ADDRINFO_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Relaxed)
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let a = RAddrinfo::new(self.endpoint.clone(), self.socktype, self.protocol);
        if copy_frozen && self.is_frozen() {
            a.set_frozen();
        }
        a
    }
}

/// A ready-made `Addrinfo` value from a resolved `SocketAddr` (what a
/// `getsockname`/`accept` returns to `#local_address`/`#remote_address`).
pub(crate) fn from_socketaddr(addr: SocketAddr, socktype: i32, protocol: i32) -> RubyValue {
    RubyValue::Object(RAddrinfo::new(Endpoint::Ip(addr), socktype, protocol))
}

fn ai_of(recv: &RubyValue) -> &RAddrinfo {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RAddrinfo>()
            .expect("the Addrinfo table only dispatches on Addrinfo receivers"),
        _ => unreachable!("the Addrinfo table only dispatches on Addrinfo receivers"),
    }
}

/// The `AF_*` family of an endpoint (`pfamily` returns the same number, since
/// PF_* == AF_* on every supported platform).
fn afamily_of(ep: &Endpoint) -> i32 {
    match ep {
        Endpoint::Ip(SocketAddr::V4(_)) => libc::AF_INET,
        Endpoint::Ip(SocketAddr::V6(_)) => libc::AF_INET6,
        Endpoint::Unix(_) => libc::AF_UNIX,
    }
}

/// A `host` argument as an `IpAddr`: a numeric literal if it parses as one, else
/// resolved through the platform resolver (first result).
fn host_ip(host: &str) -> Result<IpAddr, Signal> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(ip);
    }
    Ok(resolve_one(host, 0)?.ip())
}

/// `#inspect`'s trailing type tag: `TCP`/`UDP` for the well-known pairs, else
/// the bare `SOCK_STREAM`/`SOCK_DGRAM`, else nothing (an `Addrinfo.ip`).
fn type_tag(socktype: i32, protocol: i32) -> Option<String> {
    if socktype == libc::SOCK_STREAM && protocol == libc::IPPROTO_TCP {
        Some("TCP".into())
    } else if socktype == libc::SOCK_DGRAM && protocol == libc::IPPROTO_UDP {
        Some("UDP".into())
    } else if socktype == libc::SOCK_STREAM {
        Some("SOCK_STREAM".into())
    } else if socktype == libc::SOCK_DGRAM {
        Some("SOCK_DGRAM".into())
    } else {
        None
    }
}

fn str_val(s: impl Into<String>) -> RubyValue {
    RubyValue::Str(string_new(s.into()))
}

ruby_class! {
    Addrinfo = zeo_abi::ADDRINFO_CLASS < zeo_abi::OBJECT_CLASS;

    // `Addrinfo.tcp(host, port)` -- an AF_INET/AF_INET6 stream address.
    def self."tcp"(_recv, args, _block) {
        arity!(args, 2);
        let port = super::port_of(&args[1])?;
        let ip = host_ip(&args[0].to_display_string())?;
        Ok(RubyValue::Object(RAddrinfo::new(
            Endpoint::Ip(SocketAddr::new(ip, port)),
            libc::SOCK_STREAM,
            libc::IPPROTO_TCP,
        )))
    }
    // `Addrinfo.udp(host, port)` -- an AF_INET/AF_INET6 datagram address.
    def self."udp"(_recv, args, _block) {
        arity!(args, 2);
        let port = super::port_of(&args[1])?;
        let ip = host_ip(&args[0].to_display_string())?;
        Ok(RubyValue::Object(RAddrinfo::new(
            Endpoint::Ip(SocketAddr::new(ip, port)),
            libc::SOCK_DGRAM,
            libc::IPPROTO_UDP,
        )))
    }
    // `Addrinfo.ip(host)` -- a bare IP address (port 0, no socktype/protocol).
    def self."ip"(_recv, args, _block) {
        arity!(args, 1);
        let ip = host_ip(&args[0].to_display_string())?;
        Ok(RubyValue::Object(RAddrinfo::new(
            Endpoint::Ip(SocketAddr::new(ip, 0)),
            0,
            0,
        )))
    }
    // `Addrinfo.unix(path[, socktype])` -- an AF_UNIX address (default STREAM).
    def self."unix"(_recv, args, _block) {
        arity!(args, 1..=2);
        let path = args[0].to_display_string();
        let socktype = match args.get(1) {
            Some(RubyValue::Int(n)) => *n as i32,
            _ => libc::SOCK_STREAM,
        };
        Ok(RubyValue::Object(RAddrinfo::new(Endpoint::Unix(path), socktype, 0)))
    }

    def "afamily" | "pfamily"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(afamily_of(&ai_of(recv).endpoint) as i64))
    }
    def "socktype"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(ai_of(recv).socktype as i64))
    }
    def "protocol"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(ai_of(recv).protocol as i64))
    }
    def "ip?"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(matches!(ai_of(recv).endpoint, Endpoint::Ip(_))))
    }
    def "ipv4?"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(matches!(ai_of(recv).endpoint, Endpoint::Ip(SocketAddr::V4(_)))))
    }
    def "ipv6?"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(matches!(ai_of(recv).endpoint, Endpoint::Ip(SocketAddr::V6(_)))))
    }
    def "unix?"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(matches!(ai_of(recv).endpoint, Endpoint::Unix(_))))
    }
    // `#ip_address` -- the address without the port; raises for a Unix address.
    def "ip_address"(recv, args, _block) {
        arity!(args, 0);
        match &ai_of(recv).endpoint {
            Endpoint::Ip(a) => Ok(str_val(a.ip().to_string())),
            Endpoint::Unix(_) => Err(arg_error!("need IPv4 or IPv6 address")),
        }
    }
    def "ip_port"(recv, args, _block) {
        arity!(args, 0);
        match &ai_of(recv).endpoint {
            Endpoint::Ip(a) => Ok(RubyValue::Int(a.port() as i64)),
            Endpoint::Unix(_) => Err(arg_error!("need IPv4 or IPv6 address")),
        }
    }
    def "unix_path"(recv, args, _block) {
        arity!(args, 0);
        match &ai_of(recv).endpoint {
            Endpoint::Unix(p) => Ok(str_val(p.clone())),
            Endpoint::Ip(_) => Err(arg_error!("need AF_UNIX address")),
        }
    }
    // `#to_sockaddr`/`#to_s` -- the packed `sockaddr` bytes (ASCII-8BIT). Unix
    // addresses aren't packed here (out of scope) -- a plain path String.
    def "to_sockaddr" | "to_s"(recv, args, _block) {
        arity!(args, 0);
        match &ai_of(recv).endpoint {
            Endpoint::Ip(a) => Ok(binary_string(pack_ip_sockaddr(a))),
            Endpoint::Unix(p) => Ok(str_val(p.clone())),
        }
    }
    def "inspect"(recv, args, _block) {
        arity!(args, 0);
        let ai = ai_of(recv);
        let body = match &ai.endpoint {
            Endpoint::Ip(a) => {
                let host = match a {
                    SocketAddr::V4(v4) => v4.ip().to_string(),
                    SocketAddr::V6(v6) => format!("[{}]", v6.ip()),
                };
                let with_port = if a.port() != 0 {
                    format!("{host}:{}", a.port())
                } else {
                    host
                };
                match type_tag(ai.socktype, ai.protocol) {
                    Some(tag) => format!("{with_port} {tag}"),
                    None => with_port,
                }
            }
            Endpoint::Unix(p) => match type_tag(ai.socktype, ai.protocol) {
                Some(tag) => format!("{p} {tag}"),
                None => p.clone(),
            },
        };
        Ok(str_val(format!("#<Addrinfo: {body}>")))
    }
}

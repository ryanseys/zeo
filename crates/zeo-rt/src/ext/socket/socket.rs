//! `Socket < BasicSocket` -- the generic BSD-sockets API over `libc`:
//! `Socket.new(domain, type, protocol)` then `#bind`/`#connect`/`#listen`/
//! `#accept`/`#recvfrom`, plus the class-level resolver/helpers (`gethostname`,
//! `getaddrinfo`, `socketpair`, `pack_sockaddr_in`/`unpack_sockaddr_in`). The
//! address-family / socket-type / protocol / option CONSTANTS are seeded from
//! the host `libc` headers so they match the target platform (Darwin's
//! `AF_INET6 == 30`) exactly, like CRuby's C extension.
//!
//! Oracle-verified against ruby 4.0.6 over a loopback pair: a `Socket` bound and
//! listening accepts a connected `Socket`; `recvfrom` answers `[mesg, Addrinfo]`;
//! `getaddrinfo("localhost", 80, nil, :STREAM)` resolves `127.0.0.1`/`::1`.

use std::os::fd::RawFd;

use super::{
    addrinfo, binary_string, errno_error, pack_ip_sockaddr, raw_to_socketaddr, resolve_one,
};
use crate::builtins::io::socket_from_raw_fd;
use crate::builtins::{arg_error, io_error, type_error};
use crate::dispatch::raise_error;
use crate::{RubyValue, Signal, Symbol, string_new};
use zeo_abi::SOCKET_CLASS;
use zeo_macros::ruby_class;

fn fd_of(recv: &RubyValue) -> Result<RawFd, Signal> {
    crate::builtins::io::socket_raw_fd(recv).ok_or_else(|| io_error!("closed stream"))
}

fn str_val(s: impl Into<String>) -> RubyValue {
    RubyValue::Str(string_new(s.into()))
}

/// A domain/type/protocol argument: an Integer, or one of the common CRuby
/// symbols (`:INET`, `:STREAM`, ...) with or without the `AF_`/`SOCK_` prefix.
fn sock_int(v: &RubyValue) -> Result<libc::c_int, Signal> {
    match v {
        RubyValue::Int(n) => Ok(*n as libc::c_int),
        RubyValue::Nil => Ok(0),
        RubyValue::Symbol(s) => symbol_const(&s.name()),
        RubyValue::Str(s) => symbol_const(&s.lock().to_utf8_lossy()),
        other => Ok(crate::builtins::convert::to_index(other)? as libc::c_int),
    }
}

/// Map a bare socket symbol/string (`:INET`, `"SOCK_STREAM"`, ...) to its value.
fn symbol_const(name: &str) -> Result<libc::c_int, Signal> {
    let n = name.trim_start_matches("AF_").trim_start_matches("PF_");
    let v = match n {
        "INET" => libc::AF_INET,
        "INET6" => libc::AF_INET6,
        "UNIX" | "LOCAL" => libc::AF_UNIX,
        "UNSPEC" => libc::AF_UNSPEC,
        _ => match name.trim_start_matches("SOCK_") {
            "STREAM" => libc::SOCK_STREAM,
            "DGRAM" => libc::SOCK_DGRAM,
            "RAW" => libc::SOCK_RAW,
            _ => return Err(arg_error!("unknown socket constant {name}")),
        },
    };
    Ok(v)
}

/// A `bind`/`connect` address argument: the raw bytes of a packed-sockaddr
/// String, or an `Addrinfo`'s packed form.
fn sockaddr_bytes(v: &RubyValue) -> Result<Vec<u8>, Signal> {
    match v {
        RubyValue::Str(s) => Ok(s.lock().bytes().to_vec()),
        RubyValue::Object(_) => {
            // Reuse Addrinfo#to_sockaddr through dispatch (works for any object
            // answering it), so an Addrinfo or a duck both bind.
            let packed = crate::dispatch::send_value(v, Symbol::intern("to_sockaddr"), &[], None)?;
            match packed {
                RubyValue::Str(s) => Ok(s.lock().bytes().to_vec()),
                _ => Err(type_error!("expected a packed sockaddr String")),
            }
        }
        other => Err(type_error!(
            "no implicit conversion of {} into String",
            crate::builtins::class_name_of(other)
        )),
    }
}

/// Parse `maxlen` bytes from a `recvfrom` fd, answering `[mesg, Addrinfo]`.
fn do_recvfrom(fd: RawFd, maxlen: usize, flags: libc::c_int) -> Result<RubyValue, Signal> {
    let mut buf = vec![0u8; maxlen];
    // SAFETY: a zeroed sockaddr_storage is valid; `len` bounds the write; `buf`
    // is `maxlen` writable bytes.
    let (n, storage, alen) = unsafe {
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
        (n, storage, alen)
    };
    if n < 0 {
        return Err(errno_error("recvfrom(2)"));
    }
    buf.truncate(n as usize);
    let sender = if alen > 0 {
        match raw_to_socketaddr(&storage) {
            Some(a) => addrinfo::from_socketaddr(a, libc::SOCK_DGRAM, libc::IPPROTO_UDP),
            None => RubyValue::Nil,
        }
    } else {
        RubyValue::Nil
    };
    Ok(RubyValue::Array(crate::array_new(vec![
        binary_string(buf),
        sender,
    ])))
}

ruby_class! {
    Socket = zeo_abi::SOCKET_CLASS < zeo_abi::BASIC_SOCKET_CLASS;

    // `Socket.new(domain, type, protocol = 0)` -- a raw socket descriptor.
    def self."new" | "open" cfunc (_recv, arg1, arg2, arg3?) {
        let (domain, ty) = (sock_int(arg1)?, sock_int(arg2)?);
        let proto = match arg3 {
            None | Some(RubyValue::Nil) => 0,
            Some(v) => sock_int(v)?,
        };
        // SAFETY: a plain socket(2) call.
        let fd = unsafe { libc::socket(domain, ty, proto) };
        if fd < 0 {
            return Err(errno_error("socket(2)"));
        }
        // SAFETY: `fd` is a fresh, solely-owned descriptor.
        Ok(unsafe { socket_from_raw_fd(fd, SOCKET_CLASS) })
    }
    // `Socket.tcp(host, port, local_host = nil, local_port = nil, **opts)` --
    // a connected TCPSocket, or, with a block, that socket yielded and then
    // CLOSED (the shape net/http probes for and uses). `connect_timeout:` is
    // honoured; the other timeout keywords are accepted and ignored, since
    // resolution and connect happen in one blocking step here.
    def self."tcp"(_recv, host, port, _local_host?, _local_port?, **opts, &block) {
        let (host, port) = super::host_port(&[host.clone(), port.clone()], "127.0.0.1")?;
        let addr = resolve_one(&host, port)?;
        let timeout = super::kwarg_secs(opts, "connect_timeout")?;
        // Gvl-released: connect(2) blocks until the peer answers.
        let stream = crate::gvl::without_gvl(|| match timeout {
            Some(t) => std::net::TcpStream::connect_timeout(&addr, t),
            None => std::net::TcpStream::connect(addr),
        })
        .map_err(|e| super::map_io_err(&e, "connect(2)"))?;
        // SAFETY: `into_raw_fd` yields a fresh, solely-owned descriptor.
        let sock = unsafe {
            crate::builtins::io::socket_from_raw_fd(
                std::os::unix::io::IntoRawFd::into_raw_fd(stream),
                zeo_abi::TCPSOCKET_CLASS,
            )
        };
        let Some(RubyValue::Proc(p)) = block else {
            return Ok(sock);
        };
        let out = p.call(std::slice::from_ref(&sock));
        let _ = crate::dispatch::send_value(&sock, crate::Symbol::intern("close"), &[], None);
        out
    }
    // `Socket.gethostname` -- the host's name.
    def self."gethostname"(_recv) {
        let mut buf = vec![0u8; 256];
        // SAFETY: `buf` is 256 writable bytes; gethostname NUL-terminates.
        let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
        if rc != 0 {
            return Err(errno_error("gethostname(3)"));
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        buf.truncate(end);
        Ok(str_val(String::from_utf8_lossy(&buf).into_owned()))
    }
    // `Socket.getaddrinfo(host, service[, family, socktype, protocol, flags])`
    // -- `[family_name, port, canonname, addr, afamily, socktype, protocol]`
    // rows. Resolution runs through `libc::getaddrinfo`.
    def self."getaddrinfo" cfunc (_recv, _nodename, _service, _family?, _socktype?, _protocol?, _flags?) {
        getaddrinfo(__args)
    }
    // `Socket.socketpair(domain, type, protocol = 0)` (aka `pair`) -- a
    // connected pair of `Socket`s (AF_UNIX in practice).
    def self."socketpair" | "pair" cfunc (_recv, arg1, arg2, arg3?) {
        let (domain, ty) = (sock_int(arg1)?, sock_int(arg2)?);
        let proto = match arg3 {
            None | Some(RubyValue::Nil) => 0,
            Some(v) => sock_int(v)?,
        };
        let mut fds = [0 as libc::c_int; 2];
        // SAFETY: socketpair fills the two-element `fds` array.
        if unsafe { libc::socketpair(domain, ty, proto, fds.as_mut_ptr()) } != 0 {
            return Err(errno_error("socketpair(2)"));
        }
        // SAFETY: each fd is a fresh, solely-owned descriptor.
        let (a, b) = unsafe {
            (
                socket_from_raw_fd(fds[0], SOCKET_CLASS),
                socket_from_raw_fd(fds[1], SOCKET_CLASS),
            )
        };
        Ok(RubyValue::Array(crate::array_new(vec![a, b])))
    }
    // `Socket.pack_sockaddr_in(port, host)` (aka `sockaddr_in`) -- packed bytes.
    def self."pack_sockaddr_in" | "sockaddr_in"(_recv, arg1, arg2) {
        let port = super::port_of(arg1)?;
        let host = (*arg2).to_display_string();
        let ip = host.parse::<std::net::IpAddr>()
            .map(|ip| std::net::SocketAddr::new(ip, port))
            .or_else(|_| resolve_one(&host, port))?;
        Ok(binary_string(pack_ip_sockaddr(&ip)))
    }
    // `Socket.unpack_sockaddr_in(sockaddr)` -- `[port, ip_string]`.
    def self."unpack_sockaddr_in"(_recv, arg) {
        let bytes = match arg {
            RubyValue::Str(s) => s.lock().bytes().to_vec(),
            other => return Err(type_error!(
                "no implicit conversion of {} into String",
                crate::builtins::class_name_of(other)
            )),
        };
        let addr = unpack_sockaddr_in(&bytes)?;
        Ok(RubyValue::Array(crate::array_new(vec![
            RubyValue::Int(addr.port() as i64),
            str_val(addr.ip().to_string()),
        ])))
    }

    // `#bind(sockaddr)` -- bind to a local packed-sockaddr / Addrinfo.
    def "bind"(recv, arg) {
        let fd = fd_of(recv)?;
        let sa = sockaddr_bytes(arg)?;
        // SAFETY: `sa` describes `sa.len()` initialized sockaddr bytes.
        let rc = unsafe {
            libc::bind(fd, sa.as_ptr() as *const libc::sockaddr, sa.len() as libc::socklen_t)
        };
        if rc != 0 {
            return Err(errno_error("bind(2)"));
        }
        Ok(RubyValue::Int(0))
    }
    // `#connect(sockaddr)` -- connect to a peer.
    def "connect"(recv, arg) {
        let fd = fd_of(recv)?;
        let sa = sockaddr_bytes(arg)?;
        // SAFETY: as bind.
        let rc = crate::gvl::without_gvl(|| unsafe {
            libc::connect(fd, sa.as_ptr() as *const libc::sockaddr, sa.len() as libc::socklen_t)
        });
        if rc != 0 {
            return Err(errno_error("connect(2)"));
        }
        Ok(RubyValue::Int(0))
    }
    // `#listen(backlog)` -- mark a bound socket as accepting connections.
    def "listen"(recv, arg) {
        let fd = fd_of(recv)?;
        let backlog = crate::builtins::convert::to_index(arg)? as libc::c_int;
        // SAFETY: a plain listen(2) on an owned fd.
        if unsafe { libc::listen(fd, backlog) } != 0 {
            return Err(errno_error("listen(2)"));
        }
        Ok(RubyValue::Int(0))
    }
    // `#accept` -- block for a connection, answering `[Socket, Addrinfo]`.
    def "accept"(recv) {
        let fd = fd_of(recv)?;
        // SAFETY: a zeroed sockaddr_storage is valid; `len` bounds the write.
        let (nfd, storage, alen) = unsafe {
            let mut storage: libc::sockaddr_storage = std::mem::zeroed();
            let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
            let nfd = crate::gvl::without_gvl(|| {
                libc::accept(fd, &mut storage as *mut _ as *mut libc::sockaddr, &mut len)
            });
            (nfd, storage, len)
        };
        if nfd < 0 {
            return Err(errno_error("accept(2)"));
        }
        // SAFETY: `nfd` is a fresh, solely-owned descriptor.
        let sock = unsafe { socket_from_raw_fd(nfd, SOCKET_CLASS) };
        let peer = if alen > 0 {
            match raw_to_socketaddr(&storage) {
                Some(a) => addrinfo::from_socketaddr(a, libc::SOCK_STREAM, libc::IPPROTO_TCP),
                None => RubyValue::Nil,
            }
        } else {
            RubyValue::Nil
        };
        Ok(RubyValue::Array(crate::array_new(vec![sock, peer])))
    }
    // `#accept_nonblock(exception: true)` -- accept only a connection already
    // pending, answering `[Socket, Addrinfo]` as `#accept` does.
    def "accept_nonblock"(recv, **opts) {
        let raises = crate::builtins::io::nonblock_raises(opts);
        let Some((nfd, storage, alen)) = super::accept_nonblock_fd(fd_of(recv)?)? else {
            return crate::builtins::io::would_block(false, raises, "accept(2)");
        };
        // SAFETY: `nfd` is a fresh, solely-owned descriptor.
        let sock = unsafe { socket_from_raw_fd(nfd, SOCKET_CLASS) };
        let peer = match raw_to_socketaddr(&storage).filter(|_| alen > 0) {
            Some(a) => addrinfo::from_socketaddr(a, libc::SOCK_STREAM, libc::IPPROTO_TCP),
            None => RubyValue::Nil,
        };
        Ok(RubyValue::Array(crate::array_new(vec![sock, peer])))
    }
    // `#connect_nonblock(sockaddr, exception: true)` -- start a connect and
    // report it in flight rather than waiting for the handshake.
    def "connect_nonblock"(recv, remote_sockaddr, **opts) {
        let raises = crate::builtins::io::nonblock_raises(opts);
        let fd = fd_of(recv)?;
        let sa = sockaddr_bytes(remote_sockaddr)?;
        crate::builtins::io::set_fd_nonblock(fd, true)?;
        // SAFETY: `sa` describes `sa.len()` initialized sockaddr bytes.
        let rc = unsafe {
            libc::connect(fd, sa.as_ptr() as *const libc::sockaddr, sa.len() as libc::socklen_t)
        };
        if rc == 0 {
            return Ok(RubyValue::Int(0));
        }
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINPROGRESS) {
            return Err(errno_error("connect(2)"));
        }
        if !raises {
            return Ok(RubyValue::Symbol(Symbol::intern("wait_writable")));
        }
        Err(raise_error(
            "IO::EINPROGRESSWaitWritable",
            "Operation now in progress - connect(2) would block".to_string(),
        ))
    }
    // `#recvfrom(maxlen, flags = 0)` -- `[mesg, sender_Addrinfo]`.
    def "recvfrom" cfunc (recv, arg1, arg2?) {
        let fd = fd_of(recv)?;
        let maxlen = (crate::builtins::convert::to_index(arg1)?).max(0) as usize;
        let flags = match arg2 {
            None | Some(RubyValue::Nil) => 0,
            Some(v) => crate::builtins::convert::to_index(v)? as libc::c_int,
        };
        do_recvfrom(fd, maxlen, flags)
    }
}

/// `Socket.getaddrinfo` over `libc::getaddrinfo`, honoring the family / socktype
/// / protocol hints from the trailing arguments.
fn getaddrinfo(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let host = match &args[0] {
        RubyValue::Nil => None,
        v => Some(v.to_display_string()),
    };
    let service = match &args[1] {
        RubyValue::Nil => None,
        RubyValue::Int(n) => Some(n.to_string()),
        v => Some(v.to_display_string()),
    };
    let hint_family = args.get(2).map(sock_int).transpose()?.unwrap_or(0);
    let hint_socktype = args.get(3).map(sock_int).transpose()?.unwrap_or(0);
    let hint_protocol = args.get(4).map(sock_int).transpose()?.unwrap_or(0);

    let c_host = host
        .as_ref()
        .map(|h| std::ffi::CString::new(h.as_str()).unwrap_or_default());
    let c_serv = service
        .as_ref()
        .map(|s| std::ffi::CString::new(s.as_str()).unwrap_or_default());

    // SAFETY: hints is a zeroed addrinfo we fill; result is populated by
    // getaddrinfo and freed via freeaddrinfo; each node is read while the list
    // is alive.
    unsafe {
        let mut hints: libc::addrinfo = std::mem::zeroed();
        hints.ai_family = hint_family;
        hints.ai_socktype = hint_socktype;
        hints.ai_protocol = hint_protocol;
        let mut res: *mut libc::addrinfo = std::ptr::null_mut();
        let rc = libc::getaddrinfo(
            c_host.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
            c_serv.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
            &hints,
            &mut res,
        );
        if rc != 0 {
            let msg = std::ffi::CStr::from_ptr(libc::gai_strerror(rc)).to_string_lossy();
            return Err(raise_error("SocketError", format!("getaddrinfo: {msg}")));
        }
        let mut out = Vec::new();
        let mut node = res;
        while !node.is_null() {
            let ai = &*node;
            if !ai.ai_addr.is_null() {
                let mut storage: libc::sockaddr_storage = std::mem::zeroed();
                let n = (ai.ai_addrlen as usize).min(std::mem::size_of::<libc::sockaddr_storage>());
                std::ptr::copy_nonoverlapping(
                    ai.ai_addr as *const u8,
                    &mut storage as *mut _ as *mut u8,
                    n,
                );
                if let Some(addr) = raw_to_socketaddr(&storage) {
                    let fam = if addr.is_ipv6() {
                        "AF_INET6"
                    } else {
                        "AF_INET"
                    };
                    out.push(RubyValue::Array(crate::array_new(vec![
                        str_val(fam),
                        RubyValue::Int(addr.port() as i64),
                        str_val(addr.ip().to_string()),
                        str_val(addr.ip().to_string()),
                        RubyValue::Int(ai.ai_family as i64),
                        RubyValue::Int(ai.ai_socktype as i64),
                        RubyValue::Int(ai.ai_protocol as i64),
                    ])));
                }
            }
            node = ai.ai_next;
        }
        libc::freeaddrinfo(res);
        Ok(RubyValue::Array(crate::array_new(out)))
    }
}

/// Parse a packed AF_INET/AF_INET6 `sockaddr` back to a `SocketAddr`.
fn unpack_sockaddr_in(bytes: &[u8]) -> Result<std::net::SocketAddr, Signal> {
    if bytes.len() < std::mem::size_of::<libc::sockaddr_in>() {
        return Err(arg_error!("too short sockaddr"));
    }
    // SAFETY: `bytes` is at least a sockaddr_in; we copy the min of its length
    // and sockaddr_storage into an aligned storage before reading a family.
    unsafe {
        let mut storage: libc::sockaddr_storage = std::mem::zeroed();
        let n = bytes
            .len()
            .min(std::mem::size_of::<libc::sockaddr_storage>());
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), &mut storage as *mut _ as *mut u8, n);
        raw_to_socketaddr(&storage).ok_or_else(|| arg_error!("not an AF_INET/AF_INET6 sockaddr"))
    }
}

/// Install `Socket`'s address-family / socket-type / protocol / option
/// constants at startup (`Socket::AF_INET6`, `Socket::SOCK_STREAM`, ...), the
/// runtime half of `zeo_abi::SOCKET_CONSTANT_NAMES`. Values come from the host
/// `libc` headers so they match the target platform exactly. Called from
/// `bootstrap::install_core_constants` when the `socket` feature is built.
pub fn seed_socket() {
    let cid = zeo_abi::SOCKET_CLASS.0;
    // Every name lands in BOTH tables, which is what `sock_define_const`
    // (ext/socket/constants.c) does -- `rb_define_const` on `mSockConst` and
    // again on `rb_cSocket`. Neither is derived from the other at run time:
    // `Socket::Constants` is included nowhere (`Socket.include?
    // (Socket::Constants)` is false), so a lookup through `Socket` never
    // reaches it.
    let consts = zeo_abi::SOCKET_CONSTANTS_MODULE.0;
    for &name in zeo_abi::SOCKET_CONSTANT_NAMES {
        let value = socket_const_value(name);
        crate::constants::const_set(cid, name, RubyValue::Int(value));
        crate::constants::const_set(consts, name, RubyValue::Int(value));
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

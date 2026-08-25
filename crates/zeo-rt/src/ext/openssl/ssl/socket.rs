//! `OpenSSL::SSL::SSLSocket` -- a client TLS session over an
//! already-connected socket, plus the buffered read/write surface CRuby
//! mixes in from `OpenSSL::Buffering` (see the module doc for what that
//! costs).

use super::context::{ctx_of, new_context};
use super::{FdStream, RSslSocket, SockState, reason, ssl_error};
use crate::builtins::convert;
use crate::dispatch::{RObj, RubyObject};
use crate::ext::openssl::{bin_str, str, str_bytes};
use crate::{ClassId, RubyValue, Signal};
use openssl::ssl::{Ssl, SslStream};
use parking_lot::Mutex;
use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zeo_macros::ruby_class;

impl RubyObject for RSslSocket {
    fn class_id(&self) -> ClassId {
        zeo_abi::OPENSSL_SSL_SOCKET_CLASS
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
        self.ivar_pairs().into_iter().map(|(_, v)| v).collect()
    }
    // `@context` then `@io` -- CRuby's own order, and they read through to the
    // native state rather than the map. Everything after is what `Buffering`
    // or a subclass stored.
    fn ivar_pairs(&self) -> Vec<(String, RubyValue)> {
        let st = self.st.lock();
        let mut pairs = vec![
            ("@context".to_string(), st.ctx.clone()),
            ("@io".to_string(), st.io.clone()),
        ];
        pairs.extend(
            self.ivars
                .lock()
                .iter()
                .map(|(k, v)| (format!("@{k}"), v.clone())),
        );
        pairs
    }
    fn ivar_get_named(&self, name: &str) -> Option<RubyValue> {
        match name {
            "io" => Some(self.st.lock().io.clone()),
            "context" => Some(self.st.lock().ctx.clone()),
            _ => self
                .ivars
                .lock()
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone()),
        }
    }
    fn ivar_set_named(&self, name: &str, v: RubyValue) -> bool {
        let mut ivars = self.ivars.lock();
        match ivars.iter_mut().find(|(k, _)| k == name) {
            Some(slot) => slot.1 = v,
            None => ivars.push((name.to_string(), v)),
        }
        true
    }
    fn ivar_remove_named(&self, name: &str) -> Option<RubyValue> {
        let mut ivars = self.ivars.lock();
        let at = ivars.iter().position(|(k, _)| k == name)?;
        Some(ivars.remove(at).1)
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        // A live TLS session cannot be duplicated (CRuby's cannot either):
        // the copy carries the configuration, not the session.
        let st = self.st.lock();
        Arc::new(RSslSocket {
            st: Mutex::new(SockState {
                io: st.io.clone(),
                fd: st.fd,
                ctx: st.ctx.clone(),
                hostname: st.hostname.clone(),
                sync_close: st.sync_close.clone(),
                stream: None,
            }),
            frozen: AtomicBool::new(false),
            ivars: Mutex::new(Vec::new()),
        })
    }
}

fn sock_of(recv: &RubyValue) -> &RSslSocket {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RSslSocket>()
            .expect("the SSLSocket table only dispatches on socket receivers"),
        _ => unreachable!("the SSLSocket table only dispatches on socket receivers"),
    }
}

/// The live session, or the "not started" error CRuby raises.
fn established(st: &mut SockState) -> Result<&mut SslStream<FdStream>, Signal> {
    st.stream
        .as_mut()
        .ok_or_else(|| ssl_error("SSL session is not started yet".to_string()))
}

fn do_connect(recv: &RubyValue) -> Result<RubyValue, Signal> {
    let mut st = sock_of(recv).st.lock();
    if st.stream.is_some() {
        return Ok(recv.clone());
    }
    let (ctx, verify_hostname) = {
        let cfg = ctx_of(&st.ctx).st.lock();
        (super::build_ctx(&cfg)?, cfg.verify_hostname)
    };
    let mut ssl = Ssl::new(&ctx).map_err(|e| ssl_error(reason(&e)))?;
    if let Some(host) = st.hostname.clone() {
        // SNI always; the verify param only when hostname checking is on,
        // since setting it makes libssl fail the handshake on a mismatch.
        ssl.set_hostname(&host).map_err(|e| ssl_error(reason(&e)))?;
        if verify_hostname {
            ssl.param_mut()
                .set_host(&host)
                .map_err(|e| ssl_error(reason(&e)))?;
        }
    }
    let mut stream = SslStream::new(ssl, FdStream(st.fd)).map_err(|e| ssl_error(reason(&e)))?;
    crate::gvl::without_gvl(|| stream.connect())
        .map_err(|e| ssl_error(format!("SSL_connect returned=1 errno=0 state=error: {e}")))?;
    st.stream = Some(stream);
    Ok(recv.clone())
}

/// One `SSL_read` -- at most one record's worth, empty at end of session.
fn read_some(recv: &RubyValue, len: usize) -> Result<Vec<u8>, Signal> {
    let mut st = sock_of(recv).st.lock();
    let stream = established(&mut st)?;
    let mut buf = vec![0u8; len];
    let got = crate::gvl::without_gvl(|| stream.read(&mut buf))
        .map_err(|e| ssl_error(format!("SSL_read: {e}")))?;
    buf.truncate(got);
    Ok(buf)
}

/// One `SSL_read` with the DESCRIPTOR non-blocking, so a record that has not
/// arrived answers `WouldBlock` instead of parking the thread.
///
/// The flag is restored on every path: the same fd is the blocking `sysread`'s
/// too, and leaving it set would turn that into a spurious EAGAIN.
fn read_some_nonblock(recv: &RubyValue, len: usize) -> Result<Option<Vec<u8>>, Signal> {
    let sock = sock_of(recv);
    let fd = sock.st.lock().fd;
    let mut st = sock.st.lock();
    let stream = established(&mut st)?;
    crate::builtins::io::set_fd_nonblock(fd, true)?;
    let mut buf = vec![0u8; len];
    let got = stream.read(&mut buf);
    let restore = crate::builtins::io::set_fd_nonblock(fd, false);
    let got = match got {
        Ok(n) => n,
        Err(e) if would_block(&e) => {
            restore?;
            return Ok(None);
        }
        Err(e) => {
            restore?;
            return Err(ssl_error(format!("SSL_read: {e}")));
        }
    };
    restore?;
    buf.truncate(got);
    Ok(Some(buf))
}

/// [`read_some_nonblock`]'s write twin.
fn write_some_nonblock(recv: &RubyValue, data: &[u8]) -> Result<Option<usize>, Signal> {
    let sock = sock_of(recv);
    let fd = sock.st.lock().fd;
    let mut st = sock.st.lock();
    let stream = established(&mut st)?;
    crate::builtins::io::set_fd_nonblock(fd, true)?;
    let put = stream.write(data);
    let restore = crate::builtins::io::set_fd_nonblock(fd, false);
    match put {
        Ok(n) => {
            restore?;
            Ok(Some(n))
        }
        Err(e) if would_block(&e) => {
            restore?;
            Ok(None)
        }
        Err(e) => {
            restore?;
            Err(ssl_error(format!("SSL_write: {e}")))
        }
    }
}

/// The `exception:` keyword, defaulting to true.
fn wants_exception(opts: Option<&RubyValue>) -> bool {
    let key = RubyValue::Symbol(crate::Symbol::intern("exception"));
    match opts {
        Some(RubyValue::Hash(h)) => match crate::value::collections::hash_get(h, &key) {
            RubyValue::Nil => true,
            v => v.truthy(),
        },
        _ => true,
    }
}

/// The shared answer for "nothing is ready" -- `IO`'s own, so a caller's
/// rescue of `IO::WaitReadable` works on a TLS socket too.
fn nothing_ready(exception: bool, what: &str) -> Result<RubyValue, Signal> {
    crate::builtins::io::would_block(what == "write", exception, what)
}

/// Whether the error is "nothing ready yet". openssl-rs reports a starved
/// read as `WouldBlock` on the inner stream; a TLS session that wants more
/// bytes to finish a record surfaces the same way.
fn would_block(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
    ) || e
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<std::io::Error>())
        .is_some_and(|inner| inner.kind() == std::io::ErrorKind::WouldBlock)
}

/// Relay a call to the underlying socket -- upstream's `SocketForwarder`,
/// which delegates the whole descriptor-level family to `to_io` because a
/// TLS session has nothing of its own to say about any of it.
fn write_all(recv: &RubyValue, data: &[u8]) -> Result<(), Signal> {
    let mut st = sock_of(recv).st.lock();
    let stream = established(&mut st)?;
    crate::gvl::without_gvl(|| stream.write_all(data))
        .map_err(|e| ssl_error(format!("SSL_write: {e}")))
}

ruby_class! {
    SSLSocket = zeo_abi::OPENSSL_SSL_SOCKET_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::OPENSSL_BUFFERING_MODULE;
    include zeo_abi::OPENSSL_SOCKET_FORWARDER_MODULE;

    // `SSLSocket.new(io, context = SSLContext.new)`.
    def self."new" arity -1 (_recv, arg1, arg2?) {
        let io = (*arg1).clone();
        let fd = crate::builtins::io::socket_raw_fd(&io)
            .ok_or_else(|| ssl_error("SSLSocket needs an open socket".to_string()))?;
        let ctx = match arg2 {
            None | Some(RubyValue::Nil) => new_context(),
            Some(c) => c.clone(),
        };
        Ok(RubyValue::Object(Arc::new(RSslSocket {
            st: Mutex::new(SockState {
                io,
                fd,
                ctx,
                hostname: None,
                sync_close: RubyValue::Nil,
                stream: None,
            }),
            frozen: AtomicBool::new(false),
            // The three slots `Buffering#initialize` seeds, so a fresh socket
            // reports CRuby's own `instance_variables`.
            ivars: Mutex::new(vec![
                ("eof".to_string(), RubyValue::Bool(false)),
                ("rbuffer".to_string(), bin_str(Vec::new())),
                ("sync".to_string(), RubyValue::Bool(true)),
            ]),
        })))
    }

    def "connect" (recv) {
        do_connect(recv)
    }
    // The descriptor is blocking, so the handshake always completes here
    // rather than answering :wait_readable (documented divergence).
    def "connect_nonblock" (recv, _arg?) {
        do_connect(recv)
    }

    def "hostname" (recv) {
        Ok(match &sock_of(recv).st.lock().hostname {
            Some(h) => str(h.clone()),
            None => RubyValue::Nil,
        })
    }
    def "hostname=" (recv, arg) {
        sock_of(recv).st.lock().hostname = match arg {
            RubyValue::Nil => None,
            v => Some(convert::to_rstr(v)?.lock().to_utf8_lossy().into_owned()),
        };
        Ok((*arg).clone())
    }
    def "sync_close" (recv) {
        Ok(sock_of(recv).st.lock().sync_close.clone())
    }
    def "sync_close=" (recv, arg) {
        sock_of(recv).st.lock().sync_close = (*arg).clone();
        Ok((*arg).clone())
    }
    def "io" | "to_io" (recv) {
        Ok(sock_of(recv).st.lock().io.clone())
    }

    def "context" (recv) {
        Ok(sock_of(recv).st.lock().ctx.clone())
    }

    // The three unbuffered primitives `OpenSSL::Buffering` is written
    // against. Everything above them -- read/gets/puts/each_line/... -- lives
    // in that module, where CRuby puts it.
    def "syswrite" arity 1 (recv, *args, &_block) {
        let mut total = 0i64;
        for arg in args {
            // A trailing kwargs hash is `exception: false`, not data.
            if matches!(arg, RubyValue::Hash(_)) {
                continue;
            }
            let data = str_bytes(arg)?;
            write_all(recv, &data)?;
            total += data.len() as i64;
        }
        Ok(RubyValue::Int(total))
    }
    // One record's worth, blocking until something arrives; EOFError at the
    // end of the session, as CRuby's sysread does.
    def "sysread" (recv, arg1?, _arg2?, _arg3?) {
        let len = match arg1 {
            None | Some(RubyValue::Nil) => 16384,
            Some(v) => convert::to_index(v)? as usize,
        };
        let chunk = read_some(recv, len)?;
        if chunk.is_empty() && len > 0 {
            return Err(crate::builtins::eof_error!("end of file reached"));
        }
        Ok(bin_str(chunk))
    }

    // The non-blocking primitives `OpenSSL::Buffering#read_nonblock` and
    // `#write_nonblock` are written against. `:wait_readable` / `:wait_writable`
    // when nothing is ready, or the matching `IO::Wait*` exception.
    def "sysread_nonblock" params "maxlen, buf = nil, exception: true" (recv, arg1, arg2?, **opts) {
        let len = match arg1 {
            RubyValue::Nil => 16384,
            v => convert::to_index(v)? as usize,
        };
        let exception = wants_exception(opts);
        let Some(chunk) = read_some_nonblock(recv, len)? else {
            return nothing_ready(exception, "read");
        };
        if chunk.is_empty() && len > 0 {
            return match exception {
                true => Err(crate::builtins::eof_error!("end of file reached")),
                false => Ok(RubyValue::Nil),
            };
        }
        match arg2 {
            Some(b @ RubyValue::Str(s)) => {
                s.lock().replace_bytes(chunk, crate::encoding::ASCII_8BIT);
                Ok(b.clone())
            }
            _ => Ok(bin_str(chunk)),
        }
    }
    def "syswrite_nonblock" params "s, exception: true" (recv, arg1, **opts) {
        let data = str_bytes(arg1)?;
        match write_some_nonblock(recv, &data)? {
            Some(n) => Ok(RubyValue::Int(n as i64)),
            None => nothing_ready(wants_exception(opts), "write"),
        }
    }

    // TLS session facts.
    def "ssl_version" (recv) {
        let mut st = sock_of(recv).st.lock();
        let stream = established(&mut st)?;
        Ok(str(stream.ssl().version_str().to_string()))
    }
    // CRuby answers [name, protocol version, secret bits, algorithm bits].
    def "cipher" (recv) {
        let mut st = sock_of(recv).st.lock();
        let Some(stream) = st.stream.as_mut() else {
            return Ok(RubyValue::Nil);
        };
        let Some(c) = stream.ssl().current_cipher() else {
            return Ok(RubyValue::Nil);
        };
        let bits = c.bits();
        Ok(RubyValue::Array(crate::array_new(vec![
            str(c.name().to_string()),
            str(c.version().to_string()),
            RubyValue::Int(bits.secret as i64),
            RubyValue::Int(bits.algorithm as i64),
        ])))
    }
    // Answers nil rather than raising when no session is up -- CRuby's
    // `ossl_ssl_get_peer_cert` reports "no certificate" for both the
    // never-connected and the anonymous-suite cases.
    def "peer_cert" (recv) {
        let mut st = sock_of(recv).st.lock();
        let Some(stream) = st.stream.as_mut() else {
            return Ok(RubyValue::Nil);
        };
        Ok(match stream.ssl().peer_certificate() {
            Some(cert) => super::cert::new_cert(&cert),
            None => RubyValue::Nil,
        })
    }
    def "verify_result" (recv) {
        let mut st = sock_of(recv).st.lock();
        let stream = established(&mut st)?;
        Ok(RubyValue::Int(i64::from(stream.ssl().verify_result().as_raw())))
    }
    // RFC 6125 hostname verification, run AFTER the handshake and
    // independent of it -- upstream's `post_connection_check` checks the
    // identity itself rather than trusting the connection's verify mode,
    // which is what makes it meaningful under VERIFY_NONE.
    def "post_connection_check" (recv, arg) {
        let hostname = convert::to_rstr(arg)?.lock().to_utf8_lossy().into_owned();
        let mut st = sock_of(recv).st.lock();
        let peer = st.stream.as_mut().and_then(|s| s.ssl().peer_certificate());
        let Some(cert) = peer else {
            return Err(ssl_error(
                "Peer verification enabled, but no certificate received.".to_string(),
            ));
        };
        if !super::cert::verify_certificate_identity(&cert, &hostname) {
            return Err(ssl_error(format!(
                "hostname \"{hostname}\" does not match the server certificate"
            )));
        }
        Ok(RubyValue::Bool(true))
    }
    def "state" (recv) {
        let st = sock_of(recv).st.lock();
        Ok(str(if st.stream.is_some() { "SSLOK ".to_string() } else { "PINIT".to_string() }))
    }

    // Shut the session down; the underlying IO closes only under
    // `sync_close`, CRuby's rule. `Buffering#close` flushes and calls this.
    def "sysclose" (recv) {
        let (io, sync_close) = {
            let mut st = sock_of(recv).st.lock();
            if let Some(stream) = st.stream.as_mut() {
                let _ = stream.shutdown();
            }
            st.stream = None;
            (st.io.clone(), st.sync_close.truthy())
        };
        if sync_close {
            crate::dispatch::send_value_in(0, &io, crate::Symbol::intern("close"), &[], None)?;
        }
        Ok(RubyValue::Nil)
    }
}

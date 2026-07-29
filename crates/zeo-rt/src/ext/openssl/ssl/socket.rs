//! `OpenSSL::SSL::SSLSocket` -- a client TLS session over an
//! already-connected socket, plus the buffered read/write surface CRuby
//! mixes in from `OpenSSL::Buffering` (see the module doc for what that
//! costs).

use super::context::{ctx_of, new_context};
use super::{FdStream, RSslSocket, SockState, reason, ssl_error};
use crate::builtins::{arity, convert};
use crate::dispatch::{RObj, RubyObject, raise_error};
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
        let st = self.st.lock();
        vec![st.io.clone(), st.ctx.clone()]
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
    let mut stream =
        SslStream::new(ssl, FdStream(st.fd)).map_err(|e| ssl_error(reason(&e)))?;
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

fn write_all(recv: &RubyValue, data: &[u8]) -> Result<(), Signal> {
    let mut st = sock_of(recv).st.lock();
    let stream = established(&mut st)?;
    crate::gvl::without_gvl(|| stream.write_all(data))
        .map_err(|e| ssl_error(format!("SSL_write: {e}")))
}

ruby_class! {
    SSLSocket = zeo_abi::OPENSSL_SSL_SOCKET_CLASS < zeo_abi::OBJECT_CLASS;

    // `SSLSocket.new(io, context = SSLContext.new)`.
    def self."new" arity -1 (_recv, args, _block) {
        arity!(args, 1..=2);
        let io = args[0].clone();
        let fd = crate::builtins::io::socket_raw_fd(&io)
            .ok_or_else(|| ssl_error("SSLSocket needs an open socket".to_string()))?;
        let ctx = match args.get(1) {
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
        })))
    }

    def "connect" (recv, args, _block) {
        arity!(args, 0);
        do_connect(recv)
    }
    // The descriptor is blocking, so the handshake always completes here
    // rather than answering :wait_readable (documented divergence).
    def "connect_nonblock" arity -1 (recv, args, _block) {
        arity!(args, 0..=1);
        do_connect(recv)
    }

    def "hostname" (recv, args, _block) {
        arity!(args, 0);
        Ok(match &sock_of(recv).st.lock().hostname {
            Some(h) => str(h.clone()),
            None => RubyValue::Nil,
        })
    }
    def "hostname=" (recv, args, _block) {
        arity!(args, 1);
        sock_of(recv).st.lock().hostname = match &args[0] {
            RubyValue::Nil => None,
            v => Some(convert::to_rstr(v)?.lock().to_utf8_lossy().into_owned()),
        };
        Ok(args[0].clone())
    }
    def "sync_close" (recv, args, _block) {
        arity!(args, 0);
        Ok(sock_of(recv).st.lock().sync_close.clone())
    }
    def "sync_close=" (recv, args, _block) {
        arity!(args, 1);
        sock_of(recv).st.lock().sync_close = args[0].clone();
        Ok(args[0].clone())
    }
    def "io" | "to_io" (recv, args, _block) {
        arity!(args, 0);
        Ok(sock_of(recv).st.lock().io.clone())
    }
    def "context" (recv, args, _block) {
        arity!(args, 0);
        Ok(sock_of(recv).st.lock().ctx.clone())
    }
    // Writes go straight to the session, so sync is always true.
    def "sync" (_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(true))
    }
    def "sync=" (_recv, args, _block) {
        arity!(args, 1);
        Ok(args[0].clone())
    }

    def "write" | "syswrite" | "write_nonblock" | "print" (recv, args, _block) {
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
    def "<<" (recv, args, _block) {
        arity!(args, 1);
        let data = str_bytes(&args[0])?;
        write_all(recv, &data)?;
        Ok(recv.clone())
    }
    def "puts" (recv, args, _block) {
        let mut out = Vec::new();
        if args.is_empty() {
            out.push(b'\n');
        }
        for arg in args {
            let mut data = str_bytes(arg)?;
            if !data.ends_with(b"\n") {
                data.push(b'\n');
            }
            out.extend_from_slice(&data);
        }
        write_all(recv, &out)?;
        Ok(RubyValue::Nil)
    }
    def "flush" (recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }

    // `read(len = nil)` -- to end of session without a length, exactly
    // `len` bytes (short at the end) with one, `nil` at the end for a
    // positive length.
    def "read" arity -1 (recv, args, _block) {
        arity!(args, 0..=2);
        let len = match args.first() {
            None | Some(RubyValue::Nil) => None,
            Some(v) => Some(convert::to_index(v)? as usize),
        };
        let mut out = Vec::new();
        match len {
            None => loop {
                // One SSL_read answers at most one record, so drain.
                let chunk = read_some(recv, 16384)?;
                if chunk.is_empty() {
                    break;
                }
                out.extend_from_slice(&chunk);
            },
            Some(want) => {
                while out.len() < want {
                    let chunk = read_some(recv, want - out.len())?;
                    if chunk.is_empty() {
                        break;
                    }
                    out.extend_from_slice(&chunk);
                }
                if out.is_empty() && want > 0 {
                    return Ok(RubyValue::Nil);
                }
            }
        }
        Ok(bin_str(out))
    }
    // One record's worth, blocking until something arrives; EOFError at the
    // end of the session, as CRuby's sysread does.
    def "sysread" | "readpartial" | "read_nonblock" arity -1 (recv, args, _block) {
        arity!(args, 0..=3);
        let len = match args.first() {
            None | Some(RubyValue::Nil) => 16384,
            Some(v) => convert::to_index(v)? as usize,
        };
        let chunk = read_some(recv, len)?;
        if chunk.is_empty() && len > 0 {
            return Err(raise_error("EOFError", "end of file reached".to_string()));
        }
        Ok(bin_str(chunk))
    }
    // One line, up to and including the separator.
    def "gets" arity -1 (recv, args, _block) {
        arity!(args, 0..=2);
        let sep = match args.first() {
            None => b'\n',
            Some(RubyValue::Nil) => b'\n',
            Some(v) => *str_bytes(v)?.last().unwrap_or(&b'\n'),
        };
        let mut out = Vec::new();
        loop {
            let byte = read_some(recv, 1)?;
            if byte.is_empty() {
                break;
            }
            out.push(byte[0]);
            if byte[0] == sep {
                break;
            }
        }
        if out.is_empty() {
            return Ok(RubyValue::Nil);
        }
        Ok(bin_str(out))
    }
    // The session is closed, not the peer's stream end -- see the module
    // doc; a caller that needs true EOF reads until `read` answers empty.
    def "eof?" | "eof" (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(sock_of(recv).st.lock().stream.is_none()))
    }

    // TLS session facts.
    def "ssl_version" (recv, args, _block) {
        arity!(args, 0);
        let mut st = sock_of(recv).st.lock();
        let stream = established(&mut st)?;
        Ok(str(stream.ssl().version_str().to_string()))
    }
    // CRuby answers [name, protocol version, secret bits, algorithm bits].
    def "cipher" (recv, args, _block) {
        arity!(args, 0);
        let mut st = sock_of(recv).st.lock();
        let stream = established(&mut st)?;
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
    def "peer_cert" (recv, args, _block) {
        arity!(args, 0);
        let mut st = sock_of(recv).st.lock();
        let stream = established(&mut st)?;
        Ok(match stream.ssl().peer_certificate() {
            Some(cert) => super::cert::new_cert(&cert),
            None => RubyValue::Nil,
        })
    }
    def "verify_result" (recv, args, _block) {
        arity!(args, 0);
        let mut st = sock_of(recv).st.lock();
        let stream = established(&mut st)?;
        Ok(RubyValue::Int(i64::from(stream.ssl().verify_result().as_raw())))
    }
    // The check CRuby runs after the handshake. Verification itself already
    // happened during it (libssl's X509 verify param carries the hostname),
    // so what is left is the no-certificate case.
    def "post_connection_check" (recv, args, _block) {
        arity!(args, 1);
        let mut st = sock_of(recv).st.lock();
        let stream = established(&mut st)?;
        if stream.ssl().peer_certificate().is_none() {
            return Err(ssl_error(
                "Peer verification enabled, but no certificate received.".to_string(),
            ));
        }
        Ok(RubyValue::Bool(true))
    }
    def "state" (recv, args, _block) {
        arity!(args, 0);
        let st = sock_of(recv).st.lock();
        Ok(str(if st.stream.is_some() { "SSLOK ".to_string() } else { "PINIT".to_string() }))
    }

    // Shut the session down; the underlying IO closes only under
    // `sync_close`, CRuby's rule.
    def "close" | "sysclose" (recv, args, _block) {
        arity!(args, 0);
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

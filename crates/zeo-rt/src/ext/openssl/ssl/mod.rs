//! `OpenSSL::SSL` -- CLIENT-side TLS over the vendored libssl:
//!
//! ```text
//! OpenSSL::SSL                 the verify/version/option constants (here)
//!  ├ ::SSLContext              configuration                  (context.rs)
//!  ├ ::SSLSocket               a session over a connected IO    (socket.rs)
//!  └ ::SocketForwarder    what the socket underneath answers
//!                                                  (socket_forwarder.rs)
//! OpenSSL::Buffering           the buffered IO surface       (buffering.rs)
//! OpenSSL::X509                the verify-result codes           (x509.rs)
//!  ├ ::Store                   the CA trust store               (store.rs)
//!  └ ::Certificate             a peer certificate                (cert.rs)
//! ```
//!
//! Server-side TLS (`SSLServer`, `accept`) is declined -- see
//! docs/COMPATIBILITY.md. The shared state and helpers live here; each
//! class is its own file because the DSL allows one `ruby_class!` per
//! module.
//!
//! An `SSLSocket` borrows its underlying socket's DESCRIPTOR ([`FdStream`])
//! rather than taking the `IO` value's ownership, so `#io`/`#to_io` keep
//! answering the very object the caller passed (net/http calls
//! `to_io.wait_readable`) and closing follows Ruby's `sync_close` rule
//! instead of Rust's drop order.
//!
//! `SSLSocket` supplies only the three unbuffered primitives -- `sysread`,
//! `syswrite`, `sysclose` -- and mixes in `Buffering` for everything above
//! them, which is how CRuby divides the same work. One documented
//! consequence remains: `read_nonblock`/`write_nonblock` read and write a
//! BLOCKING descriptor (they never answer `:wait_readable`), so a caller's
//! socket timeout does not interrupt them.

pub(crate) mod buffering;
pub(crate) mod cert;
pub(crate) mod context;
pub(crate) mod socket;
pub(crate) mod socket_forwarder;
pub(crate) mod store;
pub(crate) mod x509;

use crate::dispatch::raise_error;
use crate::{RubyValue, Signal};
use openssl::ssl::{SslContext, SslMethod, SslStream, SslVerifyMode, SslVersion};
use parking_lot::Mutex;
use std::io::{Read, Write};
use std::os::fd::RawFd;
use std::sync::atomic::AtomicBool;
use zeo_macros::ruby_module;

pub(crate) fn ssl_error(msg: String) -> Signal {
    raise_error("OpenSSL::SSL::SSLError", msg)
}

pub(crate) fn reason(e: &openssl::error::ErrorStack) -> String {
    e.errors()
        .first()
        .and_then(|err| err.reason().map(str::to_string))
        .unwrap_or_else(|| format!("{e}"))
}

/// A borrowed descriptor as a `Read`/`Write` stream. Non-owning: the Ruby
/// `IO` value the socket was built from stays the owner and closes it.
pub(crate) struct FdStream(pub(crate) RawFd);

impl Read for FdStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = crate::gvl::without_gvl(|| unsafe {
            libc::read(self.0, buf.as_mut_ptr().cast(), buf.len())
        });
        if n < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(n as usize)
    }
}

impl Write for FdStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = crate::gvl::without_gvl(|| unsafe {
            libc::write(self.0, buf.as_ptr().cast(), buf.len())
        });
        if n < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(n as usize)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) struct CtxState {
    pub(crate) verify_mode: i64,
    pub(crate) verify_hostname: bool,
    pub(crate) ca_file: Option<String>,
    pub(crate) ca_path: Option<String>,
    pub(crate) min_version: Option<i64>,
    pub(crate) max_version: Option<i64>,
    /// Set by `set_params`, whose defaults install the system trust store.
    pub(crate) default_paths: bool,
    /// `SSL_CTX_set_session_cache_mode`'s value. libssl owns the caching
    /// itself; this is carried so a reader sees what was written, starting
    /// at `SSL_SESS_CACHE_SERVER` as CRuby's does.
    pub(crate) session_cache_mode: i64,
}

impl CtxState {
    /// A fresh context's state: no verification until `set_params` or an
    /// explicit `verify_mode=`, which is CRuby's own default.
    pub(crate) fn new() -> CtxState {
        CtxState {
            verify_mode: 0,
            verify_hostname: false,
            ca_file: None,
            ca_path: None,
            min_version: None,
            max_version: None,
            default_paths: false,
            session_cache_mode: 2,
        }
    }
}

pub(crate) struct RSslContext {
    pub(crate) st: Mutex<CtxState>,
    pub(crate) frozen: AtomicBool,
}

pub(crate) struct SockState {
    /// The Ruby `IO` this session runs over -- kept so `#io`/`#to_io`
    /// answer the caller's own object and `sync_close` can close it.
    pub(crate) io: RubyValue,
    pub(crate) fd: RawFd,
    pub(crate) ctx: RubyValue,
    pub(crate) hostname: Option<String>,
    /// The value `sync_close=` stored, verbatim -- CRuby's reader answers
    /// `nil` on a fresh socket, not `false`. Truthiness decides whether
    /// `#close` closes the underlying IO.
    pub(crate) sync_close: RubyValue,
    pub(crate) stream: Option<SslStream<FdStream>>,
}

pub(crate) struct RSslSocket {
    pub(crate) st: Mutex<SockState>,
    pub(crate) frozen: AtomicBool,
    /// Ruby-visible ivars, by name without the `@`. A hand-written native
    /// object has none by default, which left `OpenSSL::Buffering` no place
    /// to keep the pushback `ungetc` needs and no place for a subclass to
    /// keep its own state.
    pub(crate) ivars: Mutex<Vec<(String, RubyValue)>>,
}

fn version_of(raw: i64) -> Option<SslVersion> {
    match raw {
        0x0301 => Some(SslVersion::TLS1),
        0x0302 => Some(SslVersion::TLS1_1),
        0x0303 => Some(SslVersion::TLS1_2),
        0x0304 => Some(SslVersion::TLS1_3),
        _ => None,
    }
}

/// The system CA bundles to fall back on, in upstream `openssl.rb`'s own
/// order. zeo links a VENDORED OpenSSL, whose compiled-in default cert path
/// belongs to the build machine and generally does not exist on the host --
/// so, exactly as the gem's Ruby half does, a real bundle is located here
/// when the environment names none.
const SYSTEM_CERT_BUNDLES: &[&str] = &[
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/ssl/ca-bundle.pem",
    "/etc/ssl/cert.pem",
];

/// The first existing bundle, unless `SSL_CERT_FILE`/`SSL_CERT_DIR` already
/// point libssl somewhere (its own default-paths lookup reads those).
fn fallback_bundle() -> Option<&'static str> {
    let named = |k: &str| std::env::var(k).is_ok_and(|v| !v.is_empty());
    if named("SSL_CERT_FILE") || named("SSL_CERT_DIR") {
        return None;
    }
    SYSTEM_CERT_BUNDLES
        .iter()
        .copied()
        .find(|p| std::path::Path::new(p).exists())
}

/// Build the libssl context this configuration describes.
pub(crate) fn build_ctx(st: &CtxState) -> Result<SslContext, Signal> {
    let err = |e: openssl::error::ErrorStack| ssl_error(reason(&e));
    let mut b = SslContext::builder(SslMethod::tls_client()).map_err(err)?;
    b.set_verify(if st.verify_mode == 0 {
        SslVerifyMode::NONE
    } else {
        SslVerifyMode::PEER
    });
    if let Some(f) = &st.ca_file {
        b.set_ca_file(f).map_err(err)?;
    }
    // With no explicit bundle, trust the system store -- what `set_params`
    // means by its default `cert_store`.
    if st.ca_file.is_none() || st.default_paths {
        b.set_default_verify_paths().map_err(err)?;
        if st.ca_file.is_none() {
            if let Some(bundle) = fallback_bundle() {
                b.set_ca_file(bundle).map_err(err)?;
            }
        }
    }
    if let Some(v) = st.min_version.and_then(version_of) {
        b.set_min_proto_version(Some(v)).map_err(err)?;
    }
    if let Some(v) = st.max_version.and_then(version_of) {
        b.set_max_proto_version(Some(v)).map_err(err)?;
    }
    Ok(b.build())
}

ruby_module! {
    SSL = zeo_abi::OPENSSL_SSL_MODULE;

    // Peer-verification modes.
    const VERIFY_NONE = RubyValue::Int(0);
    const VERIFY_PEER = RubyValue::Int(1);
    const VERIFY_FAIL_IF_NO_PEER_CERT = RubyValue::Int(2);
    const VERIFY_CLIENT_ONCE = RubyValue::Int(4);

    // Protocol versions, as `min_version=`/`max_version=` take them.
    const TLS1_VERSION = RubyValue::Int(0x0301);
    const TLS1_1_VERSION = RubyValue::Int(0x0302);
    const TLS1_2_VERSION = RubyValue::Int(0x0303);
    const TLS1_3_VERSION = RubyValue::Int(0x0304);

    // The SSL_OP_ bits callers OR into `options`; carried by name so such
    // code compiles, and left to libssl's own defaults.
    const OP_ALL = RubyValue::Int(0x8000_0054);
    const OP_NO_COMPRESSION = RubyValue::Int(0x0002_0000);
    const OP_NO_SSLv2 = RubyValue::Int(0x0100_0000);
    const OP_NO_SSLv3 = RubyValue::Int(0x0200_0000);
    const OP_NO_TLSv1 = RubyValue::Int(0x0400_0000);
    const OP_NO_TLSv1_1 = RubyValue::Int(0x1000_0000);
    const OP_NO_TLSv1_2 = RubyValue::Int(0x0800_0000);
}

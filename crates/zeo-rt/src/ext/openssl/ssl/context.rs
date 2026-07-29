//! `OpenSSL::SSL::SSLContext` -- the client-side TLS configuration a
//! session is built from. The knobs are stored here and turned into a real
//! libssl context ([`super::build_ctx`]) at connect time, so a caller can
//! keep adjusting a context right up to the handshake, as CRuby's does.

use super::{CtxState, RSslContext};
use crate::builtins::{arity, convert};
use crate::dispatch::{RObj, RubyObject};
use crate::ext::openssl::str;
use crate::{ClassId, RubyValue};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zeo_macros::ruby_class;

impl RubyObject for RSslContext {
    fn class_id(&self) -> ClassId {
        zeo_abi::OPENSSL_SSL_CONTEXT_CLASS
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
        let st = self.st.lock();
        let d = RSslContext {
            st: Mutex::new(CtxState {
                verify_mode: st.verify_mode,
                verify_hostname: st.verify_hostname,
                ca_file: st.ca_file.clone(),
                ca_path: st.ca_path.clone(),
                min_version: st.min_version,
                max_version: st.max_version,
                default_paths: st.default_paths,
                session_cache_mode: st.session_cache_mode,
            }),
            frozen: AtomicBool::new(false),
        };
        if copy_frozen {
            d.set_frozen();
        }
        Arc::new(d)
    }
}

pub(crate) fn new_context() -> RubyValue {
    RubyValue::Object(Arc::new(RSslContext {
        st: Mutex::new(CtxState::new()),
        frozen: AtomicBool::new(false),
    }))
}

pub(crate) fn ctx_of(recv: &RubyValue) -> &RSslContext {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RSslContext>()
            .expect("the SSLContext table only dispatches on context receivers"),
        _ => unreachable!("the SSLContext table only dispatches on context receivers"),
    }
}

/// A String-or-nil configuration path.
fn path_arg(v: &RubyValue) -> Result<Option<String>, crate::Signal> {
    Ok(match v {
        RubyValue::Nil => None,
        other => Some(convert::to_rstr(other)?.lock().to_utf8_lossy().into_owned()),
    })
}

ruby_class! {
    SSLContext = zeo_abi::OPENSSL_SSL_CONTEXT_CLASS < zeo_abi::OBJECT_CLASS;

    // `SSL_SESS_CACHE_*`, as `session_cache_mode=` takes them. libssl owns
    // the caching itself; these exist because callers OR them together
    // (net/http asks for CLIENT | NO_INTERNAL_STORE before connecting).
    const SESSION_CACHE_OFF = RubyValue::Int(0);
    const SESSION_CACHE_CLIENT = RubyValue::Int(1);
    const SESSION_CACHE_SERVER = RubyValue::Int(2);
    const SESSION_CACHE_BOTH = RubyValue::Int(3);
    const SESSION_CACHE_NO_AUTO_CLEAR = RubyValue::Int(128);
    const SESSION_CACHE_NO_INTERNAL_LOOKUP = RubyValue::Int(256);
    const SESSION_CACHE_NO_INTERNAL_STORE = RubyValue::Int(512);
    const SESSION_CACHE_NO_INTERNAL = RubyValue::Int(768);

    // The optional argument is CRuby's protocol-version shorthand
    // (`SSLContext.new(:TLSv1_2)`), accepted and left to min/max_version.
    def self."new" arity -1 (_recv, args, _block) {
        arity!(args, 0..=1);
        Ok(new_context())
    }

    // `set_params(params = {})` -- CRuby's DEFAULT_PARAMS merge: verify the
    // peer against the system trust store and check the hostname, then
    // apply the caller's overrides.
    def "set_params" arity -1 (recv, args, _block) {
        arity!(args, 0..=1);
        let params = args.first().cloned();
        let mut st = ctx_of(recv).st.lock();
        st.verify_mode = 1;
        st.verify_hostname = true;
        st.default_paths = true;
        if let Some(RubyValue::Hash(h)) = &params {
            for (k, v) in crate::hash_pairs(h) {
                let RubyValue::Symbol(sym) = k else { continue };
                match sym.name_str() {
                    "verify_mode" => st.verify_mode = convert::to_index(&v)?,
                    "verify_hostname" => st.verify_hostname = v.truthy(),
                    "ca_file" => st.ca_file = path_arg(&v)?,
                    "ca_path" => st.ca_path = path_arg(&v)?,
                    "min_version" => st.min_version = Some(convert::to_index(&v)?),
                    "max_version" => st.max_version = Some(convert::to_index(&v)?),
                    // Ciphers, timeouts and the callback knobs are accepted
                    // and left to libssl's defaults.
                    _ => {}
                }
            }
        }
        Ok(params.unwrap_or(RubyValue::Nil))
    }

    def "verify_mode" (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(ctx_of(recv).st.lock().verify_mode))
    }
    def "verify_mode=" (recv, args, _block) {
        arity!(args, 1);
        ctx_of(recv).st.lock().verify_mode = convert::to_index(&args[0])?;
        Ok(args[0].clone())
    }
    def "verify_hostname" (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(ctx_of(recv).st.lock().verify_hostname))
    }
    def "verify_hostname=" (recv, args, _block) {
        arity!(args, 1);
        ctx_of(recv).st.lock().verify_hostname = args[0].truthy();
        Ok(args[0].clone())
    }
    def "ca_file" (recv, args, _block) {
        arity!(args, 0);
        Ok(match &ctx_of(recv).st.lock().ca_file {
            Some(f) => str(f.clone()),
            None => RubyValue::Nil,
        })
    }
    def "ca_file=" (recv, args, _block) {
        arity!(args, 1);
        ctx_of(recv).st.lock().ca_file = path_arg(&args[0])?;
        Ok(args[0].clone())
    }
    def "ca_path" (recv, args, _block) {
        arity!(args, 0);
        Ok(match &ctx_of(recv).st.lock().ca_path {
            Some(f) => str(f.clone()),
            None => RubyValue::Nil,
        })
    }
    def "ca_path=" (recv, args, _block) {
        arity!(args, 1);
        ctx_of(recv).st.lock().ca_path = path_arg(&args[0])?;
        Ok(args[0].clone())
    }
    def "min_version=" (recv, args, _block) {
        arity!(args, 1);
        ctx_of(recv).st.lock().min_version = match &args[0] {
            RubyValue::Nil => None,
            v => Some(convert::to_index(v)?),
        };
        Ok(args[0].clone())
    }
    def "max_version=" (recv, args, _block) {
        arity!(args, 1);
        ctx_of(recv).st.lock().max_version = match &args[0] {
            RubyValue::Nil => None,
            v => Some(convert::to_index(v)?),
        };
        Ok(args[0].clone())
    }
    // libssl owns session caching; the mode is carried so a reader sees
    // what was written (net/http sets it before connecting).
    def "session_cache_mode" (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(ctx_of(recv).st.lock().session_cache_mode))
    }
    def "session_cache_mode=" (recv, args, _block) {
        arity!(args, 1);
        ctx_of(recv).st.lock().session_cache_mode = convert::to_index(&args[0])?;
        Ok(args[0].clone())
    }
    // The trust store is libssl's own; the accessor answers a Store value
    // so `ctx.cert_store.set_default_paths` style code runs.
    def "cert_store" (_recv, args, _block) {
        arity!(args, 0);
        Ok(super::store::new_store())
    }
    def "cert_store=" (_recv, args, _block) {
        arity!(args, 1);
        Ok(args[0].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::registered_table;

    fn im(name: &str) -> crate::builtins::BuiltinMethodFn {
        let table = registered_table(zeo_abi::OPENSSL_SSL_CONTEXT_CLASS)
            .and_then(|t| t.instance.as_ref())
            .expect("SSLContext registers an instance table");
        (table.lookup)(name).expect("instance method exists")
    }

    #[test]
    fn a_fresh_context_verifies_nothing_until_set_params() {
        let ctx = new_context();
        assert!(matches!(
            im("verify_mode")(&ctx, &[], None).unwrap(),
            RubyValue::Int(0)
        ));
        assert!(matches!(
            im("verify_hostname")(&ctx, &[], None).unwrap(),
            RubyValue::Bool(false)
        ));
        assert!(matches!(im("ca_file")(&ctx, &[], None).unwrap(), RubyValue::Nil));
        // set_params installs CRuby's defaults: VERIFY_PEER + hostname.
        im("set_params")(&ctx, &[], None).unwrap();
        assert!(matches!(
            im("verify_mode")(&ctx, &[], None).unwrap(),
            RubyValue::Int(1)
        ));
        assert!(matches!(
            im("verify_hostname")(&ctx, &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
    }

    #[test]
    fn set_params_overrides_reach_the_state() {
        let ctx = new_context();
        let params = RubyValue::Hash(crate::hash_new(vec![(
            RubyValue::Symbol(crate::Symbol::intern("verify_mode")),
            RubyValue::Int(0),
        )]));
        im("set_params")(&ctx, &[params], None).unwrap();
        assert!(matches!(
            im("verify_mode")(&ctx, &[], None).unwrap(),
            RubyValue::Int(0)
        ));
        // ...and the hostname default survives an unrelated override.
        assert!(matches!(
            im("verify_hostname")(&ctx, &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
    }

    #[test]
    fn a_configured_context_builds_a_real_libssl_context() {
        let ctx = new_context();
        im("set_params")(&ctx, &[], None).unwrap();
        im("min_version=")(&ctx, &[RubyValue::Int(0x0303)], None).unwrap();
        assert!(super::super::build_ctx(&ctx_of(&ctx).st.lock()).is_ok());
    }
}

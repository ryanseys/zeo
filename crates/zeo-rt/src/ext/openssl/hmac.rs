//! `OpenSSL::HMAC` -- the streaming keyed MAC. Same buffer-and-rehash model
//! as `digest.rs` (the key and algorithm are fixed at construction; each
//! `digest`/`hexdigest` MACs the accumulated message fresh), which is what
//! makes the non-destructive peek and `reset` trivial.
//!
//! CRuby implements `==`, `base64digest` and the one-shot class forms in the
//! gem's RUBY half (`openssl/hmac.rb`); they are native here so the gem's
//! Ruby half stays exception-only, but the semantics are that file's:
//! `==` is a fixed-length constant-time compare of the two MACs.

use super::{base64, bin_str, hex, md_from_value, str, str_bytes};
use crate::builtins::arity;
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::{ClassId, RubyValue, Signal};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zeo_macros::ruby_class;

pub(crate) struct RHmac {
    key: Vec<u8>,
    /// The canonical algorithm name (`"SHA256"`).
    algo: String,
    /// The accumulated message; MAC'd fresh on each `digest`/`hexdigest`.
    buf: Mutex<Vec<u8>>,
    frozen: AtomicBool,
}

impl RHmac {
    /// The MAC of `data` under this instance's key and algorithm.
    fn mac(&self, data: &[u8]) -> Result<Vec<u8>, Signal> {
        let (md, _) = super::md_by_name(&self.algo)?;
        one_shot(md, &self.key, data)
    }
}

impl RubyObject for RHmac {
    fn class_id(&self) -> ClassId {
        zeo_abi::OPENSSL_HMAC_CLASS
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
        let h = RHmac {
            key: self.key.clone(),
            algo: self.algo.clone(),
            buf: Mutex::new(self.buf.lock().clone()),
            frozen: AtomicBool::new(false),
        };
        if copy_frozen {
            h.set_frozen();
        }
        Arc::new(h)
    }
}

fn one_shot(md: openssl::hash::MessageDigest, key: &[u8], data: &[u8]) -> Result<Vec<u8>, Signal> {
    let err = |e: openssl::error::ErrorStack| raise_error("OpenSSL::HMACError", format!("{e}"));
    let pkey = openssl::pkey::PKey::hmac(key).map_err(err)?;
    let mut signer = openssl::sign::Signer::new(md, &pkey).map_err(err)?;
    signer.update(data).map_err(err)?;
    signer.sign_to_vec().map_err(err)
}

fn hmac_of(recv: &RubyValue) -> &RHmac {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RHmac>()
            .expect("the OpenSSL::HMAC table only dispatches on HMAC receivers"),
        _ => unreachable!("the OpenSSL::HMAC table only dispatches on HMAC receivers"),
    }
}

/// The one-shot class-form body (`HMAC.digest(md, key, data)` family --
/// algorithm FIRST there, where `new` takes the key first).
fn class_mac(args: &[RubyValue]) -> Result<Vec<u8>, Signal> {
    let (md, _) = md_from_value(&args[0])?;
    one_shot(md, &str_bytes(&args[1])?, &str_bytes(&args[2])?)
}

ruby_class! {
    HMAC = zeo_abi::OPENSSL_HMAC_CLASS < zeo_abi::OBJECT_CLASS;

    // `OpenSSL::HMAC.new(key, digest)` -- digest by name or instance.
    def self."new" arity 2 (_recv, args, _block) {
        arity!(args, 2);
        let key = str_bytes(&args[0])?;
        let (md, canonical) = md_from_value(&args[1])?;
        // Probe once so an unusable algorithm surfaces at construction.
        one_shot(md, &key, b"")?;
        Ok(RubyValue::Object(Arc::new(RHmac {
            key,
            algo: canonical,
            buf: Mutex::new(Vec::new()),
            frozen: AtomicBool::new(false),
        })))
    }
    def self."digest" arity 3 (_recv, args, _block) {
        arity!(args, 3);
        Ok(bin_str(class_mac(args)?))
    }
    def self."hexdigest" arity 3 (_recv, args, _block) {
        arity!(args, 3);
        Ok(str(hex(&class_mac(args)?)))
    }
    def self."base64digest" arity 3 (_recv, args, _block) {
        arity!(args, 3);
        Ok(str(base64(&class_mac(args)?)))
    }

    def "update" | "<<" (recv, args, _block) {
        arity!(args, 1);
        hmac_of(recv).buf.lock().extend_from_slice(&str_bytes(&args[0])?);
        Ok(recv.clone())
    }
    def "digest" (recv, args, _block) {
        arity!(args, 0);
        let h = hmac_of(recv);
        let out = h.mac(&h.buf.lock())?;
        Ok(bin_str(out))
    }
    def "hexdigest" | "to_s" | "inspect" (recv, args, _block) {
        arity!(args, 0);
        let h = hmac_of(recv);
        let out = h.mac(&h.buf.lock())?;
        Ok(str(hex(&out)))
    }
    def "base64digest" (recv, args, _block) {
        arity!(args, 0);
        let h = hmac_of(recv);
        let out = h.mac(&h.buf.lock())?;
        Ok(str(base64(&out)))
    }
    def "reset" (recv, args, _block) {
        arity!(args, 0);
        hmac_of(recv).buf.lock().clear();
        Ok(recv.clone())
    }
    // Constant-time MAC equality; false for anything that is not an HMAC.
    def "==" (recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Object(o) = &args[0] else {
            return Ok(RubyValue::Bool(false));
        };
        let Some(other) = o.as_any().downcast_ref::<RHmac>() else {
            return Ok(RubyValue::Bool(false));
        };
        let mine = hmac_of(recv);
        let a = mine.mac(&mine.buf.lock())?;
        let b = other.mac(&other.buf.lock())?;
        Ok(RubyValue::Bool(a.len() == b.len() && {
            let mut diff = 0u8;
            for (x, y) in a.iter().zip(b.iter()) {
                diff |= x ^ y;
            }
            diff == 0
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::registered_table;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(crate::string_new(text.to_string()))
    }
    fn im(name: &str) -> crate::builtins::BuiltinMethodFn {
        let table = registered_table(zeo_abi::OPENSSL_HMAC_CLASS)
            .and_then(|t| t.instance.as_ref())
            .expect("HMAC registers an instance table");
        (table.lookup)(name).expect("instance method exists")
    }
    fn cm(name: &str) -> crate::builtins::BuiltinMethodFn {
        let table = registered_table(zeo_abi::OPENSSL_HMAC_CLASS)
            .and_then(|t| t.class.as_ref())
            .expect("HMAC registers a class table");
        (table.lookup)(name).expect("class method exists")
    }
    fn t(v: Result<RubyValue, Signal>) -> String {
        match v.unwrap() {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn one_shot_matches_ruby() {
        // ruby 4.0.6: OpenSSL::HMAC.hexdigest("SHA256", "key", "The quick brown fox").
        assert_eq!(
            t(cm("hexdigest")(
                &RubyValue::Nil,
                &[s("SHA256"), s("key"), s("The quick brown fox")],
                None
            )),
            "203d1e5cedd2d18f8c5a3beff0bd9c1ebcb97097dfcb288c46b00c9227fde2c0"
        );
        assert_eq!(
            t(cm("base64digest")(
                &RubyValue::Nil,
                &[s("SHA256"), s("key"), s("data")],
                None
            )),
            "UDH+PZicbRU3oBP6bnOdojRj/a7DtwE32Cjjas4iG9A="
        );
    }

    #[test]
    fn streaming_matches_one_shot_and_resets() {
        let h = cm("new")(&RubyValue::Nil, &[s("key"), s("SHA256")], None).unwrap();
        im("update")(&h, &[s("The quick ")], None).unwrap();
        im("update")(&h, &[s("brown fox")], None).unwrap();
        assert_eq!(
            t(im("hexdigest")(&h, &[], None)),
            "203d1e5cedd2d18f8c5a3beff0bd9c1ebcb97097dfcb288c46b00c9227fde2c0"
        );
        // Peek does not consume; == compares MACs in constant time.
        let g = cm("new")(&RubyValue::Nil, &[s("key"), s("sha256")], None).unwrap();
        im("update")(&g, &[s("The quick brown fox")], None).unwrap();
        assert!(matches!(
            im("==")(&h, &[g], None).unwrap(),
            RubyValue::Bool(true)
        ));
        im("reset")(&h, &[], None).unwrap();
        im("update")(&h, &[s("x")], None).unwrap();
        assert_eq!(
            t(im("hexdigest")(&h, &[], None)),
            t(cm("hexdigest")(&RubyValue::Nil, &[s("SHA256"), s("key"), s("x")], None))
        );
    }

}

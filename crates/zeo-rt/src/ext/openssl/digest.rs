//! `OpenSSL::Digest` -- EVP message digests. The base class takes the
//! algorithm by name (`OpenSSL::Digest.new("SHA256")`); the fixed-algorithm
//! subclasses (`OpenSSL::Digest::SHA256`) live in `algo_class.rs` and share
//! this instance table through the MRO.
//!
//! CRuby parents this class under the `digest` framework's `Digest::Class`;
//! zeo's digest classes are native tables with no shared Ruby superclass, so
//! it sits under `Object` (documented in docs/COMPATIBILITY.md). The
//! instance surface implemented here IS the `Digest::Instance` contract the
//! framework would supply: `hexdigest(data)` hashes accumulated+data then
//! resets, `hexdigest` alone peeks without resetting.

use super::{base64, bin_str, digest_error, hex, md_by_name, md_from_value, str, str_bytes};
use crate::dispatch::{RObj, RubyObject};
use crate::{ClassId, RubyValue, Signal};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zeo_macros::ruby_class;

pub(crate) struct RDigest {
    /// The subclass this instance reports (`OpenSSL::Digest` for the
    /// name-taking base, one of the eight algorithm ids otherwise).
    class: ClassId,
    /// The canonical algorithm name (`"SHA256"`), as `#name` reports it.
    algo: String,
    /// The accumulated message; hashed fresh on each `digest`/`hexdigest`.
    buf: Mutex<Vec<u8>>,
    frozen: AtomicBool,
}

impl RDigest {
    pub(crate) fn algo_name(&self) -> String {
        self.algo.clone()
    }
    fn md(&self) -> openssl::hash::MessageDigest {
        md_by_name(&self.algo)
            .expect("the algorithm was validated at construction")
            .0
    }
    /// The digest of `data` under this instance's algorithm.
    fn raw(&self, data: &[u8]) -> Result<Vec<u8>, Signal> {
        raw_hash(self.md(), data)
    }
}

impl RubyObject for RDigest {
    fn class_id(&self) -> ClassId {
        self.class
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
        let d = RDigest {
            class: self.class,
            algo: self.algo.clone(),
            buf: Mutex::new(self.buf.lock().clone()),
            frozen: AtomicBool::new(false),
        };
        if copy_frozen {
            d.set_frozen();
        }
        Arc::new(d)
    }
}

fn raw_hash(md: openssl::hash::MessageDigest, data: &[u8]) -> Result<Vec<u8>, Signal> {
    openssl::hash::hash(md, data)
        .map(|d| d.to_vec())
        .map_err(|e| digest_error(&format!("Digest initialization failed: {e}")))
}

/// A fresh digest object under `class`/`algo`, optionally seeded (CRuby's
/// `new(data)` shape). Probes the algorithm once so an EVP fetch failure
/// (an absent legacy provider) surfaces at construction, as CRuby's does.
pub(crate) fn new_digest(
    class: ClassId,
    algo_arg: &RubyValue,
    seed: Option<&RubyValue>,
) -> Result<RubyValue, Signal> {
    let (md, canonical) = md_from_value(algo_arg)?;
    raw_hash(md, b"")?;
    let buf = match seed {
        None | Some(RubyValue::Nil) => Vec::new(),
        Some(v) => str_bytes(v)?,
    };
    Ok(RubyValue::Object(Arc::new(RDigest {
        class,
        algo: canonical,
        buf: Mutex::new(buf),
        frozen: AtomicBool::new(false),
    })))
}

pub(crate) fn digest_of(recv: &RubyValue) -> &RDigest {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RDigest>()
            .expect("the OpenSSL::Digest table only dispatches on digest receivers"),
        _ => unreachable!("the OpenSSL::Digest table only dispatches on digest receivers"),
    }
}

/// Shared body of the instance `hexdigest`/`digest`/`base64digest`: an
/// optional string arg is appended, the buffer hashed, and (per the
/// `Digest::Instance` contract) the object reset when an arg was supplied.
pub(crate) fn finalize(recv: &RubyValue, data: Option<&RubyValue>) -> Result<Vec<u8>, Signal> {
    let d = digest_of(recv);
    let mut buf = d.buf.lock();
    if let Some(arg) = data {
        buf.extend_from_slice(&str_bytes(arg)?);
    }
    let out = d.raw(&buf)?;
    if data.is_some() {
        buf.clear();
    }
    Ok(out)
}

/// `d == other`: another digest compares by raw digest; a String is compared
/// to `d`'s hexdigest (the `Digest::Instance` contract).
fn digest_eq(recv: &RubyValue, other: &RubyValue) -> Result<bool, Signal> {
    let mine = digest_of(recv);
    let my_raw = mine.raw(&mine.buf.lock())?;
    Ok(match other {
        RubyValue::Object(o) if o.as_any().downcast_ref::<RDigest>().is_some() => {
            let theirs = digest_of(other);
            my_raw == theirs.raw(&theirs.buf.lock())?
        }
        RubyValue::Str(s) => hex(&my_raw) == s.lock().to_utf8_lossy(),
        _ => false,
    })
}

ruby_class! {
    Digest = zeo_abi::OPENSSL_DIGEST_CLASS < zeo_abi::OBJECT_CLASS;

    // `OpenSSL::Digest.new(name, data = nil)` -- the name-taking base form.
    def self."new" arity -1 (_recv, arg1, arg2?) {
        new_digest(zeo_abi::OPENSSL_DIGEST_CLASS, arg1, arg2)
    }
    // `OpenSSL::Digest.digest("SHA256", data)` / `.hexdigest` /
    // `.base64digest` -- one-shot class forms, algorithm name first.
    def self."digest" (_recv, arg1, arg2) {
        let (md, _) = md_from_value(arg1)?;
        Ok(bin_str(raw_hash(md, &str_bytes(arg2)?)?))
    }
    def self."hexdigest" (_recv, arg1, arg2) {
        let (md, _) = md_from_value(arg1)?;
        Ok(str(hex(&raw_hash(md, &str_bytes(arg2)?)?)))
    }
    def self."base64digest" (_recv, arg1, arg2) {
        let (md, _) = md_from_value(arg1)?;
        Ok(str(base64(&raw_hash(md, &str_bytes(arg2)?)?)))
    }

    // -- the streaming instance surface (shared by the subclasses via MRO) --
    def "update" | "<<" (recv, other) {
        digest_of(recv).buf.lock().extend_from_slice(&str_bytes(other)?);
        Ok(recv.clone())
    }
    def "hexdigest" | "to_s" arity 0 (recv, data?) {
        Ok(str(hex(&finalize(recv, data)?)))
    }
    def "digest" (recv, data?) {
        Ok(bin_str(finalize(recv, data)?))
    }
    def "base64digest" (recv, data?) {
        Ok(str(base64(&finalize(recv, data)?)))
    }
    def "reset" (recv) {
        digest_of(recv).buf.lock().clear();
        Ok(recv.clone())
    }
    def "name" (recv) {
        Ok(str(digest_of(recv).algo_name()))
    }
    def "digest_length" | "length" | "size" (recv) {
        Ok(RubyValue::Int(digest_of(recv).md().size() as i64))
    }
    def "block_length" (recv) {
        Ok(RubyValue::Int(digest_of(recv).md().block_size() as i64))
    }
    def "==" (recv, other) {
        Ok(RubyValue::Bool(digest_eq(recv, other)?))
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
        let table = registered_table(zeo_abi::OPENSSL_DIGEST_CLASS)
            .and_then(|t| t.instance.as_ref())
            .expect("OpenSSL::Digest registers an instance table");
        (table.lookup)(name).expect("instance method exists")
    }
    fn cm(name: &str) -> crate::builtins::BuiltinMethodFn {
        let table = registered_table(zeo_abi::OPENSSL_DIGEST_CLASS)
            .and_then(|t| t.class.as_ref())
            .expect("OpenSSL::Digest registers a class table");
        (table.lookup)(name).expect("class method exists")
    }
    fn t(v: Result<RubyValue, Signal>) -> String {
        match v.unwrap() {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn class_forms_match_ruby() {
        // ruby 4.0.6: OpenSSL::Digest.hexdigest("SHA256", "abc") et al.
        assert_eq!(
            t(cm("hexdigest")(
                &RubyValue::Nil,
                &[s("SHA256"), s("abc")],
                None
            )),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            t(cm("hexdigest")(
                &RubyValue::Nil,
                &[s("SHA1"), s("abc")],
                None
            )),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn streaming_peek_and_arg_reset_semantics() {
        let d = cm("new")(&RubyValue::Nil, &[s("sha256")], None).unwrap();
        assert_eq!(t(im("name")(&d, &[], None)), "SHA256");
        im("update")(&d, &[s("a")], None).unwrap();
        im("update")(&d, &[s("bc")], None).unwrap();
        // Peeking does not consume.
        assert_eq!(
            t(im("hexdigest")(&d, &[], None)),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            t(im("hexdigest")(&d, &[], None)),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // An arg appends, hashes, then resets.
        im("reset")(&d, &[], None).unwrap();
        assert_eq!(
            t(im("hexdigest")(&d, &[s("abc")], None)),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let fresh = cm("new")(&RubyValue::Nil, &[s("SHA256")], None).unwrap();
        assert_eq!(
            t(im("hexdigest")(&d, &[], None)),
            t(im("hexdigest")(&fresh, &[], None))
        );
    }

    #[test]
    fn lengths_and_seeded_new() {
        let d = cm("new")(&RubyValue::Nil, &[s("SHA384"), s("abc")], None).unwrap();
        assert!(matches!(
            im("digest_length")(&d, &[], None).unwrap(),
            RubyValue::Int(48)
        ));
        assert!(matches!(
            im("block_length")(&d, &[], None).unwrap(),
            RubyValue::Int(128)
        ));
        // Seeding via new(name, data) equals updating after construction.
        let e = cm("new")(&RubyValue::Nil, &[s("SHA384")], None).unwrap();
        im("update")(&e, &[s("abc")], None).unwrap();
        assert_eq!(
            t(im("hexdigest")(&d, &[], None)),
            t(im("hexdigest")(&e, &[], None))
        );
    }
}

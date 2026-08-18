//! `digest` (CRuby's bundled `digest` gem, a C extension) -- the `Digest`
//! framework and its `Digest::MD5`/`SHA1`/`SHA256`/`SHA512` algorithm classes.
//! `require "digest"` activates them. Hashing is RustCrypto (pure Rust, no
//! system libs; pulled in only by the `ext-digest` cargo feature).
//!
//! The four algorithm classes are ONE implementation parameterized by [`Algo`]
//! (each method reads the algorithm off its receiver), so they share a single
//! instance + class table rather than duplicating it four ways. `Digest::MD5`
//! carries that table via `algorithm.rs`'s `ruby_class!`; the other three ids
//! alias it through the linkme registrations below. The `Digest` framework
//! module (`Digest.hexencode`/`bubblebabble`) lives in `digest_module.rs`.
//!
//! The class methods (`Digest::SHA256.hexdigest(str)`) and the streaming
//! instance API (`new`/`update`/`<<`/`hexdigest`/`digest`/`base64digest`/
//! `reset`) are oracle-verified against ruby 4.0.6. An instance stores the
//! accumulated message and hashes it on demand -- simpler than cloning a live
//! hasher, and identical in result.

mod algorithm;
mod digest_module;

use crate::builtins::{BUILTIN_TABLES, BuiltinClassTable, MethodTable};
use crate::dispatch::{RObj, RubyObject};
use crate::{ClassId, RubyValue, Signal, string_new};
use digest::Digest as _;
use linkme::distributed_slice;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zeo_abi::{DIGEST_MD5_CLASS, DIGEST_SHA1_CLASS, DIGEST_SHA256_CLASS, DIGEST_SHA512_CLASS};

// `Digest::MD5` self-registers its (shared) table through `algorithm.rs`'s
// `ruby_class!`; SHA1/SHA256/SHA512 are the same table under a different id.
#[distributed_slice(BUILTIN_TABLES)]
static SHA1_TABLE: BuiltinClassTable = alias_table(DIGEST_SHA1_CLASS);
#[distributed_slice(BUILTIN_TABLES)]
static SHA256_TABLE: BuiltinClassTable = alias_table(DIGEST_SHA256_CLASS);
#[distributed_slice(BUILTIN_TABLES)]
static SHA512_TABLE: BuiltinClassTable = alias_table(DIGEST_SHA512_CLASS);

/// One of the SHA algorithm classes, routed at `id` to the shared MD5-carried
/// table (the methods read the algorithm off the receiver, so the same fns
/// serve every algorithm).
const fn alias_table(id: ClassId) -> BuiltinClassTable {
    BuiltinClassTable {
        id,
        instance: Some(MethodTable {
            lookup: algorithm::lookup,
            names: algorithm::lookup_names,
            arity: algorithm::lookup_arity,
            is_private: algorithm::lookup_is_private,
            is_protected: algorithm::lookup_is_protected,
            allocs: algorithm::lookup_allocs,
            inherits: algorithm::lookup_inherits,
        }),
        class: Some(MethodTable {
            lookup: algorithm::lookup_class,
            names: algorithm::lookup_class_names,
            arity: algorithm::lookup_class_arity,
            is_private: algorithm::lookup_class_is_private,
            is_protected: algorithm::lookup_class_is_protected,
            allocs: algorithm::lookup_class_allocs,
            inherits: algorithm::lookup_class_inherits,
        }),
        install_constants: None,
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Algo {
    Md5,
    Sha1,
    Sha256,
    Sha512,
}

impl Algo {
    fn from_class_id(id: ClassId) -> Algo {
        match id {
            DIGEST_MD5_CLASS => Algo::Md5,
            DIGEST_SHA1_CLASS => Algo::Sha1,
            DIGEST_SHA256_CLASS => Algo::Sha256,
            DIGEST_SHA512_CLASS => Algo::Sha512,
            _ => unreachable!("the Digest table only dispatches on algorithm classes"),
        }
    }
    fn class_id(self) -> ClassId {
        match self {
            Algo::Md5 => DIGEST_MD5_CLASS,
            Algo::Sha1 => DIGEST_SHA1_CLASS,
            Algo::Sha256 => DIGEST_SHA256_CLASS,
            Algo::Sha512 => DIGEST_SHA512_CLASS,
        }
    }
    /// The raw digest bytes of `data` under this algorithm.
    pub(crate) fn raw(self, data: &[u8]) -> Vec<u8> {
        match self {
            Algo::Md5 => md5::Md5::digest(data).to_vec(),
            Algo::Sha1 => sha1::Sha1::digest(data).to_vec(),
            Algo::Sha256 => sha2::Sha256::digest(data).to_vec(),
            Algo::Sha512 => sha2::Sha512::digest(data).to_vec(),
        }
    }
    /// The digest output size in bytes (`Digest#digest_length`).
    fn digest_length(self) -> i64 {
        match self {
            Algo::Md5 => 16,
            Algo::Sha1 => 20,
            Algo::Sha256 => 32,
            Algo::Sha512 => 64,
        }
    }
    /// The internal block size in bytes (`Digest#block_length`).
    fn block_length(self) -> i64 {
        match self {
            Algo::Md5 | Algo::Sha1 | Algo::Sha256 => 64,
            Algo::Sha512 => 128,
        }
    }
}

/// The "bubble babble" encoding of `data` (`Digest.bubblebabble`), the
/// pseudo-word format from the original SSH fingerprint scheme.
pub(crate) fn bubble_babble(data: &[u8]) -> String {
    const VOWELS: &[u8; 6] = b"aeiouy";
    const CONSONANTS: &[u8; 17] = b"bcdfghklmnprstvzx";
    let mut out = vec![b'x'];
    let mut seed: usize = 1;
    let n = data.len();
    let mut i = 0;
    loop {
        if i >= n {
            out.push(VOWELS[seed % 6]);
            out.push(CONSONANTS[16]);
            out.push(VOWELS[seed / 6]);
            break;
        }
        let b1 = data[i] as usize;
        out.push(VOWELS[(((b1 >> 6) & 3) + seed) % 6]);
        out.push(CONSONANTS[(b1 >> 2) & 15]);
        out.push(VOWELS[((b1 & 3) + (seed / 6)) % 6]);
        if i + 1 >= n {
            break;
        }
        let b2 = data[i + 1] as usize;
        out.push(CONSONANTS[(b2 >> 4) & 15]);
        out.push(b'-');
        out.push(CONSONANTS[b2 & 15]);
        seed = (seed * 5 + b1 * 7 + b2) % 36;
        i += 2;
    }
    out.push(b'x');
    String::from_utf8(out).expect("bubble babble is ASCII")
}

pub struct RDigest {
    algo: Algo,
    /// The accumulated message; hashed fresh on each `digest`/`hexdigest`.
    buf: Mutex<Vec<u8>>,
    frozen: AtomicBool,
}

impl RDigest {
    fn new(algo: Algo) -> RDigest {
        RDigest {
            algo,
            buf: Mutex::new(Vec::new()),
            frozen: AtomicBool::new(false),
        }
    }
}

impl RubyObject for RDigest {
    fn class_id(&self) -> ClassId {
        self.algo.class_id()
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
        let d = RDigest::new(self.algo);
        *d.buf.lock() = self.buf.lock().clone();
        if copy_frozen {
            d.set_frozen();
        }
        Arc::new(d)
    }
}

pub(crate) fn digest_of(recv: &RubyValue) -> &RDigest {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RDigest>()
            .expect("the Digest table only dispatches on Digest receivers"),
        _ => unreachable!("the Digest table only dispatches on Digest receivers"),
    }
}

pub(crate) fn algo_of_class(recv: &RubyValue) -> Algo {
    match recv {
        RubyValue::Class(id) => Algo::from_class_id(*id),
        _ => unreachable!("a Digest class method's receiver is its class"),
    }
}

pub(crate) fn in_bytes(v: &RubyValue) -> Result<Vec<u8>, Signal> {
    Ok(crate::builtins::convert::to_rstr(v)?
        .lock()
        .bytes()
        .to_vec())
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard RFC 4648 base64 with padding -- inlined so `ext-digest` doesn't
/// depend on the (separately gated) `ext-base64` module.
pub(crate) fn base64(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        s.push(B64[(n >> 18) as usize & 63] as char);
        s.push(B64[(n >> 12) as usize & 63] as char);
        s.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        s.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    s
}

/// Shared body of the instance `hexdigest`/`digest`/`base64digest`: an optional
/// string arg is appended, the buffer hashed, and (per CRuby) the object reset
/// when an arg was supplied.
pub(crate) fn finalize(recv: &RubyValue, data: Option<&RubyValue>) -> Result<Vec<u8>, Signal> {
    let d = digest_of(recv);
    let mut buf = d.buf.lock();
    if let Some(arg) = data {
        buf.extend_from_slice(&in_bytes(arg)?);
    }
    let out = d.algo.raw(&buf);
    if data.is_some() {
        buf.clear();
    }
    Ok(out)
}

/// A fresh streaming digest object for `algo` (the shared `.new` body).
pub(crate) fn new_digest(algo: Algo) -> RubyValue {
    RubyValue::Object(Arc::new(RDigest::new(algo)))
}

pub(crate) fn digest_length_of(recv: &RubyValue) -> i64 {
    digest_of(recv).algo.digest_length()
}

pub(crate) fn block_length_of(recv: &RubyValue) -> i64 {
    digest_of(recv).algo.block_length()
}

/// `d == other`: another Digest compares by raw digest; anything else is
/// compared to `d`'s hexdigest (CRuby's `to_str` path).
pub(crate) fn digest_eq(recv: &RubyValue, other: &RubyValue) -> bool {
    let mine = digest_of(recv);
    let my_raw = mine.algo.raw(&mine.buf.lock());
    match other {
        RubyValue::Object(o) if o.as_any().downcast_ref::<RDigest>().is_some() => {
            let other = digest_of(other);
            my_raw == other.algo.raw(&other.buf.lock())
        }
        RubyValue::Str(s) => hex(&my_raw) == s.lock().to_utf8_lossy(),
        _ => false,
    }
}

/// The bubble babble of a receiver's current digest.
pub(crate) fn digest_bubblebabble(recv: &RubyValue) -> String {
    let d = digest_of(recv);
    bubble_babble(&d.algo.raw(&d.buf.lock()))
}

/// Append `bytes` to a receiver's buffer (the shared `update`/`<<` body).
pub(crate) fn push_bytes(recv: &RubyValue, bytes: &[u8]) {
    digest_of(recv).buf.lock().extend_from_slice(bytes);
}

/// Clear a receiver's buffer (`reset`).
pub(crate) fn reset_buf(recv: &RubyValue) {
    digest_of(recv).buf.lock().clear();
}

pub(crate) fn str(s: String) -> RubyValue {
    RubyValue::Str(string_new(s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::registered_table;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(string_new(text.to_string()))
    }
    fn cls(id: ClassId) -> RubyValue {
        RubyValue::Class(id)
    }
    fn im(name: &str) -> crate::builtins::BuiltinMethodFn {
        let table = registered_table(DIGEST_MD5_CLASS)
            .and_then(|t| t.instance.as_ref())
            .expect("Digest registers an instance table");
        (table.lookup)(name).expect("instance method exists")
    }
    fn cm(name: &str) -> crate::builtins::BuiltinMethodFn {
        let table = registered_table(DIGEST_MD5_CLASS)
            .and_then(|t| t.class.as_ref())
            .expect("Digest registers a class table");
        (table.lookup)(name).expect("class method exists")
    }
    fn t(v: Result<RubyValue, Signal>) -> String {
        match v.unwrap() {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn class_hexdigest_matches_ruby() {
        assert_eq!(
            t(cm("hexdigest")(&cls(DIGEST_MD5_CLASS), &[s("")], None)),
            "d41d8cd98f00b204e9800998ecf8427e"
        );
        assert_eq!(
            t(cm("hexdigest")(&cls(DIGEST_SHA1_CLASS), &[s("abc")], None)),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            t(cm("hexdigest")(
                &cls(DIGEST_SHA256_CLASS),
                &[s("abc")],
                None
            )),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            t(cm("base64digest")(
                &cls(DIGEST_SHA256_CLASS),
                &[s("abc")],
                None
            )),
            "ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0="
        );
    }

    #[test]
    fn streaming_update_matches_oneshot() {
        let d = cm("new")(&cls(DIGEST_SHA256_CLASS), &[], None).unwrap();
        im("update")(&d, &[s("a")], None).unwrap();
        im("update")(&d, &[s("bc")], None).unwrap();
        assert_eq!(
            t(im("hexdigest")(&d, &[], None)),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn digest_and_block_lengths_match_ruby() {
        for (id, dlen, blen) in [
            (DIGEST_MD5_CLASS, 16, 64),
            (DIGEST_SHA1_CLASS, 20, 64),
            (DIGEST_SHA256_CLASS, 32, 64),
            (DIGEST_SHA512_CLASS, 64, 128),
        ] {
            let d = cm("new")(&cls(id), &[], None).unwrap();
            assert!(
                matches!(im("digest_length")(&d, &[], None).unwrap(), RubyValue::Int(n) if n == dlen)
            );
            assert!(
                matches!(im("block_length")(&d, &[], None).unwrap(), RubyValue::Int(n) if n == blen)
            );
        }
    }

    #[test]
    fn equality_compares_digest_or_hexdigest() {
        let a = cm("new")(&cls(DIGEST_SHA256_CLASS), &[], None).unwrap();
        im("update")(&a, &[s("hello")], None).unwrap();
        let b = cm("new")(&cls(DIGEST_SHA256_CLASS), &[], None).unwrap();
        im("update")(&b, &[s("hello")], None).unwrap();
        assert!(matches!(
            im("==")(&a, &[b], None).unwrap(),
            RubyValue::Bool(true)
        ));
        let hexed = t(im("hexdigest")(&a, &[], None));
        assert!(matches!(
            im("==")(&a, &[s(&hexed)], None).unwrap(),
            RubyValue::Bool(true)
        ));
        assert!(matches!(
            im("==")(&a, &[s("nope")], None).unwrap(),
            RubyValue::Bool(false)
        ));
    }

    #[test]
    fn bubble_babble_matches_ruby_reference() {
        // `Digest.bubblebabble("1234567890")` from ruby 4.0.6.
        assert_eq!(
            bubble_babble(b"1234567890"),
            "xesef-disof-gytuf-katof-movif-baxux"
        );
        assert_eq!(bubble_babble(b"Pineapple"), "xigak-nyryk-humil-bosek-sonax");
        assert_eq!(bubble_babble(b""), "xexax");
    }
}

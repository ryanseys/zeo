//! The fixed-algorithm `OpenSSL::Digest` subclasses (`OpenSSL::Digest::MD4`
//! .. `::SHA512`): one shared CLASS-method table whose methods read the
//! algorithm off the receiver class, registered here under `MD4`; the other
//! seven ids alias it via the linkme entries below (the `Digest::MD5`-style
//! aliasing -- see `ext/digest/mod.rs`). The instance surface is inherited
//! from `OpenSSL::Digest` through the MRO, so the alias tables carry no
//! instance half.

use super::digest::new_digest;
use super::{bin_str, hex, md_by_name, str, str_bytes};
use crate::builtins::{BUILTIN_TABLES, BuiltinClassTable, MethodTable};
use crate::{ClassId, RubyValue, Signal};
use linkme::distributed_slice;
use zeo_abi::{
    OPENSSL_DIGEST_MD4_CLASS, OPENSSL_DIGEST_MD5_CLASS, OPENSSL_DIGEST_RIPEMD160_CLASS,
    OPENSSL_DIGEST_SHA1_CLASS, OPENSSL_DIGEST_SHA224_CLASS, OPENSSL_DIGEST_SHA256_CLASS,
    OPENSSL_DIGEST_SHA384_CLASS, OPENSSL_DIGEST_SHA512_CLASS,
};
use zeo_macros::ruby_class;

#[distributed_slice(BUILTIN_TABLES)]
static MD5_TABLE: BuiltinClassTable = alias_table(OPENSSL_DIGEST_MD5_CLASS);
#[distributed_slice(BUILTIN_TABLES)]
static RIPEMD160_TABLE: BuiltinClassTable = alias_table(OPENSSL_DIGEST_RIPEMD160_CLASS);
#[distributed_slice(BUILTIN_TABLES)]
static SHA1_TABLE: BuiltinClassTable = alias_table(OPENSSL_DIGEST_SHA1_CLASS);
#[distributed_slice(BUILTIN_TABLES)]
static SHA224_TABLE: BuiltinClassTable = alias_table(OPENSSL_DIGEST_SHA224_CLASS);
#[distributed_slice(BUILTIN_TABLES)]
static SHA256_TABLE: BuiltinClassTable = alias_table(OPENSSL_DIGEST_SHA256_CLASS);
#[distributed_slice(BUILTIN_TABLES)]
static SHA384_TABLE: BuiltinClassTable = alias_table(OPENSSL_DIGEST_SHA384_CLASS);
#[distributed_slice(BUILTIN_TABLES)]
static SHA512_TABLE: BuiltinClassTable = alias_table(OPENSSL_DIGEST_SHA512_CLASS);

const fn alias_table(id: ClassId) -> BuiltinClassTable {
    BuiltinClassTable {
        id,
        instance: None,
        class: Some(MethodTable {
            lookup: lookup_class,
            names: lookup_class_names,
            arity: lookup_class_arity,
            is_private: lookup_class_is_private,
            is_protected: lookup_class_is_protected,
            allocs: lookup_class_allocs,
            inherits: lookup_class_inherits,
        }),
        install_constants: None,
    }
}

/// The receiver class's fixed algorithm name.
fn algo_of_class(recv: &RubyValue) -> (ClassId, &'static str) {
    let RubyValue::Class(id) = recv else {
        unreachable!("an algorithm class method's receiver is its class")
    };
    let name = match *id {
        OPENSSL_DIGEST_MD4_CLASS => "MD4",
        OPENSSL_DIGEST_MD5_CLASS => "MD5",
        OPENSSL_DIGEST_RIPEMD160_CLASS => "RIPEMD160",
        OPENSSL_DIGEST_SHA1_CLASS => "SHA1",
        OPENSSL_DIGEST_SHA224_CLASS => "SHA224",
        OPENSSL_DIGEST_SHA256_CLASS => "SHA256",
        OPENSSL_DIGEST_SHA384_CLASS => "SHA384",
        OPENSSL_DIGEST_SHA512_CLASS => "SHA512",
        _ => unreachable!("the algorithm table only dispatches on algorithm classes"),
    };
    (*id, name)
}

fn class_raw(recv: &RubyValue, arg: &RubyValue) -> Result<Vec<u8>, Signal> {
    let (_, name) = algo_of_class(recv);
    let (md, _) = md_by_name(name)?;
    openssl::hash::hash(md, &str_bytes(arg)?)
        .map(|d| d.to_vec())
        .map_err(|e| super::digest_error(&format!("Digest initialization failed: {e}")))
}

ruby_class! {
    // Registered under MD4; the other seven algorithm ids alias this table.
    DigestAlgo = zeo_abi::OPENSSL_DIGEST_MD4_CLASS < zeo_abi::OPENSSL_DIGEST_CLASS;

    // `OpenSSL::Digest::SHA256.new(data = nil)` -- the algorithm is the
    // receiver class's own.
    def self."new" (recv, arg?) {
        let (id, name) = algo_of_class(recv);
        new_digest(id, &str(name.to_string()), arg)
    }
    // One-shot class forms, data only.
    def self."digest" (recv, arg) {
        Ok(bin_str(class_raw(recv, arg)?))
    }
    def self."hexdigest" (recv, arg) {
        Ok(str(hex(&class_raw(recv, arg)?)))
    }
    def self."base64digest" (recv, arg, *_rest) {
        Ok(str(super::base64(&class_raw(recv, arg)?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::registered_table;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(crate::string_new(text.to_string()))
    }
    fn cm(id: ClassId, name: &str) -> crate::builtins::BuiltinMethodFn {
        let table = registered_table(id)
            .and_then(|t| t.class.as_ref())
            .expect("algorithm class registers a class table");
        (table.lookup)(name).expect("class method exists")
    }
    fn t(v: Result<RubyValue, Signal>) -> String {
        match v.unwrap() {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn subclass_one_shots_match_ruby() {
        assert_eq!(
            t(cm(OPENSSL_DIGEST_SHA256_CLASS, "hexdigest")(
                &RubyValue::Class(OPENSSL_DIGEST_SHA256_CLASS),
                &[s("abc")],
                None
            )),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            t(cm(OPENSSL_DIGEST_MD5_CLASS, "hexdigest")(
                &RubyValue::Class(OPENSSL_DIGEST_MD5_CLASS),
                &[s("abc")],
                None
            )),
            "900150983cd24fb0d6963f7d28e17f72"
        );
    }

    #[test]
    fn subclass_new_reports_its_class_and_name() {
        let d = cm(OPENSSL_DIGEST_SHA1_CLASS, "new")(
            &RubyValue::Class(OPENSSL_DIGEST_SHA1_CLASS),
            &[],
            None,
        )
        .unwrap();
        let RubyValue::Object(o) = &d else {
            panic!("expected an Object")
        };
        assert_eq!(o.class_id(), OPENSSL_DIGEST_SHA1_CLASS);
        // The instance surface is inherited: resolve through the BASE table.
        let base = registered_table(zeo_abi::OPENSSL_DIGEST_CLASS)
            .and_then(|t| t.instance.as_ref())
            .expect("base instance table");
        let name = (base.lookup)("name").expect("name");
        assert_eq!(t(name(&d, &[], None)), "SHA1");
    }
}

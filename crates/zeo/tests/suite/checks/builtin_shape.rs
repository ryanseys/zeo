//! The `ruby_class!` header vs `zeo_abi::BUILTINS`: one class's shape, spelled
//! twice.
//!
//! A builtin names its identity three times -- the header identifier
//! (`String`), its `ClassId` const, and the `BUILTINS` row the compiler seeds
//! its class arena from. The header's `< SUPER` and `include` lines restate
//! the ABI, and without this comparison they are decoration that drifts.
//!
//! `CLASS_SURFACE` carries the header's shape symbolically (rustc resolves
//! each `zeo_abi::` const), so the two can be compared row for row.
//!
//! Accepted mismatches live in [`ALLOWED`], each with the reason -- a header
//! identifier is a Rust `Ident` and cannot spell `ARGF.class` or
//! `FFI::Type::Builtin`, so those two can never match by construction.

use std::collections::BTreeMap;

use zeo::builtin_surface::CLASS_SURFACE;
use zeo_abi::{BuiltinClass, ClassId};

/// Header name -> the `BUILTINS` name it legitimately differs from, and why.
///
/// A header identifier is compared against the ABI name's LAST namespace
/// segment (`OpenSSL::BN` -> `BN`), which is what a Rust `Ident` can spell.
/// These are the headers that still differ after that.
const ALLOWED: &[(&str, &str, &str)] = &[
    (
        "Argf",
        "ARGF.class",
        "a Rust Ident cannot spell a name with a dot",
    ),
    (
        "TypeBuiltin",
        "FFI::Type::Builtin",
        "nested under Type in the same file, so the ident is prefixed",
    ),
    (
        "Digest",
        "Digest::MD5",
        "one table registered under MD5 that SHA1/SHA256/SHA512 alias",
    ),
    (
        "DigestAlgo",
        "OpenSSL::Digest::MD4",
        "one table registered under MD4 that the other seven algorithms alias",
    ),
];

/// The last `::` segment of a Ruby name -- all a Rust `Ident` can carry.
fn leaf(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

fn abi_rows() -> BTreeMap<u32, &'static BuiltinClass> {
    zeo_abi::BUILTINS.iter().map(|b| (b.id.0, b)).collect()
}

fn name_of(id: ClassId, rows: &BTreeMap<u32, &'static BuiltinClass>) -> String {
    rows.get(&id.0)
        .map(|b| b.name.to_string())
        .unwrap_or_else(|| format!("<no BUILTINS row for ClassId({})>", id.0))
}

/// Every migrated header agrees with the ABI row for its own `ClassId` on the
/// three facts both spell: Ruby name, superclass, and includes.
#[test]
fn every_header_agrees_with_the_abi_row_it_restates() {
    let rows = abi_rows();
    let mut drift: Vec<String> = Vec::new();

    for s in CLASS_SURFACE {
        let Some(abi) = rows.get(&s.id.0) else {
            // A header whose ClassId has no ABI row at all -- reported by the
            // companion test below, not here.
            continue;
        };

        let name_ok = leaf(abi.name) == s.header_name
            || ALLOWED
                .iter()
                .any(|(header, abi_name, _)| *header == s.header_name && *abi_name == abi.name);
        if !name_ok {
            drift.push(format!(
                "header `{}` vs BUILTINS `{}`",
                s.header_name, abi.name
            ));
        }

        if abi.is_module != s.is_module {
            drift.push(format!(
                "{}: header declares a {}, BUILTINS says {}",
                s.header_name,
                if s.is_module { "module" } else { "class" },
                if abi.is_module { "module" } else { "class" },
            ));
        }

        if abi.superclass.map(|c| c.0) != s.superclass.map(|c| c.0) {
            drift.push(format!(
                "{}: header superclass {:?}, BUILTINS {:?}",
                s.header_name,
                s.superclass.map(|c| name_of(c, &rows)),
                abi.superclass.map(|c| name_of(c, &rows)),
            ));
        }

        let header_includes: Vec<u32> = s.includes.iter().map(|c| c.0).collect();
        let abi_includes: Vec<u32> = abi.includes.iter().map(|c| c.0).collect();
        if header_includes != abi_includes {
            drift.push(format!(
                "{}: header includes {:?}, BUILTINS {:?}",
                s.header_name,
                s.includes
                    .iter()
                    .map(|&c| name_of(c, &rows))
                    .collect::<Vec<_>>(),
                abi.includes
                    .iter()
                    .map(|&c| name_of(c, &rows))
                    .collect::<Vec<_>>(),
            ));
        }
    }

    assert!(
        drift.is_empty(),
        "{} builtin header(s) disagree with their ABI row:\n  {}\n\
         Fix the header or the BUILTINS row -- they describe the same class.",
        drift.len(),
        drift.join("\n  "),
    );
}

/// A `ClassId` const with no `BUILTINS` row is a class the runtime registers
/// and the compiler cannot see: no name, no ancestry, no `require` gate. Every
/// such id must be listed here with the reason it has no row.
#[test]
fn every_header_class_id_has_an_abi_row() {
    /// Header name -> why it has no `BUILTINS` row.
    const ROWLESS: &[(&str, &str)] = &[(
        "Env",
        "ENV is not a class: `ENV.class` is Object, so its rows hang off the \
         reserved ENV_SINGLETON_CLASS table key that dispatch reaches by \
         identity. Nothing reports that id, so it has nothing to describe.",
    )];

    let rows = abi_rows();
    let missing: Vec<&str> = CLASS_SURFACE
        .iter()
        .filter(|s| !rows.contains_key(&s.id.0))
        .filter(|s| !ROWLESS.iter().any(|(name, _)| *name == s.header_name))
        .map(|s| s.header_name)
        .collect();

    assert!(
        missing.is_empty(),
        "{} header(s) declare a ClassId with no zeo_abi::BUILTINS row: {missing:?}\n\
         Add the row, or list it in ROWLESS with the reason.",
        missing.len(),
    );
}

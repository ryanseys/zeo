//! The gem disclosure record: for every library a program `require`s,
//! how zeo actually satisfied it -- and, where zeo's answer is NOT the
//! upstream gem/extension, that it DIVERGES, with a note naming the backing.
//!
//! Why this exists (the GraalVM lesson): a substitution is silent by nature --
//! zeo's `json` behaves *almost* like the gem until an edge case where it
//! doesn't, and a user debugging that has no breadcrumb unless the swap was
//! recorded up front. `--report` writes this record next to the artifact.
//! It is the ONLY disclosure channel: a compiler diagnostic on every run is
//! noise on the program's own stderr, which nothing else in ruby produces.
//!
//! The record is populated by `parse::loader` as each require resolves and
//! rides on `Hir::gem_records`; `compile_to_object_with` writes the JSON
//! once lowering is done.

use std::collections::BTreeMap;
use std::path::Path;

/// One required library and how it was satisfied. Deduped by `name` at record
/// time (`Hir::record_gem`), first-wins -- the user-facing entry point (a
/// bundled gem's `.rb`) is recorded before its internal `.so` loader-idiom
/// require, which is skipped outright.
#[derive(Clone, Debug)]
pub struct GemRecord {
    /// The library name as required (`"json"`, `"optparse"`).
    pub name: String,
    pub by: SatisfiedBy,
}

/// The mechanism that satisfied a require. `Excluded` is reserved for the
/// external gem store, where a native gem zeo has no ext for is a
/// first-class, recorded outcome rather than a hard in-tree compile error.
#[derive(Clone, Debug)]
pub enum SatisfiedBy {
    /// A statically-linked in-tree `ext/` feature (`require "base64"`).
    BuiltinExt { feature: String },
    /// A zeo-bundled gem under `gems/<name>/` (`require "optparse"`).
    BundledGem { path: String },
    /// A file found on a `-I` load root (the installed Ruby's own stdlib).
    StdlibRoot { path: String },
    /// The gem shipped its C as SOURCE and zeo compiled it. `library` is
    /// the shared object, which the program dlopens at the require's own
    /// line -- so it has to be there at RUN time too, and naming it is what
    /// makes that checkable.
    CompiledExt { library: String },
    /// Recorded but unavailable: a native gem the external store located but
    /// zeo has no ext for, or a library only a method body required.
    Excluded { kind: String, reason: String },
}

/// `Excluded::kind` for a library only a method body required. zeo does not
/// load it, so the record is the only breadcrumb before the runtime
/// `LoadError`.
pub const DEFERRED_KIND: &str = "deferred-require";

/// If zeo's implementation of `name` is NOT the upstream gem/extension,
/// the note explaining what it actually is. `Some` here is exactly the
/// `diverges: true` set -- a curated table, because "serde_json is not the
/// json gem" is a fact that can only be stated, never inferred.
pub fn substitution_note(name: &str) -> Option<&'static str> {
    match name {
        "json" => Some("serde_json-backed; not the json gem"),
        "psych" | "yaml" => Some("yaml-rust2-backed; not libyaml/the psych gem"),
        "zlib" => Some("flate2-backed; not the zlib C extension"),
        "digest" => Some("RustCrypto-backed; not the OpenSSL digest C extension"),
        "openssl" => Some(
            "a vendored OpenSSL 3 via rust-openssl; PKey generation, X509 issuance and SSLServer are absent",
        ),
        "strscan" => Some("a zeo reimplementation of StringScanner"),
        "stringio" => Some("a zeo reimplementation of StringIO"),
        "date" => Some("a zeo reimplementation of Date/DateTime"),
        "socket" => Some("a partial zeo reimplementation"),
        "base64" => Some("a zeo reimplementation of Base64"),
        "cgi" | "cgi/escape" => Some("a zeo reimplementation of CGI escaping"),
        "nkf" => Some("a zeo reimplementation over its own encoding engine; not the nkf C library"),
        "bigdecimal" => Some("native core reimplemented; the gem's Ruby half is vendored upstream"),
        "ffi" => {
            Some("zeo's own FFI (compile-time externs + a libffi runtime tier); not the ffi gem")
        }
        "fiddle" => Some("the gem's own FFI backend over zeo's ffi; not the fiddle C extension"),
        "coverage" => {
            Some("line coverage over zeo's AOT instrumentation; not the VM coverage extension")
        }
        _ => None,
    }
}

/// Write `target/zeo-gems.json` (or wherever the CLI pointed): one object
/// keyed by library name, each recording `by` and -- for a substitution --
/// `diverges: true` plus a `note`. Keys are sorted for a stable, diffable file.
pub fn write_report(records: &[GemRecord], path: &Path) -> Result<(), String> {
    let mut by_name: BTreeMap<&str, &GemRecord> = BTreeMap::new();
    for r in records {
        by_name.entry(&r.name).or_insert(r);
    }

    let mut out = String::from("{\n");
    for (i, (name, r)) in by_name.iter().enumerate() {
        out.push_str("  ");
        out.push_str(&json_str(name));
        out.push_str(": {");
        out.push_str(&entry_body(r));
        out.push('}');
        out.push_str(if i + 1 < by_name.len() { ",\n" } else { "\n" });
    }
    out.push_str("}\n");

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    std::fs::write(path, out).map_err(|e| format!("writing {}: {e}", path.display()))
}

/// The `{...}` body for one record's JSON object (no braces).
fn entry_body(r: &GemRecord) -> String {
    let mut fields: Vec<String> = Vec::new();
    match &r.by {
        SatisfiedBy::BuiltinExt { feature } => {
            fields.push("\"by\": \"builtin-ext\"".to_string());
            fields.push(format!("\"feature\": {}", json_str(feature)));
        }
        SatisfiedBy::BundledGem { path } => {
            fields.push("\"by\": \"bundled-gem\"".to_string());
            fields.push(format!("\"path\": {}", json_str(path)));
        }
        SatisfiedBy::StdlibRoot { path } => {
            fields.push("\"by\": \"stdlib-root\"".to_string());
            fields.push(format!("\"path\": {}", json_str(path)));
        }
        SatisfiedBy::CompiledExt { library } => {
            fields.push("\"by\": \"compiled-ext\"".to_string());
            fields.push(format!("\"library\": {}", json_str(library)));
        }
        SatisfiedBy::Excluded { kind, reason } => {
            fields.push("\"by\": null".to_string());
            fields.push(format!("\"excluded\": {}", json_str(kind)));
            fields.push(format!("\"reason\": {}", json_str(reason)));
        }
    }
    if let Some(note) = substitution_note(&r.name) {
        fields.push("\"diverges\": true".to_string());
        fields.push(format!("\"note\": {}", json_str(note)));
    }
    fields.join(", ")
}

/// A JSON string literal: quote, and escape the characters JSON requires.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

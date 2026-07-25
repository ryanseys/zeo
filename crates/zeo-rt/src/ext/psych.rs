//! `psych` / `yaml` (CRuby's bundled YAML engine). `require "psych"` activates
//! `Psych`; `require "yaml"` additionally exposes `YAML` (the same module --
//! Ruby's `yaml.rb` is just `YAML = Psych`). Both dispatch here.
//!
//! Loading goes through `yaml-rust2` (the maintained pure-Rust YAML 1.2 parser;
//! `serde_yaml` is deprecated/archived), whose `Yaml` node tree converts
//! directly to `RubyValue`. Dumping is hand-rolled to reproduce Psych's block
//! style, which the yaml-rust2 emitter does not match (Psych keeps a sequence
//! nested under a mapping key at the KEY's indent, not one level deeper).
//!
//! Oracle-verified against ruby 4.0.5 for scalars, sequences, mappings, and
//! their nesting. A YAML syntax error raises `Psych::SyntaxError`, which this
//! gem's RUBY half (`gems/psych/lib/psych.rb`) defines -- see `ext/json.rs`
//! for why the exception lives there and not here. Documented divergences:
//! `Psych::SyntaxError` carries no file/line/column readers (the YAML backend
//! does not surface positions); anchors/aliases and custom
//! tags load as `nil`; exotic scalar styles (multi-line block scalars) dump as
//! quoted strings rather than `|-` blocks. `load`/`load_file`/`load_stream` are
//! built; the `parse`/`parse_stream` node-tree API (`Psych::Nodes::*`) raises
//! NotImplementedError (not modelled).

use crate::builtins::{arity, not_impl_error};
use crate::collections::{array_new, hash_new, hash_pairs};
use crate::dispatch::raise_error;
use crate::{RubyValue, Signal, string_new};
use zeo_macros::ruby_module;
use yaml_rust2::{Yaml, YamlLoader};

fn yaml_to_ruby(y: &Yaml) -> RubyValue {
    match y {
        Yaml::Null | Yaml::BadValue => RubyValue::Nil,
        Yaml::Boolean(b) => RubyValue::Bool(*b),
        Yaml::Integer(i) => RubyValue::Int(*i),
        Yaml::Real(s) => RubyValue::Float(parse_real(s)),
        Yaml::String(s) => RubyValue::Str(string_new(s.clone())),
        Yaml::Array(a) => RubyValue::Array(array_new(a.iter().map(yaml_to_ruby).collect())),
        Yaml::Hash(h) => RubyValue::Hash(hash_new(
            h.iter()
                .map(|(k, v)| (yaml_to_ruby(k), yaml_to_ruby(v)))
                .collect(),
        )),
        // Aliases resolve to their anchor in a full loader; unsupported here.
        Yaml::Alias(_) => RubyValue::Nil,
    }
}

fn parse_real(s: &str) -> f64 {
    match s {
        ".inf" | ".Inf" | ".INF" | "+.inf" => f64::INFINITY,
        "-.inf" | "-.Inf" | "-.INF" => f64::NEG_INFINITY,
        ".nan" | ".NaN" | ".NAN" => f64::NAN,
        _ => s.parse().unwrap_or(f64::NAN),
    }
}

fn load_text(v: &RubyValue) -> Result<String, Signal> {
    Ok(crate::builtins::convert::to_rstr(v)?
        .lock()
        .to_utf8_lossy()
        .into_owned())
}

// ---- dump (hand-rolled Psych block style) -------------------------------

fn is_nonempty_collection(v: &RubyValue) -> bool {
    match v {
        RubyValue::Array(a) => !a.lock().is_empty(),
        RubyValue::Hash(h) => crate::collections::hash_len(h) > 0,
        _ => false,
    }
}

/// A single scalar's YAML text (no indentation, no newline).
fn scalar(v: &RubyValue) -> String {
    match v {
        RubyValue::Nil => String::new(),
        RubyValue::Bool(b) => b.to_string(),
        RubyValue::Int(_) | RubyValue::Float(_) | RubyValue::BigInt(_) => v.to_display_string(),
        RubyValue::Symbol(s) => format!(":{}", s.name()),
        RubyValue::Str(s) => yaml_string(&s.lock().to_utf8_lossy()),
        RubyValue::Array(_) => "[]".to_string(), // only reached for empty arrays
        RubyValue::Hash(_) => "{}".to_string(),  // only reached for empty hashes
        other => yaml_string(&other.to_display_string()),
    }
}

/// A string as a YAML scalar: plain when unambiguous, else single-quoted
/// (Psych's default), else double-quoted for strings with newlines.
fn yaml_string(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    if s.contains('\n') {
        return format!(
            "\"{}\"",
            s.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
        );
    }
    if is_plain_safe(s) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "''"))
    }
}

/// Whether `s` can be a plain (unquoted) YAML scalar without being misread as a
/// number/bool/null or breaking block structure.
fn is_plain_safe(s: &str) -> bool {
    let reserved = matches!(
        s,
        "true"
            | "false"
            | "null"
            | "yes"
            | "no"
            | "on"
            | "off"
            | "~"
            | "True"
            | "False"
            | "Null"
            | "TRUE"
            | "FALSE"
            | "NULL"
    );
    let first_ok = s
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    let body_ok = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ' ' | '/' | '.' | '-'));
    let edge_ok = !s.starts_with(' ') && !s.ends_with(' ');
    let no_colon = !s.contains(": ") && !s.ends_with(':');
    first_ok && body_ok && edge_ok && no_colon && !reserved
}

fn pad(indent: usize, out: &mut String) {
    out.push_str(&"  ".repeat(indent));
}

/// Emit a block sequence at `indent` (each `- item`).
fn emit_seq(a: &[RubyValue], indent: usize, out: &mut String) {
    for el in a {
        pad(indent, out);
        out.push('-');
        emit_after_dash(el, indent, out);
    }
}

/// The value following a `-` sequence marker.
fn emit_after_dash(v: &RubyValue, indent: usize, out: &mut String) {
    match v {
        _ if !is_nonempty_collection(v) => {
            out.push(' ');
            out.push_str(&scalar(v));
            out.push('\n');
        }
        RubyValue::Array(a) => {
            out.push('\n');
            emit_seq(&a.lock(), indent + 1, out);
        }
        RubyValue::Hash(h) => {
            // First pair rides the dash line; the rest indent one level in.
            out.push(' ');
            emit_map_from(&hash_pairs(h), indent + 1, out);
        }
        _ => unreachable!("non-collection handled above"),
    }
}

/// Emit a block mapping at `indent`.
fn emit_map(pairs: &[(RubyValue, RubyValue)], indent: usize, out: &mut String) {
    for pair in pairs {
        pad(indent, out);
        emit_pair(pair, indent, out);
    }
}

/// Like `emit_map` but the FIRST pair is written where the cursor already is
/// (used right after a `- ` marker); later pairs get full indentation.
fn emit_map_from(pairs: &[(RubyValue, RubyValue)], indent: usize, out: &mut String) {
    for (i, pair) in pairs.iter().enumerate() {
        if i > 0 {
            pad(indent, out);
        }
        emit_pair(pair, indent, out);
    }
}

fn emit_pair(pair: &(RubyValue, RubyValue), indent: usize, out: &mut String) {
    let (k, v) = pair;
    out.push_str(&scalar(k));
    out.push(':');
    match v {
        _ if !is_nonempty_collection(v) => {
            if matches!(v, RubyValue::Nil) {
                out.push('\n'); // `key:\n`
            } else {
                out.push(' ');
                out.push_str(&scalar(v));
                out.push('\n');
            }
        }
        // Psych quirk: a sequence under a key stays at the KEY's indent...
        RubyValue::Array(a) => {
            out.push('\n');
            emit_seq(&a.lock(), indent, out);
        }
        // ...but a nested mapping indents one level deeper.
        RubyValue::Hash(h) => {
            out.push('\n');
            emit_map(&hash_pairs(h), indent + 1, out);
        }
        _ => unreachable!("non-collection handled above"),
    }
}

fn dump(v: &RubyValue) -> String {
    match v {
        RubyValue::Nil => "---\n".to_string(),
        RubyValue::Array(a) if !a.lock().is_empty() => {
            let mut out = "---\n".to_string();
            emit_seq(&a.lock(), 0, &mut out);
            out
        }
        RubyValue::Hash(h) if crate::collections::hash_len(h) > 0 => {
            let mut out = "---\n".to_string();
            emit_map(&hash_pairs(h), 0, &mut out);
            out
        }
        scalar_v => format!("--- {}\n", scalar(scalar_v)),
    }
}

ruby_module! {
    Psych = zeo_abi::PSYCH_MODULE;

    // CRuby's `Psych.load`/`dump`/... are singleton module methods (`def self.`),
    // each arity -2 (one required arg + optional opts).
    def self."load" arity -2 | "unsafe_load" arity -2 | "safe_load" arity -2 (_recv, args, _block) {
        arity!(args, 1..=2); // (yaml[, opts]) -- opts ignored
        let text = load_text(&args[0])?;
        let docs = YamlLoader::load_from_str(&text)
            .map_err(|e| raise_error("Psych::SyntaxError", format!("{e}")))?;
        Ok(docs.first().map(yaml_to_ruby).unwrap_or(RubyValue::Nil))
    }
    def self."dump" arity -2 (_recv, args, _block) {
        arity!(args, 1..=2); // (obj[, io/opts]) -- only the compact string form
        Ok(RubyValue::Str(string_new(dump(&args[0]))))
    }

    // `Psych.load_file(path)` -- read the file and load its first document.
    def self."load_file" arity -2 (_recv, args, _block) {
        arity!(args, 1..=2); // (path[, opts]) -- opts ignored
        let path = crate::builtins::file::path_arg(&args[0], "load_file")?;
        let text = crate::gvl::without_gvl(|| std::fs::read_to_string(&path)).map_err(|e| raise_error(
            "Errno::ENOENT",
            format!("No such file or directory - {path} ({e})"),
        ))?;
        let docs = YamlLoader::load_from_str(&text)
            .map_err(|e| raise_error("Psych::SyntaxError", format!("{e}")))?;
        Ok(docs.first().map(yaml_to_ruby).unwrap_or(RubyValue::Nil))
    }
    // `Psych.load_stream(yaml)` -- EVERY document; an Array, or yielded one by
    // one to a block (then the receiver's nil, matching CRuby's block form).
    def self."load_stream" arity -2 (_recv, args, block) {
        arity!(args, 1..=2);
        let text = load_text(&args[0])?;
        let docs = YamlLoader::load_from_str(&text)
            .map_err(|e| raise_error("Psych::SyntaxError", format!("{e}")))?;
        if let Some(RubyValue::Proc(p)) = &block {
            for doc in &docs {
                p.call(std::slice::from_ref(&yaml_to_ruby(doc)))?;
            }
            return Ok(RubyValue::Nil);
        }
        Ok(RubyValue::Array(crate::array_new(docs.iter().map(yaml_to_ruby).collect())))
    }

    // The `parse`/`parse_stream` node-tree API (`Psych::Nodes::*`) isn't
    // modelled; a clean NotImplementedError rather than a panic.
    def self."parse" arity -2 (_recv, _args, _block) {
        Err(not_impl_error!("Psych.parse (the node-tree API) is not implemented"))
    }
    def self."parse_stream" arity -2 (_recv, _args, _block) {
        Err(not_impl_error!("Psych.parse_stream (the node-tree API) is not implemented"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(string_new(text.to_string()))
    }
    /// `Psych`'s `ruby_module!`-generated functions have mangled Rust idents, so
    /// the tests call them through the registered class-method `lookup`.
    fn f(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::PSYCH_MODULE)
            .expect("Psych is a registered builtin table")
            .class
            .as_ref()
            .expect("Psych has class methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Psych.{name} is defined"))
    }
    fn dumped(v: &RubyValue) -> String {
        match f("dump")(&RubyValue::Nil, std::slice::from_ref(v), None).unwrap() {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn load_maps_and_sequences() {
        let v = f("load")(
            &RubyValue::Nil,
            &[s("a: 1\nb:\n  - 2\n  - 3.5\n  - null\n  - true")],
            None,
        )
        .unwrap();
        let RubyValue::Hash(h) = &v else {
            panic!("expected Hash")
        };
        assert!(matches!(
            crate::collections::hash_get(h, &s("a")),
            RubyValue::Int(1)
        ));
    }

    #[test]
    fn load_scalars() {
        assert!(matches!(
            f("load")(&RubyValue::Nil, &[s("42")], None).unwrap(),
            RubyValue::Int(42)
        ));
        assert!(matches!(
            f("load")(&RubyValue::Nil, &[s("hello")], None).unwrap(),
            RubyValue::Str(_)
        ));
    }

    #[test]
    fn dump_matches_ruby_block_style() {
        assert_eq!(dumped(&RubyValue::Nil), "---\n");
        assert_eq!(dumped(&RubyValue::Int(42)), "--- 42\n");
        let arr = RubyValue::Array(array_new(vec![
            RubyValue::Int(1),
            RubyValue::Int(2),
            s("x"),
        ]));
        assert_eq!(dumped(&arr), "---\n- 1\n- 2\n- x\n");
        // A sequence nested under a key stays at the key's indent (Psych quirk).
        let h = RubyValue::Hash(hash_new(vec![
            (
                s("a"),
                RubyValue::Array(array_new(vec![RubyValue::Int(1), RubyValue::Int(2)])),
            ),
            (
                s("b"),
                RubyValue::Hash(hash_new(vec![(s("c"), RubyValue::Int(3))])),
            ),
        ]));
        assert_eq!(dumped(&h), "---\na:\n- 1\n- 2\nb:\n  c: 3\n");
    }
}

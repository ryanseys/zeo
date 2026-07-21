//! `json` (CRuby's bundled `json` gem). `require "json"` activates the `JSON`
//! module. Parsing goes through `serde_json` (its `Value` tree converts cleanly
//! to `RubyValue`, distinguishing integers from floats); generation is hand-
//! rolled so integers, bignums, floats, and string escaping match ruby 4.0.5's
//! `JSON.generate`/`pretty_generate` output exactly.
//!
//! Semantics oracle-verified against ruby 4.0.5: objects -> `Hash` with String
//! keys (Symbol keys with `symbolize_names: true`), arrays -> `Array`, `null`
//! -> `nil`, integers -> `Integer` (bignums preserved), reals -> `Float`.
//!
//! A parse error raises `JSON::ParserError`, which this gem's RUBY half
//! (`gems/json/lib/json.rb`) defines -- a feature-gated NATIVE class cannot
//! register a constructible exception, so before the Ruby half existed this
//! degraded to `RuntimeError`. Documented divergence: `to_json` on arbitrary
//! objects (the require-time monkeypatch) is not added -- use `JSON.generate`.

use crate::builtins::{arity, builtin_methods};
use crate::collections::{array_new, hash_new};
use crate::dispatch::raise_error;
use crate::{RubyValue, Signal, string_new};

/// `serde_json::Value` -> `RubyValue`. `symbolize` turns object keys into
/// Symbols.
fn to_ruby(v: &serde_json::Value, symbolize: bool) -> RubyValue {
    match v {
        serde_json::Value::Null => RubyValue::Nil,
        serde_json::Value::Bool(b) => RubyValue::Bool(*b),
        serde_json::Value::Number(n) => number_to_ruby(n),
        serde_json::Value::String(s) => RubyValue::Str(string_new(s.clone())),
        serde_json::Value::Array(a) => {
            RubyValue::Array(array_new(a.iter().map(|e| to_ruby(e, symbolize)).collect()))
        }
        serde_json::Value::Object(o) => {
            let pairs = o
                .iter()
                .map(|(k, val)| {
                    let key = if symbolize {
                        RubyValue::Symbol(crate::symbol::Symbol::intern(k))
                    } else {
                        RubyValue::Str(string_new(k.clone()))
                    };
                    (key, to_ruby(val, symbolize))
                })
                .collect();
            RubyValue::Hash(hash_new(pairs))
        }
    }
}

fn number_to_ruby(n: &serde_json::Number) -> RubyValue {
    if let Some(i) = n.as_i64() {
        RubyValue::Int(i)
    } else if let Some(u) = n.as_u64() {
        // A `u64` above `i64::MAX` -- a bignum in Ruby (`int_value` keeps the
        // canonical Int-vs-BigInt invariant).
        crate::builtins::integer::int_value(num_bigint::BigInt::from(u))
    } else {
        RubyValue::Float(n.as_f64().unwrap_or(f64::NAN))
    }
}

/// Whether `symbolize_names: true` was passed as the trailing options Hash.
fn symbolize_opt(opts: Option<&RubyValue>) -> bool {
    let Some(RubyValue::Hash(h)) = opts else {
        return false;
    };
    let key = RubyValue::Symbol(crate::symbol::Symbol::intern("symbolize_names"));
    matches!(crate::collections::hash_get(h, &key), RubyValue::Bool(true))
}

fn escape_into(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Recursively emit `v` as JSON. `indent` is `Some(depth)` for pretty output.
fn generate_into(v: &RubyValue, indent: Option<usize>, out: &mut String) {
    match v {
        RubyValue::Nil => out.push_str("null"),
        RubyValue::Bool(true) => out.push_str("true"),
        RubyValue::Bool(false) => out.push_str("false"),
        // Numerics render via their Ruby `to_s` (Float "3.0", bignums intact).
        RubyValue::Int(_) | RubyValue::Float(_) | RubyValue::BigInt(_) => {
            out.push_str(&v.to_display_string())
        }
        RubyValue::Str(s) => escape_into(&s.lock().to_utf8_lossy(), out),
        RubyValue::Symbol(sym) => escape_into(&sym.name(), out),
        RubyValue::Array(a) => {
            let elems = a.lock();
            emit_seq(elems.len(), indent, out, '[', ']', |i, inner, out| {
                generate_into(&elems[i], inner, out)
            });
        }
        RubyValue::Hash(h) => {
            let pairs = crate::collections::hash_pairs(h);
            // Compact output separates key and value with `":"`, pretty with
            // `": "` (matching CRuby's `generate` vs `pretty_generate`).
            let sep = if indent.is_some() { ": " } else { ":" };
            emit_seq(pairs.len(), indent, out, '{', '}', |i, inner, out| {
                let (k, val) = &pairs[i];
                escape_into(&k.to_display_string(), out);
                out.push_str(sep);
                generate_into(val, inner, out);
            });
        }
        // CRuby's default `Object#to_json` is `to_s.to_json` -- a JSON string.
        other => escape_into(&other.to_display_string(), out),
    }
}

/// Shared array/object emitter: `open`/`close` brackets, `len` items written by
/// `item(i, inner_indent, out)`, honoring pretty-print indentation.
fn emit_seq(
    len: usize,
    indent: Option<usize>,
    out: &mut String,
    open: char,
    close: char,
    mut item: impl FnMut(usize, Option<usize>, &mut String),
) {
    out.push(open);
    if len == 0 {
        out.push(close);
        return;
    }
    let inner = indent.map(|d| d + 1);
    for i in 0..len {
        if i > 0 {
            out.push(',');
        }
        if let Some(depth) = inner {
            out.push('\n');
            out.push_str(&"  ".repeat(depth));
        }
        item(i, inner, out);
    }
    if let Some(depth) = indent {
        out.push('\n');
        out.push_str(&"  ".repeat(depth));
    }
    out.push(close);
}

fn parse_text(v: &RubyValue) -> Result<String, Signal> {
    Ok(crate::builtins::convert::to_rstr(v)?
        .lock()
        .to_utf8_lossy()
        .into_owned())
}

builtin_methods! {
    pub(crate) fn lookup_class;

    "parse" | "load" => fn parse(_recv, args, _block) {
        arity!(args, 1..=2);
        let text = parse_text(&args[0])?;
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| raise_error("JSON::ParserError", format!("{e}")))?;
        Ok(to_ruby(&value, symbolize_opt(args.get(1))))
    }
    "generate" | "dump" => fn generate(_recv, args, _block) {
        arity!(args, 1..=2); // (obj[, opts]) -- opts ignored for compact form
        let mut out = String::new();
        generate_into(&args[0], None, &mut out);
        Ok(RubyValue::Str(string_new(out)))
    }
    "pretty_generate" => fn pretty_generate(_recv, args, _block) {
        arity!(args, 1..=2);
        let mut out = String::new();
        generate_into(&args[0], Some(0), &mut out);
        Ok(RubyValue::Str(string_new(out)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(string_new(text.to_string()))
    }
    fn t(v: Result<RubyValue, Signal>) -> String {
        match v.unwrap() {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn generate_matches_ruby() {
        let inner = RubyValue::Array(array_new(vec![
            RubyValue::Int(2),
            RubyValue::Float(3.5),
            RubyValue::Nil,
            RubyValue::Bool(true),
        ]));
        let h = RubyValue::Hash(hash_new(vec![(s("a"), RubyValue::Int(1)), (s("b"), inner)]));
        assert_eq!(
            t(generate(&RubyValue::Nil, &[h], None)),
            "{\"a\":1,\"b\":[2,3.5,null,true]}"
        );
    }

    #[test]
    fn pretty_generate_matches_ruby() {
        let h = RubyValue::Hash(hash_new(vec![
            (s("a"), RubyValue::Int(1)),
            (
                s("b"),
                RubyValue::Array(array_new(vec![RubyValue::Int(2), RubyValue::Int(3)])),
            ),
        ]));
        assert_eq!(
            t(pretty_generate(&RubyValue::Nil, &[h], None)),
            "{\n  \"a\": 1,\n  \"b\": [\n    2,\n    3\n  ]\n}"
        );
    }

    #[test]
    fn parse_roundtrips_types() {
        let v = parse(
            &RubyValue::Nil,
            &[s(r#"{"a":1,"b":[2,3.5,null,true],"c":"x"}"#)],
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
        assert!(matches!(
            parse(&RubyValue::Nil, &[s("42")], None).unwrap(),
            RubyValue::Int(42)
        ));
        assert!(matches!(
            parse(&RubyValue::Nil, &[s("3.14")], None).unwrap(),
            RubyValue::Float(_)
        ));
    }
}

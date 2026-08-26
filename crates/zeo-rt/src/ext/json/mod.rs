//! `json` (CRuby's bundled `json` gem). `require "json"` activates the `JSON`
//! module.
//!
//! Both halves are zeo's own: [`parser`] is a recursive descent straight into
//! `RubyValue` and [`generator`] carries the option state. serde_json used to
//! serve the parse half and could not express what the gem does -- comments,
//! a trailing comma, `NaN`, `-0` as an Integer, an integer past `u64`,
//! `max_nesting`, `object_class:` construction -- and there was no event
//! layer low enough to fix that from. Owning the parse also means owning
//! CRuby's own error texts, which retires the "decided serde_json
//! substitution" this doc used to record.
//!
//! Semantics probed against ruby 4.0.6 (json 2.21.2): objects -> `Hash` with
//! String keys (Symbols with `symbolize_names: true`), arrays -> `Array`,
//! `null` -> `nil`, an integer literal -> `Integer` exactly however long, a
//! real -> `Float` (or `decimal_class:`).
//!
//! The exception classes live in this gem's RUBY half
//! (`gems/json/lib/json.rb`): a feature-gated NATIVE class cannot register a
//! constructible exception, so `JSONError`, `ParserError`, `NestingError`
//! and `GeneratorError` are Ruby there and raised by name from here.

mod generator;
mod parser;

use crate::{RubyValue, Signal, string_new};
use zeo_macros::ruby_module;

/// Freezes a parsed tree in place, for `JSON.parse(text, freeze: true)`:
/// every String, Array and Hash it built, keys included.
fn freeze_tree(v: &RubyValue) {
    match v {
        RubyValue::Str(s) => s.set_frozen(),
        RubyValue::Array(a) => {
            for e in a.lock().iter() {
                freeze_tree(e);
            }
            a.set_frozen();
        }
        RubyValue::Hash(h) => {
            for (_, (k, val)) in h.lock().iter() {
                freeze_tree(k);
                freeze_tree(val);
            }
            h.set_frozen();
        }
        _ => {}
    }
}

fn opt(opts: Option<&RubyValue>, name: &str) -> Option<RubyValue> {
    let RubyValue::Hash(h) = opts? else {
        return None;
    };
    let key = RubyValue::Symbol(crate::Symbol::intern(name));
    match crate::collections::hash_get(h, &key) {
        RubyValue::Nil => None,
        v => Some(v),
    }
}

fn bool_opt(opts: Option<&RubyValue>, name: &str) -> bool {
    opt(opts, name).is_some_and(|v| v.truthy())
}

fn str_opt(opts: Option<&RubyValue>, name: &str) -> Option<String> {
    match opt(opts, name)? {
        RubyValue::Str(s) => Some(s.lock().to_utf8_lossy().into_owned()),
        _ => None,
    }
}

/// `max_nesting:` -- `false` or `0` means unbounded, absent means the
/// default, and anything else is the limit.
fn nesting_opt(opts: Option<&RubyValue>, default: i64) -> Option<i64> {
    // ABSENT means the default, not unbounded. Reading `opts?` straight
    // through made a bare `JSON.parse(text)` unbounded -- so a 200-deep
    // document parsed where ruby raises, and a self-referential array
    // recursed until the stack ended the process.
    let Some(RubyValue::Hash(h)) = opts else {
        return Some(default);
    };
    let key = RubyValue::Symbol(crate::Symbol::intern("max_nesting"));
    match crate::collections::hash_get(h, &key) {
        RubyValue::Nil => Some(default),
        RubyValue::Bool(false) => None,
        RubyValue::Int(0) => None,
        // SIGNED and taken as written: ruby refuses at depth 1 for
        // `max_nesting: -1`, and truncates a Float toward zero.
        RubyValue::Int(n) => Some(n),
        RubyValue::Float(f) => Some(f as i64),
        _ => Some(default),
    }
}

fn parse_opts(opts: Option<&RubyValue>) -> parser::Opts {
    parser::Opts {
        symbolize: bool_opt(opts, "symbolize_names"),
        freeze: bool_opt(opts, "freeze"),
        allow_nan: bool_opt(opts, "allow_nan"),
        allow_trailing_comma: bool_opt(opts, "allow_trailing_comma"),
        max_nesting: nesting_opt(opts, 100),
        object_class: opt(opts, "object_class"),
        array_class: opt(opts, "array_class"),
        decimal_class: opt(opts, "decimal_class"),
    }
}

fn gen_state(opts: Option<&RubyValue>, base: generator::State) -> generator::State {
    generator::State {
        indent: str_opt(opts, "indent").unwrap_or(base.indent),
        space: str_opt(opts, "space").unwrap_or(base.space),
        space_before: str_opt(opts, "space_before").unwrap_or(base.space_before),
        object_nl: str_opt(opts, "object_nl").unwrap_or(base.object_nl),
        array_nl: str_opt(opts, "array_nl").unwrap_or(base.array_nl),
        allow_nan: bool_opt(opts, "allow_nan") || base.allow_nan,
        ascii_only: bool_opt(opts, "ascii_only") || base.ascii_only,
        script_safe: bool_opt(opts, "script_safe")
            || bool_opt(opts, "escape_slash")
            || base.script_safe,
        max_nesting: nesting_opt(opts, 100),
        ..generator::State::default()
    }
}

/// The options as a HASH, whatever shape they arrived in.
///
/// `obj.to_json(state)` hands a `JSON::State` back to `JSON.generate`, and
/// every option reader here wants a Hash -- so the one conversion lives at
/// the door rather than in each reader.
fn opts_hash(opts: Option<&RubyValue>) -> Option<RubyValue> {
    match opts? {
        h @ RubyValue::Hash(_) => Some(h.clone()),
        other => match crate::dispatch::send_value(
            other,
            crate::Symbol::intern("to_h"),
            &[],
            None,
        ) {
            Ok(h @ RubyValue::Hash(_)) => Some(h),
            _ => None,
        },
    }
}

fn parse_text(v: &RubyValue) -> Result<Vec<u8>, Signal> {
    Ok(crate::builtins::convert::to_rstr(v)?.lock().bytes().to_vec())
}

fn parse_with(text: &RubyValue, opts: Option<&RubyValue>) -> Result<RubyValue, Signal> {
    let bytes = parse_text(text)?;
    let o = parse_opts(opts);
    parser::Parser::new(&bytes, &o).parse_document()
}

fn generate_with(
    v: &RubyValue,
    opts: Option<&RubyValue>,
    base: generator::State,
) -> Result<RubyValue, Signal> {
    let opts = opts_hash(opts);
    let mut st = gen_state(opts.as_ref(), base);
    // A nested `to_json(state)` continues at the depth the state carries,
    // or its indentation would restart at zero inside a `pretty_generate`.
    st.depth = opt(opts.as_ref(), "depth")
        .and_then(|d| match d {
            RubyValue::Int(n) => Some(n),
            _ => None,
        })
        .unwrap_or(0);
    let mut out = String::new();
    generator::generate_into(v, &mut st, &mut out)?;
    Ok(RubyValue::Str(string_new(out)))
}

ruby_module! {
    JSON = zeo_abi::JSON_MODULE;

    // CRuby's json/common.rb declares these `module_function`, so each is both a
    // public `JSON.parse` and a private instance method reachable via `include
    // JSON`. `load`/`dump` are NOT aliases of these -- they have their own
    // argument shapes and live in the Ruby half.
    module_function def "parse" (_recv, arg1, arg2?) {
        parse_with(arg1, arg2)
    }
    // `parse!` is `parse` with `allow_nan:` and no nesting limit.
    module_function def "parse!" (_recv, arg1, arg2?) {
        let bytes = parse_text(arg1)?;
        let mut o = parse_opts(arg2);
        o.allow_nan = true;
        o.max_nesting = nesting_opt(arg2, i64::MAX);
        parser::Parser::new(&bytes, &o).parse_document()
    }
    module_function def "generate" (_recv, obj, opts?) {
        generate_with(obj, opts, generator::State::default())
    }
    // `pretty_generate` is `generate` with the four known options, which any
    // explicit option still overrides.
    module_function def "pretty_generate" (_recv, obj, opts?) {
        generate_with(obj, opts, generator::State::pretty())
    }
    // `JSON[x]` -- parse when `x` is a String (or converts to one via
    // `to_str`), generate otherwise. CRuby spells this in `json/common.rb`;
    // the dispatch is on the ARGUMENT, which is why one name serves both
    // directions. Singleton-only, not `module_function`: CRuby writes this
    // one inside `class << self`, so there is no `JSON#[]` instance half.
    def self."[]" (_recv, object, opts?) {
        let stringy = match object {
            RubyValue::Str(_) => true,
            other => crate::dispatch::responds_to(
                other.class_id(),
                crate::Symbol::intern("to_str"),
                false,
            ),
        };
        match stringy {
            true => parse_with(object, opts),
            false => generate_with(object, opts, generator::State::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::{array_new, hash_new};

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(string_new(text.to_string()))
    }
    fn t(v: Result<RubyValue, Signal>) -> String {
        match v.unwrap() {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => panic!("expected Str, got {other:?}"),
        }
    }
    /// `JSON`'s `ruby_module!`-generated functions have mangled Rust idents, so
    /// the tests call them the way real dispatch does -- through the registered
    /// module-function `lookup`.
    fn f(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::JSON_MODULE)
            .expect("JSON is a registered builtin table")
            .class
            .as_ref()
            .expect("JSON has module functions");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("JSON.{name} is defined"))
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
            t(f("generate")(&RubyValue::Nil, &[h], None)),
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
            t(f("pretty_generate")(&RubyValue::Nil, &[h], None)),
            "{\n  \"a\": 1,\n  \"b\": [\n    2,\n    3\n  ]\n}"
        );
    }

    #[test]
    fn parse_roundtrips_types() {
        let v = f("parse")(
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
            f("parse")(&RubyValue::Nil, &[s("42")], None).unwrap(),
            RubyValue::Int(42)
        ));
        assert!(matches!(
            f("parse")(&RubyValue::Nil, &[s("3.14")], None).unwrap(),
            RubyValue::Float(_)
        ));
    }
}

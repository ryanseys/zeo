//! The `Warning` module: per-category emission flags (`Warning[:cat]` /
//! `Warning[:cat]=`) and the `Warning.warn` sink. Defaults oracle-verified
//! against ruby 4.0.5 under a plain (no `-w`) run: `deprecated` false,
//! `experimental` true, `performance` false. What stdlib needs at load
//! time (ostruct's `HAS_PERFORMANCE_WARNINGS` probe) plus the flag writes.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::Symbol;
use crate::builtins::{arg_error, type_error};
use crate::signal::Signal;
use crate::value::RubyValue;

static DEPRECATED: AtomicBool = AtomicBool::new(false);
static EXPERIMENTAL: AtomicBool = AtomicBool::new(true);
static PERFORMANCE: AtomicBool = AtomicBool::new(false);

/// The category argument as its flag cell -- CRuby accepts a Symbol only
/// (a String is `TypeError`), and an unknown category is `ArgumentError:
/// unknown category: <name>`.
fn category_flag(v: &RubyValue) -> Result<&'static AtomicBool, Signal> {
    let RubyValue::Symbol(s) = v else {
        return Err(type_error!(
            "no implicit conversion of {} into Symbol",
            crate::builtins::class_name_of(v)
        ));
    };
    match s.name().as_str() {
        "deprecated" => Ok(&DEPRECATED),
        "experimental" => Ok(&EXPERIMENTAL),
        "performance" => Ok(&PERFORMANCE),
        other => Err(arg_error!("unknown category: {other}")),
    }
}

fn c_aref(
    _recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1);
    Ok(RubyValue::Bool(
        category_flag(&args[0])?.load(Ordering::Relaxed),
    ))
}

fn c_aset(
    _recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 2);
    let on = args[1].truthy();
    category_flag(&args[0])?.store(on, Ordering::Relaxed);
    Ok(args[1].clone())
}

/// `Warning.warn(msg)` -- writes `msg` to stderr AS-IS (no added newline;
/// `Kernel#warn` is the one that appends). The optional `category:`
/// keyword arrives as a trailing Hash and only gates on its flag.
fn c_warn(
    _recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1..=2);
    if let Some(RubyValue::Hash(h)) = args.get(1) {
        let cat = crate::hash_get(h, &RubyValue::Symbol(Symbol::intern("category")));
        if !matches!(cat, RubyValue::Nil) && !category_flag(&cat)?.load(Ordering::Relaxed) {
            return Ok(RubyValue::Nil);
        }
    }
    let msg = args[0].try_display_string()?;
    eprint!("{msg}");
    Ok(RubyValue::Nil)
}

pub fn lookup_class(name: &str) -> Option<crate::builtins::BuiltinMethodFn> {
    Some(match name {
        "[]" => c_aref,
        "[]=" => c_aset,
        "warn" => c_warn,
        _ => return None,
    })
}

pub fn lookup_class_names() -> &'static [&'static str] {
    &["[]", "[]=", "warn"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_defaults_match_a_plain_cruby_run() {
        let f = |name: &str| {
            c_aref(
                &RubyValue::Nil,
                &[RubyValue::Symbol(Symbol::intern(name))],
                None,
            )
            .unwrap()
            .truthy()
        };
        assert!(!f("deprecated"));
        assert!(f("experimental"));
        assert!(!f("performance"));
    }

    #[test]
    fn setting_a_flag_round_trips_and_answers_the_operand() {
        let cat = RubyValue::Symbol(Symbol::intern("performance"));
        let set = c_aset(&RubyValue::Nil, &[cat.clone(), RubyValue::Bool(true)], None).unwrap();
        assert!(set.truthy());
        assert!(
            c_aref(&RubyValue::Nil, std::slice::from_ref(&cat), None)
                .unwrap()
                .truthy()
        );
        c_aset(&RubyValue::Nil, &[cat, RubyValue::Bool(false)], None).unwrap();
    }
}

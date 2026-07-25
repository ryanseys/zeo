//! `ENV` (CRuby hash.c's env section) -- the process environment.
//!
//! NOT a Hash, despite the interface: `ENV.class` is `Object` in real Ruby
//! (oracle-verified), because ENV is a lone singleton object with
//! Hash-shaped methods, not a Hash instance. Modelling it as a Hash would
//! get `ENV.class` wrong AND snapshot the environment at startup, so
//! `ENV["X"]` would miss a later `ENV["X"] = ...` made through any other
//! path. This reads `std::env` live on every access instead.
//!
//! Its methods hang off the object's own class rather than a table keyed by
//! class: `ENV.class == Object` means a class-keyed table would apply to
//! every plain Object in the program. So `REnv` implements the lookup
//! itself and dispatch probes it by IDENTITY (see `dispatch::send_in`).

use std::sync::{Arc, LazyLock};

use crate::RubyValue;
use crate::builtins::BuiltinMethodFn;
use crate::builtins::type_error;
use crate::dispatch::{RObj, RubyObject};
use zeo_abi::{ClassId, OBJECT_CLASS};

/// The ENV singleton's payload -- stateless: every method reads or writes
/// the real process environment at call time.
pub struct REnv;

impl RubyObject for REnv {
    // `Object`, not a class of its own -- see the module docs.
    fn class_id(&self) -> ClassId {
        OBJECT_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        false
    }
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(REnv)
    }
}

pub fn env_value() -> RubyValue {
    static V: LazyLock<RubyValue> = LazyLock::new(|| RubyValue::Object(Arc::new(REnv)));
    V.clone()
}

/// Whether `v` IS the ENV singleton -- how dispatch decides to consult
/// `lookup` (identity, since ENV shares `Object` with every other plain
/// object; see the module docs).
pub fn is_env(v: &RubyValue) -> bool {
    match v {
        RubyValue::Object(o) => o.as_any().downcast_ref::<REnv>().is_some(),
        _ => false,
    }
}

pub fn seed_env() {
    crate::constants::const_set(0, "ENV", env_value());
}

/// An argument that must be an environment-variable NAME. Real Ruby raises
/// TypeError for a non-String here (`ENV[:PATH]` is a TypeError, not nil).
fn key(v: &RubyValue) -> Result<String, crate::Signal> {
    Ok(crate::builtins::convert::to_rstr(v)?
        .lock()
        .to_utf8_lossy()
        .into_owned())
}

fn str_val(s: String) -> RubyValue {
    RubyValue::Str(crate::collections::string_new(s))
}

/// Every `name => value` pair currently set, sorted by name so `ENV.to_h`/
/// `ENV.each` have a deterministic order. (Real Ruby's order is the OS's
/// `environ` order; nothing may depend on it, and sorted is reproducible.)
fn pairs() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = std::env::vars().collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// A fresh Hash snapshot of the current environment. ENV's read-only
/// Hash/Enumerable surface (`count`, `min`, `value?`, `each_value`, `grep`,
/// `lazy`, `tally`, `assoc`, ...) dispatches through this snapshot -- see the
/// ENV arm in `dispatch::send_value_in` -- so those methods don't each need a
/// hand-written passthrough. Mutators keep their own entries below since they
/// must write the real environment, not a copy.
pub fn snapshot() -> RubyValue {
    RubyValue::Hash(crate::collections::hash_new(
        pairs()
            .into_iter()
            .map(|(k, v)| (str_val(k), str_val(v)))
            .collect(),
    ))
}

/// ENV's method table. Because ENV is dispatched by IDENTITY (its class is
/// `Object`, so a ClassId-keyed `ruby_class!` table would apply to every plain
/// Object -- see the module docs and the `is_env` arm in `dispatch`), this is
/// hand-rolled: the one bespoke builtin method lookup left in the runtime after
/// the `builtin_methods!` macro was retired. Every other class/module is on the
/// `ruby_class!`/`ruby_module!` DSL.
pub(crate) fn lookup(name: &str) -> Option<BuiltinMethodFn> {
    Some(match name {
        "[]" => env_get,
        "[]=" | "store" => env_set,
        "fetch" => env_fetch,
        "key?" | "has_key?" | "include?" | "member?" => env_key_p,
        "assoc" => env_assoc,
        "values_at" => env_values_at,
        "slice" => env_slice,
        "delete" => env_delete,
        "key" => env_key_for_value,
        "keys" => env_keys,
        "values" => env_values,
        "to_h" | "to_hash" => env_to_h,
        "size" | "length" => env_size,
        "empty?" => env_empty_p,
        "each" | "each_pair" => env_each,
        "clear" => env_clear,
        "inspect" => env_inspect,
        "to_s" => env_to_s,
        "dup" => env_dup,
        "clone" => env_clone,
        "freeze" => env_freeze,
        "update" | "merge!" => env_update,
        "delete_if" => env_delete_if,
        "reject!" => env_reject_bang,
        "keep_if" => env_keep_if,
        "select!" | "filter!" => env_select_bang,
        "shift" => env_shift,
        "replace" => env_replace,
        _ => return None,
    })
}

fn env_get(_recv: &RubyValue, args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 1);
    let k = key(&args[0])?;
    Ok(std::env::var(&k).map_or(RubyValue::Nil, str_val))
}

fn env_set(_recv: &RubyValue, args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 2);
    let k = key(&args[0])?;
    match &args[1] {
        // `ENV["X"] = nil` DELETES the variable (real Ruby).
        RubyValue::Nil => {
            // SAFETY: see `remove_var`'s note in `env_delete`.
            unsafe { std::env::remove_var(&k) };
            Ok(RubyValue::Nil)
        }
        v => {
            let s = key(v)?;
            // SAFETY: `set_var` is unsafe since Rust 2024 because it
            // races with concurrent `getenv` in OTHER threads (a libc
            // hazard, not a Rust one). A Ruby program mutating ENV from
            // multiple threads has the same hazard under CRuby; the
            // single-threaded case this serves is sound.
            unsafe { std::env::set_var(&k, &s) };
            Ok(str_val(s))
        }
    }
}

fn env_fetch(_recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 1..=2);
    let k = key(&args[0])?;
    if let Ok(v) = std::env::var(&k) {
        return Ok(str_val(v));
    }
    if let Some(d) = args.get(1) {
        return Ok(d.clone());
    }
    if let Some(RubyValue::Proc(p)) = block {
        return p.call(&[args[0].clone()]);
    }
    Err(crate::dispatch::raise_error_details(
        "KeyError",
        format!("key not found: {}", args[0].inspect_string()),
        &[("key", args[0].clone())],
    ))
}

fn env_key_p(_recv: &RubyValue, args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 1);
    let k = key(&args[0])?;
    Ok(RubyValue::Bool(std::env::var(&k).is_ok()))
}

// These would otherwise reach the Hash snapshot (which silently accepts a
// non-String key); ENV validates the key to a String first, so a Symbol
// raises TypeError -- matching CRuby.
fn env_assoc(_recv: &RubyValue, args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 1);
    let k = key(&args[0])?;
    Ok(std::env::var(&k).map_or(RubyValue::Nil, |v| {
        RubyValue::Array(crate::array_new(vec![str_val(k), str_val(v)]))
    }))
}

fn env_values_at(_recv: &RubyValue, args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    let mut out = Vec::with_capacity(args.len());
    for a in args {
        let k = key(a)?;
        out.push(std::env::var(&k).map_or(RubyValue::Nil, str_val));
    }
    Ok(RubyValue::Array(crate::array_new(out)))
}

fn env_slice(_recv: &RubyValue, args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    let mut pairs = Vec::new();
    for a in args {
        let k = key(a)?;
        if let Ok(v) = std::env::var(&k) {
            pairs.push((str_val(k), str_val(v)));
        }
    }
    Ok(RubyValue::Hash(crate::collections::hash_new(pairs)))
}

fn env_delete(_recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 1);
    let k = key(&args[0])?;
    match std::env::var(&k) {
        Ok(v) => {
            // SAFETY: same libc-level race note as `set_var` above.
            unsafe { std::env::remove_var(&k) };
            Ok(str_val(v))
        }
        // A miss yields to the block if one was given, else nil.
        Err(_) => match block {
            Some(RubyValue::Proc(p)) => p.call(&[args[0].clone()]),
            _ => Ok(RubyValue::Nil),
        },
    }
}

fn env_key_for_value(_recv: &RubyValue, args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 1);
    let want = key(&args[0])?;
    Ok(pairs()
        .into_iter()
        .find(|(_, v)| *v == want)
        .map_or(RubyValue::Nil, |(k, _)| str_val(k)))
}

fn env_keys(_recv: &RubyValue, args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 0);
    Ok(RubyValue::Array(crate::collections::array_new(
        pairs().into_iter().map(|(k, _)| str_val(k)).collect(),
    )))
}

fn env_values(_recv: &RubyValue, args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 0);
    Ok(RubyValue::Array(crate::collections::array_new(
        pairs().into_iter().map(|(_, v)| str_val(v)).collect(),
    )))
}

fn env_to_h(_recv: &RubyValue, args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 0);
    Ok(RubyValue::Hash(crate::collections::hash_new(
        pairs().into_iter().map(|(k, v)| (str_val(k), str_val(v))).collect(),
    )))
}

fn env_size(_recv: &RubyValue, args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 0);
    Ok(RubyValue::Int(pairs().len() as i64))
}

fn env_empty_p(_recv: &RubyValue, args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 0);
    Ok(RubyValue::Bool(pairs().is_empty()))
}

fn env_each(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 0);
    let Some(RubyValue::Proc(p)) = block else {
        return Err(crate::dispatch::raise_no_block_yield());
    };
    for (k, v) in pairs() {
        p.call(&[str_val(k), str_val(v)])?;
    }
    Ok(recv.clone())
}

fn env_clear(recv: &RubyValue, args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 0);
    for (k, _) in pairs() {
        // SAFETY: same libc-level race note as `set_var` above.
        unsafe { std::env::remove_var(&k) };
    }
    Ok(recv.clone())
}

fn env_inspect(_recv: &RubyValue, args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 0);
    // `inspect` renders ENV like a Hash -- reuse the Hash inspect so the
    // shape can't drift from it.
    Ok(str_val(snapshot().inspect_string()))
}

// `ENV.to_s` is the literal `"ENV"` (NOT the Hash rendering `#inspect` gives).
fn env_to_s(_recv: &RubyValue, args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 0);
    Ok(str_val("ENV".to_string()))
}

// ENV is a process-global singleton, so it can be neither copied nor frozen;
// CRuby raises TypeError with these exact messages (overriding Kernel#dup/
// #clone and #freeze, which is why they need explicit entries here rather
// than falling through to the Hash snapshot).
fn env_dup(_recv: &RubyValue, _args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    Err(type_error!("Cannot dup ENV, use ENV.to_h to get a copy of ENV as a hash"))
}

fn env_clone(_recv: &RubyValue, _args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    Err(type_error!("Cannot clone ENV, use ENV.to_h to get a copy of ENV as a hash"))
}

fn env_freeze(_recv: &RubyValue, _args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    Err(type_error!("cannot freeze ENV"))
}

// `ENV.update(hash, ...)` / `ENV.merge!(...)` -- set each name => value into
// the real environment; a block resolves a key already present, taking
// (key, old, new). Answers ENV.
fn env_update(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    for a in args {
        let h = &crate::builtins::convert::to_rhash(a)?;
        for (k, v) in crate::collections::hash_pairs(h) {
            let ks = key(&k)?;
            let vs = match (&block, std::env::var(&ks)) {
                (Some(RubyValue::Proc(p)), Ok(old)) => {
                    key(&p.call(&[str_val(ks.clone()), str_val(old), v.clone()])?)?
                }
                _ => key(&v)?,
            };
            unsafe { std::env::set_var(&ks, &vs) };
        }
    }
    Ok(recv.clone())
}

// `ENV.delete_if { |k, v| }` -- remove every pair the block accepts; answers
// ENV. `reject!` is the same removal but answers nil when NOTHING changed
// (CRuby's bang-method convention).
fn env_delete_if(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 0);
    env_remove_matching(&require_block(block)?, true)?;
    Ok(recv.clone())
}

fn env_reject_bang(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 0);
    let changed = env_remove_matching(&require_block(block)?, true)?;
    Ok(if changed { recv.clone() } else { RubyValue::Nil })
}

// `ENV.keep_if { |k, v| }` -- remove every pair the block REJECTS; answers
// ENV. `select!`/`filter!` answer nil when nothing changed.
fn env_keep_if(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 0);
    env_remove_matching(&require_block(block)?, false)?;
    Ok(recv.clone())
}

fn env_select_bang(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 0);
    let changed = env_remove_matching(&require_block(block)?, false)?;
    Ok(if changed { recv.clone() } else { RubyValue::Nil })
}

// `ENV.shift` -- remove and return the first `[name, value]` pair, nil if
// the environment is empty.
fn env_shift(_recv: &RubyValue, args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 0);
    match pairs().into_iter().next() {
        Some((k, v)) => {
            unsafe { std::env::remove_var(&k) };
            Ok(RubyValue::Array(crate::array_new(vec![str_val(k), str_val(v)])))
        }
        None => Ok(RubyValue::Nil),
    }
}

// `ENV.replace(hash)` -- make the environment exactly `hash`.
fn env_replace(recv: &RubyValue, args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    crate::builtins::arity!(args, 1);
    let h = &crate::builtins::convert::to_rhash(&args[0])?;
    let next = crate::collections::hash_pairs(h);
    for (k, _) in pairs() {
        unsafe { std::env::remove_var(&k) };
    }
    for (k, v) in next {
        let ks = key(&k)?;
        let vs = key(&v)?;
        unsafe { std::env::set_var(&ks, &vs) };
    }
    Ok(recv.clone())
}

/// The Proc a block-taking ENV mutator requires, or CRuby's LocalJumpError.
fn require_block(block: Option<RubyValue>) -> Result<RubyValue, crate::Signal> {
    match block {
        Some(b @ RubyValue::Proc(_)) => Ok(b),
        _ => Err(crate::dispatch::raise_no_block_yield()),
    }
}

/// Remove every environment pair for which `block(key, value)`'s truth equals
/// `remove_when` (`true` for `delete_if`/`reject!`, `false` for `keep_if`/
/// `select!`). Returns whether anything was removed, which the bang variants
/// use to answer nil-on-no-change.
fn env_remove_matching(block: &RubyValue, remove_when: bool) -> Result<bool, crate::Signal> {
    let RubyValue::Proc(p) = block else {
        unreachable!("require_block returns a Proc")
    };
    let mut changed = false;
    for (k, v) in pairs() {
        if p.call(&[str_val(k.clone()), str_val(v)])?.truthy() == remove_when {
            unsafe { std::env::remove_var(&k) };
            changed = true;
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Symbol;

    /// Each test uses its own variable name: `std::env` is process-global and
    /// Rust runs tests in threads, so a shared name would race.
    fn with_var<T>(name: &str, value: &str, f: impl FnOnce() -> T) -> T {
        unsafe { std::env::set_var(name, value) };
        let out = f();
        unsafe { std::env::remove_var(name) };
        out
    }

    fn s(v: &str) -> RubyValue {
        RubyValue::Str(crate::collections::string_new(v.to_string()))
    }

    /// `ENV.class` is Object, not Hash -- oracle-verified. A Hash model would
    /// get this wrong and snapshot the environment besides.
    #[test]
    fn env_is_an_object_not_a_hash() {
        let e = env_value();
        assert_eq!(e.class_id(), OBJECT_CLASS);
        assert!(is_env(&e));
        assert!(!is_env(&s("nope")));
    }

    #[test]
    fn get_reads_the_live_environment() {
        with_var("ZEO_TEST_GET", "yes", || {
            let got = env_get(&env_value(), &[s("ZEO_TEST_GET")], None).unwrap();
            assert_eq!(got.to_display_string(), "yes");
        });
        // Absent after the scope: reads are live, not a startup snapshot.
        let got = env_get(&env_value(), &[s("ZEO_TEST_GET")], None).unwrap();
        assert!(matches!(got, RubyValue::Nil));
    }

    #[test]
    fn set_then_get_round_trips_and_nil_deletes() {
        let e = env_value();
        env_set(&e, &[s("ZEO_TEST_SET"), s("v1")], None).unwrap();
        assert_eq!(
            env_get(&e, &[s("ZEO_TEST_SET")], None)
                .unwrap()
                .to_display_string(),
            "v1"
        );
        // `ENV["X"] = nil` deletes.
        env_set(&e, &[s("ZEO_TEST_SET"), RubyValue::Nil], None).unwrap();
        assert!(matches!(
            env_get(&e, &[s("ZEO_TEST_SET")], None).unwrap(),
            RubyValue::Nil
        ));
    }

    #[test]
    fn fetch_falls_back_to_a_default_then_a_block() {
        let e = env_value();
        let d = env_fetch(&e, &[s("ZEO_TEST_MISSING"), s("dflt")], None).unwrap();
        assert_eq!(d.to_display_string(), "dflt");

        let blk = RubyValue::Proc(crate::RProc::new(|args: &[RubyValue]| {
            Ok(RubyValue::Str(crate::collections::string_new(format!(
                "computed:{}",
                args[0].to_display_string()
            ))))
        }));
        let b = env_fetch(&e, &[s("ZEO_TEST_MISSING")], Some(blk)).unwrap();
        assert_eq!(b.to_display_string(), "computed:ZEO_TEST_MISSING");
    }

    /// A bare `fetch` miss raises KeyError (registry-less: a panic).
    #[test]
    fn fetch_raises_key_error_with_no_default() {
        let r = std::panic::catch_unwind(|| env_fetch(&env_value(), &[s("ZEO_TEST_ABSENT")], None));
        assert!(r.is_err());
    }

    #[test]
    fn key_p_and_delete() {
        let e = env_value();
        env_set(&e, &[s("ZEO_TEST_DEL"), s("x")], None).unwrap();
        assert!(matches!(
            env_key_p(&e, &[s("ZEO_TEST_DEL")], None).unwrap(),
            RubyValue::Bool(true)
        ));
        let old = env_delete(&e, &[s("ZEO_TEST_DEL")], None).unwrap();
        assert_eq!(old.to_display_string(), "x");
        assert!(matches!(
            env_key_p(&e, &[s("ZEO_TEST_DEL")], None).unwrap(),
            RubyValue::Bool(false)
        ));
        // Deleting an absent key is nil, not an error.
        assert!(matches!(
            env_delete(&e, &[s("ZEO_TEST_DEL")], None).unwrap(),
            RubyValue::Nil
        ));
    }

    /// A non-String key is a TypeError, not a silent nil.
    #[test]
    fn a_non_string_key_raises_type_error() {
        let r = std::panic::catch_unwind(|| {
            env_get(
                &env_value(),
                &[RubyValue::Symbol(Symbol::intern("PATH"))],
                None,
            )
        });
        assert!(r.is_err());
    }

    #[test]
    fn to_h_and_keys_see_a_set_variable() {
        with_var("ZEO_TEST_TOH", "1", || {
            let e = env_value();
            let RubyValue::Hash(h) = env_to_h(&e, &[], None).unwrap() else {
                panic!("expected a Hash")
            };
            assert!(!h.lock().is_empty());

            let RubyValue::Array(ks) = env_keys(&e, &[], None).unwrap() else {
                panic!("expected an Array")
            };
            let names: Vec<String> = ks.lock().iter().map(|k| k.to_display_string()).collect();
            assert!(names.contains(&"ZEO_TEST_TOH".to_string()));
            // keys are sorted -- a deterministic order to depend on.
            let mut sorted = names.clone();
            sorted.sort();
            assert_eq!(names, sorted);
        });
    }

    #[test]
    fn each_yields_every_pair() {
        with_var("ZEO_TEST_EACH", "v", || {
            let seen = std::sync::Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
            let sink = seen.clone();
            let blk = RubyValue::Proc(crate::RProc::new(move |args: &[RubyValue]| {
                sink.lock().push(args[0].to_display_string());
                Ok(RubyValue::Nil)
            }));
            env_each(&env_value(), &[], Some(blk)).unwrap();
            assert!(seen.lock().contains(&"ZEO_TEST_EACH".to_string()));
        });
    }

    #[test]
    fn lookup_finds_the_env_names() {
        assert!(lookup("[]").is_some());
        assert!(lookup("fetch").is_some());
        assert!(lookup("to_h").is_some());
        assert!(lookup("nope").is_none());
    }
}

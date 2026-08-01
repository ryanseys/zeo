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
use crate::builtins::type_error;
use crate::dispatch::{RObj, RubyObject};
use zeo_abi::{ClassId, OBJECT_CLASS};
use zeo_macros::ruby_class;

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

/// Whether `o` IS the ENV singleton -- how dispatch decides to consult
/// `lookup` (identity, since ENV shares `Object` with every other plain
/// object; see the module docs). A pointer compare against the singleton's
/// data address, so the dispatch hot path pays neither a `TypeId` downcast
/// nor a boxed `RubyValue`.
pub fn is_env_obj(o: &crate::dispatch::RObj) -> bool {
    static ADDR: LazyLock<usize> = LazyLock::new(|| match env_value() {
        RubyValue::Object(o) => Arc::as_ptr(&o) as *const () as usize,
        _ => 0,
    });
    Arc::as_ptr(o) as *const () as usize == *ADDR
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

// ENV's method table. Because ENV is dispatched by IDENTITY (its class is
// `Object`, so a table keyed by a REAL ClassId would answer for every plain
// Object -- see the module docs and the `is_env` arm in `dispatch`), the
// header names the reserved `ENV_SINGLETON_CLASS` key rather than a class
// anything reports. Everything else is the ordinary DSL: `dispatch` calls the
// `lookup` this block generates, exactly as it called the hand-rolled one.
ruby_class! {
    Env = zeo_abi::ENV_SINGLETON_CLASS < zeo_abi::OBJECT_CLASS;

    def "[]"(_recv, name) {
        Ok(std::env::var(&key(name)?).map_or(RubyValue::Nil, str_val))
    }
    def "[]=" | "store"(_recv, name, value) {
        let k = key(name)?;
        match value {
            // `ENV["X"] = nil` DELETES the variable (real Ruby).
            RubyValue::Nil => {
                // SAFETY: see `remove_var`'s note in `delete`.
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
    def "fetch" cfunc (_recv, name, default?, &block) {
        let k = key(name)?;
        if let Ok(v) = std::env::var(&k) {
            return Ok(str_val(v));
        }
        if let Some(d) = default {
            return Ok(d.clone());
        }
        if let Some(RubyValue::Proc(p)) = block {
            return p.call(std::slice::from_ref(name));
        }
        Err(crate::dispatch::raise_error_details(
            "KeyError",
            format!("key not found: {}", name.inspect_string()),
            &[("key", name.clone())],
        ))
    }
    def "key?" | "has_key?" | "include?" | "member?"(_recv, name) {
        Ok(RubyValue::Bool(std::env::var(&key(name)?).is_ok()))
    }
    // These would otherwise reach the Hash snapshot (which silently accepts a
    // non-String key); ENV validates the key to a String first, so a Symbol
    // raises TypeError -- matching CRuby.
    def "assoc"(_recv, name) {
        let k = key(name)?;
        Ok(std::env::var(&k).map_or(RubyValue::Nil, |v| {
            RubyValue::Array(crate::array_new(vec![str_val(k), str_val(v)]))
        }))
    }
    def "values_at"(_recv, *names) {
        let mut out = Vec::with_capacity(names.len());
        for a in names {
            out.push(std::env::var(&key(a)?).map_or(RubyValue::Nil, str_val));
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    def "slice"(_recv, *names) {
        let mut pairs = Vec::new();
        for a in names {
            let k = key(a)?;
            if let Ok(v) = std::env::var(&k) {
                pairs.push((str_val(k), str_val(v)));
            }
        }
        Ok(RubyValue::Hash(crate::collections::hash_new(pairs)))
    }
    def "delete"(_recv, name, &block) {
        let k = key(name)?;
        match std::env::var(&k) {
            Ok(v) => {
                // SAFETY: same libc-level race note as `[]=` above.
                unsafe { std::env::remove_var(&k) };
                Ok(str_val(v))
            }
            // A miss yields to the block if one was given, else nil.
            Err(_) => match block {
                Some(RubyValue::Proc(p)) => p.call(std::slice::from_ref(name)),
                _ => Ok(RubyValue::Nil),
            },
        }
    }
    def "key"(_recv, value) {
        let want = key(value)?;
        Ok(pairs()
            .into_iter()
            .find(|(_, v)| *v == want)
            .map_or(RubyValue::Nil, |(k, _)| str_val(k)))
    }
    def "keys"(_recv) {
        Ok(RubyValue::Array(crate::collections::array_new(
            pairs().into_iter().map(|(k, _)| str_val(k)).collect(),
        )))
    }
    def "values"(_recv) {
        Ok(RubyValue::Array(crate::collections::array_new(
            pairs().into_iter().map(|(_, v)| str_val(v)).collect(),
        )))
    }
    def "to_h" | "to_hash"(_recv) {
        Ok(snapshot())
    }
    def "size" | "length"(_recv) {
        Ok(RubyValue::Int(pairs().len() as i64))
    }
    def "empty?"(_recv) {
        Ok(RubyValue::Bool(pairs().is_empty()))
    }
    def "each" | "each_pair"(recv, &block) {
        let Some(RubyValue::Proc(p)) = block else {
            return Err(crate::dispatch::raise_no_block_yield());
        };
        for (k, v) in pairs() {
            p.call(&[str_val(k), str_val(v)])?;
        }
        Ok(recv.clone())
    }
    def "clear"(recv) {
        for (k, _) in pairs() {
            // SAFETY: same libc-level race note as `[]=` above.
            unsafe { std::env::remove_var(&k) };
        }
        Ok(recv.clone())
    }
    def "inspect"(_recv) {
        // `inspect` renders ENV like a Hash -- reuse the Hash inspect so the
        // shape can't drift from it.
        Ok(str_val(snapshot().inspect_string()))
    }
    // `ENV.to_s` is the literal `"ENV"` (NOT the Hash rendering `#inspect` gives).
    def "to_s"(_recv) {
        Ok(str_val("ENV".to_string()))
    }
    // ENV is a process-global singleton, so it can be neither copied nor frozen;
    // CRuby raises TypeError with these exact messages (overriding Kernel#dup/
    // #clone and #freeze, which is why they need explicit entries here rather
    // than falling through to the Hash snapshot).
    def "dup"(_recv) {
        Err(type_error!(
            "Cannot dup ENV, use ENV.to_h to get a copy of ENV as a hash"
        ))
    }
    def "clone" cfunc (_recv, **_opts) {
        Err(type_error!(
            "Cannot clone ENV, use ENV.to_h to get a copy of ENV as a hash"
        ))
    }
    def "freeze"(_recv) {
        Err(type_error!("cannot freeze ENV"))
    }
    // `ENV.update(hash, ...)` / `ENV.merge!(...)` -- set each name => value into
    // the real environment; a block resolves a key already present, taking
    // (key, old, new). Answers ENV.
    def "update" | "merge!"(recv, *hashes, &block) {
        for a in hashes {
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
    def "delete_if"(recv, &block) {
        remove_matching(&require_block(block)?, true)?;
        Ok(recv.clone())
    }
    def "reject!"(recv, &block) {
        let changed = remove_matching(&require_block(block)?, true)?;
        Ok(if changed { recv.clone() } else { RubyValue::Nil })
    }
    // `ENV.keep_if { |k, v| }` -- remove every pair the block REJECTS; answers
    // ENV. `select!`/`filter!` answer nil when nothing changed.
    def "keep_if"(recv, &block) {
        remove_matching(&require_block(block)?, false)?;
        Ok(recv.clone())
    }
    def "select!" | "filter!"(recv, &block) {
        let changed = remove_matching(&require_block(block)?, false)?;
        Ok(if changed { recv.clone() } else { RubyValue::Nil })
    }
    // `ENV.shift` -- remove and return the first `[name, value]` pair, nil if
    // the environment is empty.
    def "shift"(_recv) {
        match pairs().into_iter().next() {
            Some((k, v)) => {
                unsafe { std::env::remove_var(&k) };
                Ok(RubyValue::Array(crate::array_new(vec![
                    str_val(k),
                    str_val(v),
                ])))
            }
            None => Ok(RubyValue::Nil),
        }
    }
    // `ENV.replace(hash)` -- make the environment exactly `hash`.
    def "replace"(recv, hash) {
        let h = &crate::builtins::convert::to_rhash(hash)?;
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
fn remove_matching(block: &RubyValue, remove_when: bool) -> Result<bool, crate::Signal> {
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

    /// The rows are `ruby_class!`-generated, so their Rust fn names are
    /// mangled; the tests reach them the way dispatch does, through `lookup`.
    fn m(name: &str) -> crate::builtins::BuiltinMethodFn {
        lookup(name).unwrap_or_else(|| panic!("ENV.{name} is defined"))
    }

    /// `ENV.class` is Object, not Hash -- oracle-verified. A Hash model would
    /// get this wrong and snapshot the environment besides.
    #[test]
    fn env_is_an_object_not_a_hash() {
        let e = env_value();
        assert_eq!(e.class_id(), OBJECT_CLASS);
        let RubyValue::Object(o) = &e else {
            panic!("ENV must be an Object");
        };
        assert!(is_env_obj(o));
        let other: crate::dispatch::RObj = std::sync::Arc::new(crate::dispatch::Object::default());
        assert!(!is_env_obj(&other));
    }

    #[test]
    fn get_reads_the_live_environment() {
        with_var("ZEO_TEST_GET", "yes", || {
            let got = m("[]")(&env_value(), &[s("ZEO_TEST_GET")], None).unwrap();
            assert_eq!(got.to_display_string(), "yes");
        });
        // Absent after the scope: reads are live, not a startup snapshot.
        let got = m("[]")(&env_value(), &[s("ZEO_TEST_GET")], None).unwrap();
        assert!(matches!(got, RubyValue::Nil));
    }

    #[test]
    fn set_then_get_round_trips_and_nil_deletes() {
        let e = env_value();
        m("[]=")(&e, &[s("ZEO_TEST_SET"), s("v1")], None).unwrap();
        assert_eq!(
            m("[]")(&e, &[s("ZEO_TEST_SET")], None)
                .unwrap()
                .to_display_string(),
            "v1"
        );
        // `ENV["X"] = nil` deletes.
        m("[]=")(&e, &[s("ZEO_TEST_SET"), RubyValue::Nil], None).unwrap();
        assert!(matches!(
            m("[]")(&e, &[s("ZEO_TEST_SET")], None).unwrap(),
            RubyValue::Nil
        ));
    }

    #[test]
    fn fetch_falls_back_to_a_default_then_a_block() {
        let e = env_value();
        let d = m("fetch")(&e, &[s("ZEO_TEST_MISSING"), s("dflt")], None).unwrap();
        assert_eq!(d.to_display_string(), "dflt");

        let blk = RubyValue::Proc(crate::RProc::new(|args: &[RubyValue]| {
            Ok(RubyValue::Str(crate::collections::string_new(format!(
                "computed:{}",
                args[0].to_display_string()
            ))))
        }));
        let b = m("fetch")(&e, &[s("ZEO_TEST_MISSING")], Some(blk)).unwrap();
        assert_eq!(b.to_display_string(), "computed:ZEO_TEST_MISSING");
    }

    /// A bare `fetch` miss raises KeyError (registry-less: a panic).
    #[test]
    fn fetch_raises_key_error_with_no_default() {
        let r =
            std::panic::catch_unwind(|| m("fetch")(&env_value(), &[s("ZEO_TEST_ABSENT")], None));
        assert!(r.is_err());
    }

    #[test]
    fn key_p_and_delete() {
        let e = env_value();
        m("[]=")(&e, &[s("ZEO_TEST_DEL"), s("x")], None).unwrap();
        assert!(matches!(
            m("key?")(&e, &[s("ZEO_TEST_DEL")], None).unwrap(),
            RubyValue::Bool(true)
        ));
        let old = m("delete")(&e, &[s("ZEO_TEST_DEL")], None).unwrap();
        assert_eq!(old.to_display_string(), "x");
        assert!(matches!(
            m("key?")(&e, &[s("ZEO_TEST_DEL")], None).unwrap(),
            RubyValue::Bool(false)
        ));
        // Deleting an absent key is nil, not an error.
        assert!(matches!(
            m("delete")(&e, &[s("ZEO_TEST_DEL")], None).unwrap(),
            RubyValue::Nil
        ));
    }

    /// A non-String key is a TypeError, not a silent nil.
    #[test]
    fn a_non_string_key_raises_type_error() {
        let r = std::panic::catch_unwind(|| {
            m("[]")(
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
            let RubyValue::Hash(h) = m("to_h")(&e, &[], None).unwrap() else {
                panic!("expected a Hash")
            };
            assert!(!h.lock().is_empty());

            let RubyValue::Array(ks) = m("keys")(&e, &[], None).unwrap() else {
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
            m("each")(&env_value(), &[], Some(blk)).unwrap();
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

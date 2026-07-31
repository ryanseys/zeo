//! The `Warning` module: per-category emission flags (`Warning[:cat]` /
//! `Warning[:cat]=`) and the `Warning.warn` sink. Defaults oracle-verified
//! against ruby 4.0.6 under a plain (no `-w`) run: `deprecated` false,
//! `experimental` true, `performance` false. What stdlib needs at load
//! time (ostruct's `HAS_PERFORMANCE_WARNINGS` probe) plus the flag writes.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::Symbol;
use crate::builtins::{arg_error, type_error};
use crate::signal::Signal;
use crate::value::RubyValue;
use zeo_macros::ruby_module;

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

ruby_module! {
    Warning = zeo_abi::WARNING_MODULE;

    def self."[]"(_recv, arg) {
        Ok(RubyValue::Bool(category_flag(arg)?.load(Ordering::Relaxed)))
    }
    def self."[]="(_recv, arg1, arg2) {
        let on = (*arg2).truthy();
        category_flag(arg1)?.store(on, Ordering::Relaxed);
        Ok((*arg2).clone())
    }
    // `Warning.warn(msg)` -- writes `msg` to stderr AS-IS (no added newline;
    // `Kernel#warn` is the one that appends). The optional `category:`
    // keyword arrives as a trailing Hash and only gates on its flag.
    def self."warn" cfunc (_recv, arg1, arg2?) {
        if let Some(RubyValue::Hash(h)) = arg2 {
            let cat = crate::hash_get(h, &RubyValue::Symbol(Symbol::intern("category")));
            if !matches!(cat, RubyValue::Nil) && !category_flag(&cat)?.load(Ordering::Relaxed) {
                return Ok(RubyValue::Nil);
            }
        }
        let msg = (*arg1).try_display_string()?;
        eprint!("{msg}");
        Ok(RubyValue::Nil)
    }
}

/// The `Ractor API is experimental` notice, at the FIRST `Ractor.new` and
/// never again -- CRuby's own once-per-process `rb_warn` with the caller's
/// `file:line`, and gated on the same `Warning[:experimental]` flag, so a
/// program that turns the category off before its first Ractor sees nothing.
pub(crate) fn warn_ractor_experimental() {
    static WARNED: AtomicBool = AtomicBool::new(false);
    if WARNED.swap(true, Ordering::Relaxed) || !EXPERIMENTAL.load(Ordering::Relaxed) {
        return;
    }
    let Some((file, line)) = crate::frames::current_location() else {
        return;
    };
    eprintln!(
        "{file}:{line}: warning: Ractor API is experimental and may change in \
         future versions of Ruby."
    );
}

/// Ruby's PARSE-time warnings, which CRuby prints before the program's first
/// line runs. zeo parses at COMPILE time, so the compiler collects them and
/// generated `main()` replays them here -- ahead of `run_main`, which is the
/// same position relative to any program output.
///
/// Straight to stderr, not through `Warning.warn`: in CRuby these are already
/// out before the program could install an override.
pub fn emit_parse_warnings(lines: &[&str]) {
    for line in lines {
        eprintln!("{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `ruby_module!`-generated class methods are reachable only through
    /// the dispatch table (their Rust fn names are mangled), so the tests call
    /// them the way real dispatch does -- through `Warning`'s registered
    /// class-method `lookup`.
    fn cmethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::WARNING_MODULE)
            .expect("Warning is a registered builtin table")
            .class
            .as_ref()
            .expect("Warning has class methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Warning.{name} is defined"))
    }

    // Defaults and the flag round-trip share process-global category statics,
    // so they live in ONE test -- as two parallel tests they'd race on
    // `PERFORMANCE` (one toggling it while the other reads the default).
    #[test]
    fn category_defaults_and_flag_writes() {
        let aref = |name: &str| {
            cmethod("[]")(
                &RubyValue::Nil,
                &[RubyValue::Symbol(Symbol::intern(name))],
                None,
            )
            .unwrap()
            .truthy()
        };

        // Defaults match a plain (no `-w`) ruby 4.0.6 run.
        assert!(!aref("deprecated"));
        assert!(aref("experimental"));
        assert!(!aref("performance"));

        // Setting a flag round-trips and answers the assigned operand.
        let cat = RubyValue::Symbol(Symbol::intern("performance"));
        let set =
            cmethod("[]=")(&RubyValue::Nil, &[cat.clone(), RubyValue::Bool(true)], None).unwrap();
        assert!(set.truthy());
        assert!(aref("performance"));
        cmethod("[]=")(&RubyValue::Nil, &[cat, RubyValue::Bool(false)], None).unwrap();
        assert!(!aref("performance"));
    }
}

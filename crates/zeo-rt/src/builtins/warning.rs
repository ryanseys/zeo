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

/// Is a category on? The C API's `rb_category_warn` asks by name, because
/// `rb_warning_category_t` is an enum over the same four categories.
#[cfg(feature = "cext")]
pub(crate) fn category_enabled(name: &str) -> bool {
    match name {
        "deprecated" => DEPRECATED.load(Ordering::Relaxed),
        "experimental" => EXPERIMENTAL.load(Ordering::Relaxed),
        "performance" => PERFORMANCE.load(Ordering::Relaxed),
        // `strict_unused_block` has no cell, and CRuby leaves it off under a
        // plain run. An unknown name is not a category and warns anyway.
        "strict_unused_block" => false,
        _ => true,
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
    // `Warning.categories` -- every category `Warning[]` accepts, in CRuby's
    // own order.
    def self."categories"(_recv) {
        let names = ["deprecated", "experimental", "performance", "strict_unused_block"]
            .into_iter()
            .map(|n| RubyValue::Symbol(Symbol::intern(n)))
            .collect();
        Ok(RubyValue::Array(crate::array_new(names)))
    }
    // `Warning#warn` is a PUBLIC instance method in CRuby, not a
    // `module_function`: warning.c defines it on the module and then extends
    // the module with itself, which is what makes `Warning.warn` resolve while
    // `Warning.instance_methods(false)` still lists it. A program overrides it
    // with `module Warning; def warn(msg, category: nil); ...; end`.
    def "warn" cfunc (_recv, arg1, arg2?) {
        warn_impl(arg1, arg2)
    }
    def self."warn" cfunc (_recv, arg1, arg2?) {
        warn_impl(arg1, arg2)
    }
}

/// The shared body of `Warning#warn` and `Warning.warn`: write `msg` to stderr
/// AS-IS (no added newline; `Kernel#warn` is the one that appends). The
/// optional `category:` keyword arrives as a trailing Hash and only gates on
/// its flag.
fn warn_impl(arg1: &RubyValue, arg2: Option<&RubyValue>) -> Result<RubyValue, Signal> {
    {
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

/// CRuby's `rb_warn`: the CALLER's `file:line`, the `warning: ` prefix and a
/// trailing newline. Silent only when `$VERBOSE` is `nil` (`-W0`) -- unlike
/// `rb_warning`, which additionally needs `$VERBOSE` true.
///
/// Through the Ruby-level `$stderr` (CRuby's `rb_warn` writes to `rb_stderr`),
/// so `$stderr.reopen(IO::NULL)` silences it -- the standard trick for keeping
/// a nondeterministic `file:line` out of a fixture's stderr. A raising
/// redirected writer must not turn a warning into an exception, so the write
/// result is dropped.
pub(crate) fn rb_warn(msg: &str) {
    if matches!(crate::globals::global_get(0, "$VERBOSE"), RubyValue::Nil) {
        return;
    }
    let Some((file, line)) = crate::frames::current_location() else {
        return;
    };
    let line = format!("{file}:{line}: warning: {msg}\n");
    let _ =
        crate::builtins::io::write_bytes(&crate::builtins::io::current_stderr(), line.as_bytes());
}

/// One category-gated notice, at its FIRST reach and never again -- CRuby's
/// own once-per-process `rb_category_warn` shape. `warned` is the caller's
/// own cell, so each notice counts separately.
fn warn_experimental_once(warned: &AtomicBool, msg: &str) {
    if warned.swap(true, Ordering::Relaxed) || !EXPERIMENTAL.load(Ordering::Relaxed) {
        return;
    }
    rb_warn(msg);
}

/// The `Ractor API is experimental` notice, at the FIRST `Ractor.new` and
/// never again. Gated on `Warning[:experimental]`, so a program that turns the
/// category off before its first Ractor sees nothing.
pub(crate) fn warn_ractor_experimental() {
    static WARNED: AtomicBool = AtomicBool::new(false);
    warn_experimental_once(
        &WARNED,
        "Ractor API is experimental and may change in future versions of Ruby.",
    );
}

/// The same notice for `IO::Buffer`, which CRuby raises from its ALLOCATOR
/// (`rb_io_buffer_type_allocate`) -- so every constructor spelling reaches it,
/// `IO::Buffer.new`, `.for`, `.string` and `.map` alike.
pub(crate) fn warn_io_buffer_experimental() {
    static WARNED: AtomicBool = AtomicBool::new(false);
    warn_experimental_once(
        &WARNED,
        "IO::Buffer is experimental and both the Ruby and C interface may change in the future!",
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

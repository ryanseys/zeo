//! `GC` (CRuby gc.c) -- honest no-ops. zeo is `Arc`-refcounted with no
//! collector to drive, so there is nothing for `GC.start` to start. The
//! methods exist because real programs call them incidentally (a benchmark
//! harness, a test's `GC.start` between cases) and raising NoMethodError
//! there would be a worse lie than doing nothing.
//!
//! Cycles genuinely leak; that is documented in the plan as an accepted
//! trade, not something these rows paper over. `GC.stat` answers an empty
//! Hash so `GC.stat[:count]`-style reads get `nil` rather than crashing --
//! no fabricated statistics.

use crate::RubyValue;
use zeo_macros::ruby_module;

ruby_module! {
    GC = zeo_abi::GC_CLASS;

    def self."start" | "compact"(_recv, _args, _block) {
        // No tracing collector to drive, but this is the honest moment to run
        // finalizers for objects whose last strong reference has dropped.
        crate::builtins::weak::run_finalizers_for_dead();
        Ok(RubyValue::Nil)
    }
    // Real Ruby answers the PREVIOUS enabled state. Always-false is
    // truthful here: the collector is never enabled, because there isn't one.
    def self."enable" | "disable"(_recv, _args, _block) {
        Ok(RubyValue::Bool(false))
    }
    def self."stress"(_recv, _args, _block) {
        Ok(RubyValue::Bool(false))
    }
    def self."count"(_recv, _args, _block) {
        Ok(RubyValue::Int(0))
    }
    def self."stat"(_recv, _args, _block) {
        Ok(RubyValue::Hash(crate::collections::hash_new(Vec::new())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `ruby_module!`-generated class methods are reachable only through
    /// the dispatch table (their Rust fn names are mangled), so the tests call
    /// them the way real dispatch does -- through `GC`'s registered
    /// class-method `lookup`.
    fn cmethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::GC_CLASS)
            .expect("GC is a registered builtin table")
            .class
            .as_ref()
            .expect("GC has class methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("GC.{name} is defined"))
    }

    #[test]
    fn gc_rows_are_no_ops_that_answer_ruby_shapes() {
        let cls = RubyValue::Class(zeo_abi::GC_CLASS);
        assert!(matches!(cmethod("start")(&cls, &[], None).unwrap(), RubyValue::Nil));
        assert!(matches!(
            cmethod("count")(&cls, &[], None).unwrap(),
            RubyValue::Int(0)
        ));
        assert!(matches!(
            cmethod("enable")(&cls, &[], None).unwrap(),
            RubyValue::Bool(false)
        ));
    }

    /// `GC.stat` is an empty Hash, not a fabricated one -- a miss reads nil.
    #[test]
    fn gc_stat_is_an_empty_hash() {
        let cls = RubyValue::Class(zeo_abi::GC_CLASS);
        let RubyValue::Hash(h) = cmethod("stat")(&cls, &[], None).unwrap() else {
            panic!("expected a Hash")
        };
        assert_eq!(h.lock().len(), 0);
    }

    #[test]
    fn lookup_finds_the_gc_names() {
        let tbl = crate::builtins::registered_table(zeo_abi::GC_CLASS)
            .expect("GC registered")
            .class
            .as_ref()
            .expect("GC has class methods");
        assert!((tbl.lookup)("start").is_some());
        assert!((tbl.lookup)("stat").is_some());
        assert!((tbl.lookup)("compact").is_some());
        assert!((tbl.lookup)("nope").is_none());
    }
}

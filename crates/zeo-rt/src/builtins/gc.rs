//! `GC` (CRuby gc.c) -- honest no-ops. zeo-rs is `Arc`-refcounted with no
//! collector to drive, so there is nothing for `GC.start` to start. The
//! methods exist because real programs call them incidentally (a benchmark
//! harness, a test's `GC.start` between cases) and raising NoMethodError
//! there would be a worse lie than doing nothing.
//!
//! Cycles genuinely leak; that is documented in the plan as an accepted
//! trade, not something these rows paper over. `GC.stat` answers an empty
//! Hash so `GC.stat[:count]`-style reads get `nil` rather than crashing --
//! no fabricated statistics.

use crate::builtins::builtin_methods;
use crate::RubyValue;

builtin_methods! {
    pub(crate) fn lookup_class;

    "start" | "compact" => fn gc_start(_recv, _args, _block) {
        Ok(RubyValue::Nil)
    }
    // Real Ruby answers the PREVIOUS enabled state. Always-false is
    // truthful here: the collector is never enabled, because there isn't one.
    "enable" | "disable" => fn gc_enable(_recv, _args, _block) {
        Ok(RubyValue::Bool(false))
    }
    "stress" => fn gc_stress(_recv, _args, _block) {
        Ok(RubyValue::Bool(false))
    }
    "count" => fn gc_count(_recv, _args, _block) {
        Ok(RubyValue::Int(0))
    }
    "stat" => fn gc_stat(_recv, _args, _block) {
        Ok(RubyValue::Hash(crate::collections::hash_new(Vec::new())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gc_rows_are_no_ops_that_answer_ruby_shapes() {
        let cls = RubyValue::Class(zeo_abi::GC_CLASS);
        assert!(matches!(gc_start(&cls, &[], None).unwrap(), RubyValue::Nil));
        assert!(matches!(gc_count(&cls, &[], None).unwrap(), RubyValue::Int(0)));
        assert!(matches!(
            gc_enable(&cls, &[], None).unwrap(),
            RubyValue::Bool(false)
        ));
    }

    /// `GC.stat` is an empty Hash, not a fabricated one -- a miss reads nil.
    #[test]
    fn gc_stat_is_an_empty_hash() {
        let cls = RubyValue::Class(zeo_abi::GC_CLASS);
        let RubyValue::Hash(h) = gc_stat(&cls, &[], None).unwrap() else {
            panic!("expected a Hash")
        };
        assert_eq!(h.lock().len(), 0);
    }

    #[test]
    fn lookup_finds_the_gc_names() {
        assert!(lookup_class("start").is_some());
        assert!(lookup_class("stat").is_some());
        assert!(lookup_class("nope").is_none());
    }
}

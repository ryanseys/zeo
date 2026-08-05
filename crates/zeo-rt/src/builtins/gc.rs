//! `GC` (CRuby gc.c) -- honest no-ops. zeo is `Arc`-refcounted with no
//! collector to drive, so there is nothing for `GC.start` to start. The
//! methods exist because real programs call them incidentally (a benchmark
//! harness, a test's `GC.start` between cases) and raising NoMethodError
//! there would be a worse lie than doing nothing.
//!
//! Cycles genuinely leak; that is documented in the plan as an accepted
//! trade, not something these rows paper over. `GC.stat` carries exactly one
//! real number -- `:count`, the explicit collections run -- and answers
//! `nil` for every statistic this heap has no truthful value for. No
//! fabricated statistics.

use crate::{RubyValue, Signal};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use zeo_macros::ruby_module;

/// The two switches a program may set and read back. Nothing consults them --
/// there is no collector to measure or compact -- but a setter that silently
/// discarded its argument would be a worse lie than one that remembers.
static MEASURE_TOTAL_TIME: AtomicBool = AtomicBool::new(true);
static AUTO_COMPACT: AtomicBool = AtomicBool::new(false);
static STRESS: AtomicBool = AtomicBool::new(false);
/// `GC::Profiler`'s switch. Nothing reads it but `enabled?` -- there is no
/// tracing collector here to time -- but a program that turns profiling on
/// and asks must be told what it asked for.
static PROFILING: AtomicBool = AtomicBool::new(false);
/// Collections run -- each explicit `GC.start`/`garbage_collect` counts one
/// (in a refcounting heap each is trivially a full collection). What
/// `GC.count` and `GC.stat(:count)` answer.
static COUNT: AtomicU64 = AtomicU64::new(0);

fn hash_of(pairs: Vec<(&str, RubyValue)>) -> RubyValue {
    RubyValue::Hash(crate::collections::hash_new(
        pairs
            .into_iter()
            .map(|(k, v)| (RubyValue::Symbol(crate::Symbol::intern(k)), v))
            .collect(),
    ))
}

fn empty_hash() -> RubyValue {
    RubyValue::Hash(crate::collections::hash_new(Vec::new()))
}

/// The `considered`/`moved`/`moved_up`/`moved_down` shape both compaction
/// queries answer. Every bucket is empty, because nothing ever moves.
fn compact_info() -> RubyValue {
    hash_of(vec![
        ("considered", empty_hash()),
        ("moved", empty_hash()),
        ("moved_up", empty_hash()),
        ("moved_down", empty_hash()),
    ])
}

fn flag(cell: &AtomicBool, arg: &RubyValue) -> Result<RubyValue, Signal> {
    cell.store(arg.truthy(), Ordering::Relaxed);
    Ok(arg.clone())
}

ruby_module! {
    GC = zeo_abi::GC_CLASS;

    // zeo's heap is refcounted with no tracing collector, so `OPTS` names no
    // build options and `INTERNAL_CONSTANTS` describes no slot layout. Empty
    // is the truthful answer to both; CRuby's values describe its own heap.
    const OPTS = RubyValue::Array(crate::array_new(Vec::new()));
    const INTERNAL_CONSTANTS = empty_hash();

    def self."start" | "compact" arity 0 (_recv, *_args, &_block) {
        // No tracing collector to drive, but this is the honest moment to run
        // finalizers for objects whose last strong reference has dropped.
        COUNT.fetch_add(1, Ordering::Relaxed);
        crate::builtins::weak::run_finalizers_for_dead();
        Ok(RubyValue::Nil)
    }
    // `GC#garbage_collect` -- the instance twin of `GC.start`, which a class
    // gets by `include GC`. Public, as CRuby lists it.
    def "garbage_collect" (_recv, *_args, &_block) {
        COUNT.fetch_add(1, Ordering::Relaxed);
        crate::builtins::weak::run_finalizers_for_dead();
        Ok(RubyValue::Nil)
    }
    // Real Ruby answers the PREVIOUS enabled state. Always-false is
    // truthful here: the collector is never enabled, because there isn't one.
    def self."enable" | "disable"(_recv) {
        Ok(RubyValue::Bool(false))
    }
    def self."stress"(_recv) {
        Ok(RubyValue::Bool(STRESS.load(Ordering::Relaxed)))
    }
    def self."stress="(_recv, on) {
        flag(&STRESS, on)
    }
    // The number of collections RUN: every explicit `GC.start`/
    // `garbage_collect` is one (in a refcounting heap each is trivially a
    // full collection), so `GC.start; GC.count > 0` holds as in CRuby.
    def self."count"(_recv) {
        Ok(RubyValue::Int(COUNT.load(Ordering::Relaxed) as i64))
    }
    // `stat()` -> the whole hash, `stat(:key)` -> one value (nil for a key
    // this heap has no truthful number for), `stat(hash)` -> fills and
    // answers the hash. Only `:count` carries a real value.
    def self."stat"(_recv, *_args, &_block) {
        let count = RubyValue::Int(COUNT.load(Ordering::Relaxed) as i64);
        match _args.first() {
            Some(RubyValue::Symbol(s)) => Ok(if s.name() == "count" {
                count
            } else {
                RubyValue::Nil
            }),
            Some(RubyValue::Hash(h)) => {
                crate::collections::hash_set(
                    h,
                    RubyValue::Symbol(crate::Symbol::intern("count")),
                    count,
                );
                Ok(RubyValue::Hash(h.clone()))
            }
            _ => Ok(hash_of(vec![("count", count)])),
        }
    }
    // One size-pooled heap per slot size is an MRI structure; there are no
    // heaps here to describe, whether asked for all of them or for one.
    def self."stat_heap" cfunc (_recv, *_args, &_block) {
        Ok(empty_hash())
    }
    // `GC.config` reports the collector's own settings. zeo's answer names the
    // implementation truthfully rather than echoing MRI's `"default"`.
    def self."config" cfunc (_recv, *_args, &_block) {
        Ok(hash_of(vec![(
            "implementation",
            RubyValue::Str(crate::string_new("refcount".to_string())),
        )]))
    }
    // No collection has ever run, so every measurement is zero and every
    // "what did the last GC do" query describes nothing having happened --
    // which is the same shape CRuby answers in a process that has not yet
    // collected.
    def self."total_time"(_recv) {
        Ok(RubyValue::Int(0))
    }
    def self."measure_total_time"(_recv) {
        Ok(RubyValue::Bool(MEASURE_TOTAL_TIME.load(Ordering::Relaxed)))
    }
    def self."measure_total_time="(_recv, on) {
        flag(&MEASURE_TOTAL_TIME, on)
    }
    def self."latest_gc_info" cfunc (_recv, *_args, &_block) {
        Ok(hash_of(vec![
            ("major_by", RubyValue::Nil),
            ("need_major_by", RubyValue::Nil),
            ("gc_by", RubyValue::Nil),
            ("have_finalizer", RubyValue::Bool(false)),
            ("immediate_sweep", RubyValue::Bool(false)),
            ("state", RubyValue::Symbol(crate::Symbol::intern("none"))),
            ("weak_references_count", RubyValue::Int(0)),
            ("retained_weak_references_count", RubyValue::Int(0)),
        ]))
    }
    def self."latest_compact_info"(_recv) {
        Ok(compact_info())
    }
    def self."verify_compaction_references" cfunc (_recv, *_args, &_block) {
        Ok(compact_info())
    }
    def self."verify_internal_consistency"(_recv) {
        Ok(RubyValue::Nil)
    }
    def self."auto_compact"(_recv) {
        Ok(RubyValue::Bool(AUTO_COMPACT.load(Ordering::Relaxed)))
    }
    def self."auto_compact="(_recv, on) {
        flag(&AUTO_COMPACT, on)
    }

    // `GC::Profiler` -- the switch is real (it remembers what it was told,
    // and `enabled?` reads it back), and the readouts are honestly empty:
    // there is no tracing collector here to time, so no run has ever been
    // recorded. That is what CRuby answers before its first collection too.
    module Profiler = zeo_abi::GC_PROFILER_MODULE {
        def self."enable"(_recv) {
            PROFILING.store(true, Ordering::Relaxed);
            Ok(RubyValue::Nil)
        }
        def self."disable"(_recv) {
            PROFILING.store(false, Ordering::Relaxed);
            Ok(RubyValue::Nil)
        }
        def self."enabled?"(_recv) {
            Ok(RubyValue::Bool(PROFILING.load(Ordering::Relaxed)))
        }
        def self."clear"(_recv) {
            Ok(RubyValue::Nil)
        }
        def self."total_time"(_recv) {
            Ok(RubyValue::Float(0.0))
        }
        def self."result"(_recv) {
            Ok(RubyValue::Str(crate::string_new(String::new())))
        }
        // `report` PRINTS `result` and answers nil, so an empty result
        // prints nothing.
        def self."report"(_recv, *_args, &_block) {
            Ok(RubyValue::Nil)
        }
        def self."raw_data"(_recv) {
            Ok(RubyValue::Nil)
        }
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
        assert!(matches!(
            cmethod("start")(&cls, &[], None).unwrap(),
            RubyValue::Nil
        ));
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

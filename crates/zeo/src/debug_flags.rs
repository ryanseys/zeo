//! One dial for the compiler's debug escape hatches and verification nets:
//! `ZEO_DEBUG=<flag>[,<flag>...]`, parsed once per process.
//!
//! This replaced seven single-purpose variables with three different
//! conventions among them -- one spelling, discoverable in one place. An
//! unknown flag warns rather than erroring: a debug dial must never make a
//! production compile fail, but a typo silently doing nothing already cost
//! real debugging time elsewhere. Flags whose subsystems died with the
//! rustc backend were deleted with it.

use std::sync::OnceLock;

#[derive(Clone, Copy)]
pub(crate) enum DebugFlag {
    /// `runtime-struct`: turn off compile-time `Struct.new` lowering.
    RuntimeStruct,
    /// `strict-ambiguous-require`: hard error when a feature resolves in
    /// more than one gem, instead of first-wins with a warning.
    StrictAmbiguousRequire,
    /// `verify-class-index`: shadow-compare every `class_in_scope` answer
    /// against the linear scan it replaced.
    VerifyClassIndex,
    /// `no-typed-calls`: turn off every TyKind-driven emission (typed
    /// direct calls, unboxed locals) -- the differential-oracle kill
    /// switch. Reserved ahead of the folds so the oracle leg is proven
    /// green BEFORE the first fold lands; a wrong static type is a
    /// miscompile, and this flag is what the on-vs-off diff toggles.
    NoTypedCalls,
    /// `no-package-sweep`: refuse the package-wide unit demand a computed
    /// `require` falls back on. Speculative compilation is the largest
    /// single input to a binary's size -- one `require ENV["X"]` compiles
    /// its whole package -- and this is the only way to price it: compile
    /// once with and once without, and diff `--dump=units`. The program it
    /// produces may raise `LoadError` where the swept one would not, so it
    /// measures rather than ships.
    NoPackageSweep,
    /// `outline-frames`: emit the CALL form of every frame push, pop and
    /// pool mark instead of the inline one. The inline protocol is the
    /// largest per-method cost in an emitted body -- a `def m; 1; end`
    /// whose work is two instructions -- and this prices it: compile once
    /// with and once without, and diff the binary and the bench.
    OutlineFrames,
}

const NAMES: &[(&str, DebugFlag)] = &[
    ("runtime-struct", DebugFlag::RuntimeStruct),
    (
        "strict-ambiguous-require",
        DebugFlag::StrictAmbiguousRequire,
    ),
    ("verify-class-index", DebugFlag::VerifyClassIndex),
    ("no-typed-calls", DebugFlag::NoTypedCalls),
    ("no-package-sweep", DebugFlag::NoPackageSweep),
    ("outline-frames", DebugFlag::OutlineFrames),
];

/// Whether `flag` was named in `ZEO_DEBUG`.
pub(crate) fn debug(flag: DebugFlag) -> bool {
    static BITS: OnceLock<u32> = OnceLock::new();
    let bits = *BITS.get_or_init(|| {
        let Ok(raw) = std::env::var("ZEO_DEBUG") else {
            return 0;
        };
        let mut bits = 0u32;
        for tok in raw.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            match NAMES.iter().find(|(name, _)| *name == tok) {
                Some((_, f)) => bits |= 1 << (*f as u32),
                None => eprintln!(
                    "zeo: ZEO_DEBUG: unknown flag `{tok}` (known: {})",
                    NAMES.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
                ),
            }
        }
        bits
    });
    bits & (1 << (flag as u32)) != 0
}

/// The builtin class tables `ZEO_DEBUG_DROP_TABLE` names -- one symbol, or a
/// comma-separated list.
///
/// A VALUE rather than a bit, so it sits beside [`debug`] rather than in it.
/// It exists for one job: `cargo xtask size` links `puts 1` once per table
/// with that table dropped and diffs the binary, which is exact per-table
/// attribution and cannot be got any other way -- every size figure in the
/// docs before this was prose. A LIST prices a whole set at once, which is
/// the only way to see what the columns share: they overlap, because two
/// tables can root the same code.
///
/// Safe precisely because a missing table is now LOUD: `builtins::
/// registered_table` aborts naming the class rather than answering
/// `NoMethodError` for every row it has. Dropping one cannot produce a
/// quietly wrong program.
pub(crate) fn dropped_tables() -> &'static [String] {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        std::env::var("ZEO_DEBUG_DROP_TABLE")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .collect()
    })
}

/// The libraries `ZEO_DEBUG_RUNTIME_LOAD` names -- one feature, or a
/// comma-separated list.
///
/// A VALUE rather than a bit, for the same reason as [`dropped_tables`]: a
/// bit has no room for a name.
///
/// It answers one question, and it is the question every loader bug in this
/// area has had to answer first: is the failure the COMPILER's or the
/// LIBRARY's? A named feature gets no compile-time verdict, so its require
/// survives lowering and loads through `Kernel#require` at run time, against
/// the `$LOAD_PATH` the running program actually holds. If the library then
/// works, the compile-time loader is what to fix; if it fails the same way,
/// the runtime or the library is, and no amount of splice-order work will
/// help.
///
/// The whole library goes, not just the entry file: nothing demands its
/// units, so none of its files is compiled in at all.
pub(crate) fn loads_at_runtime(feature: &str) -> bool {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    let names = NAMES.get_or_init(|| {
        std::env::var("ZEO_DEBUG_RUNTIME_LOAD")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .collect()
    });
    // `rubygems` names `rubygems/version` too. A library is deferred whole or
    // not at all: deferring the entry file while compiling its parts in would
    // measure neither half.
    names.iter().any(|n| {
        feature == n
            || feature
                .strip_prefix(n.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

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
}

const NAMES: &[(&str, DebugFlag)] = &[
    ("runtime-struct", DebugFlag::RuntimeStruct),
    (
        "strict-ambiguous-require",
        DebugFlag::StrictAmbiguousRequire,
    ),
    ("verify-class-index", DebugFlag::VerifyClassIndex),
    ("no-typed-calls", DebugFlag::NoTypedCalls),
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
/// It exists for one job: `tools/zeo-dev size` links `puts 1` once per table
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

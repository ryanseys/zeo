//! One dial for the compiler's debug escape hatches and verification nets:
//! `ZEO_DEBUG=<flag>[,<flag>...]`, parsed once per process.
//!
//! This replaced seven single-purpose variables with three different
//! conventions among them (`ZEO_CGU_MODULES=0`, `ZEO_SHARE=0`,
//! `ZEO_STRUCT=runtime`, presence-checked `ZEO_STRICT_AMBIGUOUS_REQUIRE` /
//! `ZEO_VALIDATE` / `ZEO_VERIFY_SHARE` / `ZEO_VERIFY_CLASS_INDEX`) -- one
//! spelling, and discoverable in one place. An unknown flag warns rather than
//! erroring: a debug dial must never make a production compile fail, but a
//! typo silently doing nothing already cost real debugging time elsewhere.

use std::sync::OnceLock;

#[derive(Clone, Copy)]
pub(crate) enum DebugFlag {
    /// `no-cgu-modules`: emit the flat crate root instead of the `__cgu<N>`
    /// module partitions -- one-line field diagnosis for partition suspects.
    NoCguModules,
    /// `no-share`: materialize every class body instead of sharing duplicate
    /// method bodies.
    NoShare,
    /// `runtime-struct`: turn off compile-time `Struct.new` lowering.
    RuntimeStruct,
    /// `strict-ambiguous-require`: hard error when a feature resolves in
    /// more than one gem, instead of first-wins with a warning.
    StrictAmbiguousRequire,
    /// `validate`: re-parse the assembled program with `syn` (the collecting
    /// emission path) so an invalid emission fails HERE, not in rustc.
    Validate,
    /// `verify-share`: emit every shared member the unshared way too and
    /// assert the bodies agree.
    VerifyShare,
    /// `verify-class-index`: shadow-compare every `class_in_scope` answer
    /// against the linear scan it replaced.
    VerifyClassIndex,
}

const NAMES: &[(&str, DebugFlag)] = &[
    ("no-cgu-modules", DebugFlag::NoCguModules),
    ("no-share", DebugFlag::NoShare),
    ("runtime-struct", DebugFlag::RuntimeStruct),
    (
        "strict-ambiguous-require",
        DebugFlag::StrictAmbiguousRequire,
    ),
    ("validate", DebugFlag::Validate),
    ("verify-share", DebugFlag::VerifyShare),
    ("verify-class-index", DebugFlag::VerifyClassIndex),
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

/// The one builtin class table `ZEO_DEBUG_DROP_TABLE` names, if any.
///
/// A VALUE rather than a bit, so it sits beside [`debug`] rather than in it.
/// It exists for one job: `tools/zeo-dev size` links `puts 1` once per table
/// with that table dropped and diffs the binary, which is exact per-table
/// attribution and cannot be got any other way -- every size figure in the
/// docs before this was prose.
///
/// Safe precisely because a missing table is now LOUD: `builtins::
/// registered_table` aborts naming the class rather than answering
/// `NoMethodError` for every row it has. Dropping one cannot produce a
/// quietly wrong program.
pub(crate) fn dropped_table() -> Option<&'static str> {
    static NAME: OnceLock<Option<String>> = OnceLock::new();
    NAME.get_or_init(|| {
        std::env::var("ZEO_DEBUG_DROP_TABLE")
            .ok()
            .filter(|v| !v.is_empty())
    })
    .as_deref()
}

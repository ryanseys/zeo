//! The C extension surface.
//!
//! zeo compiles a gem's `ext/**/*.c` from source against MRI's own headers
//! (`crates/zeo-rt/cext/`, whose README states the stance) and never loads a
//! prebuilt MRI `.so`. This module is the other side of that boundary: the
//! `VALUE` encoding, the handles a heap `VALUE` points at, and the scope that
//! decides how long one lives.
//!
//! It is the one place the runtime's `unsafe` discipline changes. Everywhere
//! else, `unsafe` is a local claim about a pointer this crate made. Here, a C
//! function the project has never seen is handed a pointer and trusted with
//! it -- so the rules that matter are the ones the extension cannot break by
//! accident: the encoding is MRI's bit for bit, a handle is canonical per
//! object, and a layout reader that zeo cannot answer raises rather than
//! guesses.

pub mod alloc;
pub mod builtins;
pub mod call;
pub mod collection;
pub mod convert;
pub mod data;
pub mod error;
pub mod eval;
pub mod r#final;
pub mod format;
pub mod forward;
pub mod gc;
pub mod globals;
pub mod handles;
pub mod io;
pub mod jmp;
pub mod method;
pub mod misc;
pub mod numeric;
pub mod object;
pub mod scope;
pub mod st;
pub mod string;
pub mod stubs;
pub mod symbol;
pub mod thread;
pub mod value;

#[cfg(test)]
mod tests {
    /// `conformance/cext-api.tsv` is the ledger the stub file is generated
    /// from, and the two are checked against each other in CI by
    /// `zeo-dev cext api --check`. What that check cannot see is whether the
    /// ledger still describes the runtime, so these do.
    fn ledger() -> Vec<(String, String, String)> {
        let text = include_str!("../../../../conformance/cext-api.tsv");
        text.lines()
            .filter(|l| !l.starts_with('#') && !l.starts_with("symbol\t"))
            .filter_map(|l| {
                let mut f = l.split('\t');
                Some((f.next()?.into(), f.next()?.into(), f.next()?.into()))
            })
            .collect()
    }

    /// One `VALUE` global per row the ledger calls a variable, and every one
    /// of them addressable by the name C spells. A missing entry is a link
    /// error in a gem, with a message naming a symbol and no file or line.
    #[test]
    fn every_value_global_the_ledger_names_is_a_real_symbol() {
        let want: Vec<String> = ledger()
            .into_iter()
            .filter(|(_, kind, _)| kind == "var")
            .map(|(name, _, _)| name)
            .collect();
        assert!(!want.is_empty(), "the ledger has no variables at all");
        assert_eq!(
            want.len(),
            super::stubs::GLOBALS.len(),
            "the ledger and stubs.rs disagree about how many globals there are"
        );
        for name in &want {
            assert!(
                super::stubs::global(name).is_some(),
                "{name} is in the ledger and not in GLOBALS"
            );
        }
    }

    /// A global starts as `Qundef` and the loader moves it. Nothing can read
    /// one before that, because nothing has loaded -- but if a later change
    /// makes a global default to a real value, that is a silently wrong
    /// answer rather than a missing one, so it is worth pinning.
    #[test]
    fn a_global_starts_undefined() {
        for (name, g) in super::stubs::GLOBALS {
            assert_eq!(
                g.load(std::sync::atomic::Ordering::Relaxed),
                super::value::Q_UNDEF,
                "{name} was born with a value"
            );
        }
    }

    /// The whole point of the stub file: every symbol a gem can reference
    /// resolves. A row is implemented by the runtime, stubbed, or a `VALUE`
    /// global -- never none of the three, because none of the three is a
    /// link error in a gem.
    #[test]
    fn every_ledger_row_is_answered() {
        for (name, kind, status) in ledger() {
            assert!(
                matches!(status.as_str(), "zeo" | "stub" | "refused" | "global"),
                "{name} ({kind}) has status {status:?}, which resolves to nothing"
            );
            assert_eq!(
                status == "global",
                kind == "var",
                "{name}: only a variable can have the `global` status"
            );
        }
    }
}

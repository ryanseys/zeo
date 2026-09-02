//! zeo-capi: MRI's C extension API over the zeo-rt runtime.
//!
//! zeo compiles a gem's `ext/**/*.c` from source against MRI's own headers
//! (`cext/`, whose README states the stance) and never loads a prebuilt MRI
//! `.so`. This crate is the other side of that boundary: the `VALUE`
//! encoding, the handles a heap `VALUE` points at, and the scope that
//! decides how long one lives.
//!
//! It is the one place the runtime's `unsafe` discipline changes. Everywhere
//! else, `unsafe` is a local claim about a pointer zeo made. Here, a C
//! function the project has never seen is handed a pointer and trusted with
//! it -- so the rules that matter are the ones the extension cannot break by
//! accident: the encoding is MRI's bit for bit, a handle is canonical per
//! object, and a layout reader that zeo cannot answer raises rather than
//! guesses.
//!
//! The runtime never names this crate. It asks its four questions through
//! [`zeo_rt::capi_hooks`], and [`HOOKS`] is the one element this crate
//! contributes to that slice.

pub mod alloc;
pub mod api;
pub mod builtins;
pub mod call;
pub mod collection;
pub mod convert;
pub mod data;
pub mod encoding;
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
pub mod layout;
pub mod load;
pub mod method;
pub mod misc;
pub mod numeric;
pub mod object;
pub mod ractor;
pub mod scope;
pub mod st;
pub mod string;
pub mod stubs;
pub mod symbol;
pub mod thread;
pub mod util;
pub mod value;
pub mod view;

/// What the runtime may ask of this crate. Linking the crate is the whole
/// installation: no startup call, so an AOT binary that carries `libzeo.a`
/// is served the same way the JIT is.
#[linkme::distributed_slice(zeo_rt::capi_hooks::CAPI_HOOKS)]
static HOOKS: zeo_rt::capi_hooks::CapiHooks = zeo_rt::capi_hooks::CapiHooks {
    load: load::load,
    has_alloc_func: method::has_alloc_func,
    c_allocate: method::c_allocate,
    run_vm_at_exit: misc::run_vm_at_exit,
};

#[cfg(test)]
mod tests {
    /// `api::API` is the census the stub file is generated from, and the two
    /// are checked against each other in CI by `cargo xtask cext api --check`.
    /// What that check cannot see is whether the census still describes the
    /// RUNTIME, so these do.
    fn ledger() -> &'static [(&'static str, &'static str, &'static str, &'static str)] {
        super::api::API
    }

    /// One `VALUE` global per row the ledger calls a variable, and every one
    /// of them addressable by the name C spells. A missing entry is a link
    /// error in a gem, with a message naming a symbol and no file or line.
    #[test]
    fn every_value_global_the_ledger_names_is_a_real_symbol() {
        let want: Vec<&str> = ledger()
            .iter()
            .filter(|(_, kind, _, _)| *kind == "var")
            .map(|(name, _, _, _)| *name)
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

    /// Every forwarded `rb_*` is a symbol the runtime EXPORTS, not a stub.
    ///
    /// `forward.rs` implements each one, so the census has to agree -- a row
    /// the census still calls `stub` would mean two files define the same
    /// symbol, or that a real implementation is being reported as missing.
    #[test]
    fn every_forwarded_entry_is_implemented() {
        let forwarded = super::forward::FORWARDED;
        assert!(!forwarded.is_empty(), "the forwarding table is empty");
        let mut sorted: Vec<&str> = forwarded.iter().map(|(n, _, _, _)| *n).collect();
        let unsorted = sorted.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, unsorted, "FORWARDED must stay sorted by symbol");
        let mut bad: Vec<String> = Vec::new();
        for (name, klass, meth, _) in forwarded {
            match ledger().iter().find(|(n, _, _, _)| n == name) {
                None => bad.push(format!("{name} ({klass}#{meth}) is in no census row")),
                Some((_, _, status, _)) if *status != "zeo" => {
                    bad.push(format!("{name} forwards but the census says {status:?}"));
                }
                Some(_) => {}
            }
        }
        assert!(bad.is_empty(), "{bad:?}");
    }

    /// The whole point of the stub file: every symbol a gem can reference
    /// resolves. A row is implemented by the runtime, stubbed, or a `VALUE`
    /// global -- never none of the three, because none of the three is a
    /// link error in a gem.
    #[test]
    fn every_ledger_row_is_answered() {
        for (name, kind, status, _) in ledger() {
            assert!(
                matches!(*status, "zeo" | "stub" | "refused" | "global"),
                "{name} ({kind}) has status {status:?}, which resolves to nothing"
            );
            assert_eq!(
                *status == "global",
                *kind == "var",
                "{name}: only a variable can have the `global` status"
            );
        }
    }
}

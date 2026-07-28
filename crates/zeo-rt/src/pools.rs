//! Per-program literal pools. Codegen collects every literal `Symbol` name
//! and every frozen string literal it emits into two static tables at the
//! end of the generated program, and hot call sites index into
//! lazily-interned vectors -- `__SYMS.s(4)` / `__LITS.s(2)` -- instead of
//! re-interning per execution: `Symbol::intern` is a lock + hash even on a
//! hit, and `intern_frozen` also rebuilt its key `String` first. Both pools
//! intern once, on first touch, through the same interners as before, so
//! object identity is exactly what the per-site calls produced.

use crate::Symbol;
use crate::collections::RStr;
use crate::encoding::StrBuf;
use std::sync::OnceLock;

pub struct SymPool {
    names: &'static [&'static str],
    syms: OnceLock<Vec<Symbol>>,
}

impl SymPool {
    pub const fn new(names: &'static [&'static str]) -> SymPool {
        SymPool {
            names,
            syms: OnceLock::new(),
        }
    }

    #[inline]
    pub fn s(&self, i: usize) -> Symbol {
        self.syms
            .get_or_init(|| self.names.iter().map(|n| Symbol::intern(n)).collect())[i]
    }
}

pub struct LitPool {
    texts: &'static [&'static str],
    strs: OnceLock<Vec<RStr>>,
}

impl LitPool {
    pub const fn new(texts: &'static [&'static str]) -> LitPool {
        LitPool {
            texts,
            strs: OnceLock::new(),
        }
    }

    /// The interned frozen string at `i`, as a value -- an `Arc` bump, no
    /// re-hash. Equal literals across the program share one entry (the
    /// builder dedups), preserving `intern_frozen`'s shared-object identity.
    #[inline]
    pub fn s(&self, i: usize) -> crate::RubyValue {
        let strs = self.strs.get_or_init(|| {
            self.texts
                .iter()
                .map(|t| crate::collections::intern_frozen(StrBuf::from_utf8((*t).to_string())))
                .collect()
        });
        crate::RubyValue::Str(strs[i].clone())
    }
}

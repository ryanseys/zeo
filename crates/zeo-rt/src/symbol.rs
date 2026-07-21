//! A Ruby symbol. Mirrors zeo's `sp_sym` (`lib/sp_types.h`) -- an interned
//! integer id -- but simplified for the spike to pure runtime interning via a
//! `HashMap`, rather than zeo's split of a codegen-baked static name table
//! plus a small dynamic intern pool (`sp_sym_names`/`sp_dyn_syms`,
//! `codegen.c:4460-4478`). Every literal AND every runtime-computed symbol
//! goes through the same `Symbol::intern`, so two occurrences of the same
//! name -- however they were produced -- always compare equal. A
//! codegen-baked static table (matching zeo exactly) is a straightforward
//! later optimization; see `docs/PORTING_ANALYSIS.md`.
//!
//! Genuinely process-wide-shared (Part 9), not per-thread: real CRuby's
//! Symbol table is shared across every `Thread`/`Ractor` -- two threads
//! interning `:foo` must get the SAME id, which a `thread_local!` interner
//! could never guarantee (each thread would build its own independent
//! table). Migrated to a `LazyLock<Mutex<_>>` static for this reason, not
//! just as a mechanical `Rc`->`Arc` swap.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Symbol(u32);

#[derive(Default)]
struct Interner {
    names: Vec<String>,
    by_name: HashMap<String, u32>,
}

static INTERNER: LazyLock<Mutex<Interner>> = LazyLock::new(|| Mutex::new(Interner::default()));

impl Symbol {
    /// Mirrors `sp_sym_intern` (codegen.c:4470): look up or insert `name`,
    /// returning a stable id either way.
    pub fn intern(name: &str) -> Symbol {
        let mut i = INTERNER.lock();
        if let Some(&id) = i.by_name.get(name) {
            return Symbol(id);
        }
        let id = i.names.len() as u32;
        i.names.push(name.to_string());
        i.by_name.insert(name.to_string(), id);
        Symbol(id)
    }

    /// Mirrors `sp_sym_to_s`.
    /// The raw interner id -- stable for the process lifetime (the basis of
    /// `Symbol#object_id`'s derived value).
    pub fn to_u32(self) -> u32 {
        self.0
    }

    pub fn name(&self) -> String {
        INTERNER.lock().names[self.0 as usize].clone()
    }
}

impl std::fmt::Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name())
    }
}

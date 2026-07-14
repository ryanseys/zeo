//! A Ruby symbol. Mirrors spinel's `sp_sym` (`lib/sp_types.h`) -- an interned
//! integer id -- but simplified for the spike to pure runtime interning via a
//! `HashMap`, rather than spinel's split of a codegen-baked static name table
//! plus a small dynamic intern pool (`sp_sym_names`/`sp_dyn_syms`,
//! `codegen.c:4460-4478`). Every literal AND every runtime-computed symbol
//! goes through the same `Symbol::intern`, so two occurrences of the same
//! name -- however they were produced -- always compare equal. A
//! codegen-baked static table (matching spinel exactly) is a straightforward
//! later optimization; see `docs/PORTING_ANALYSIS.md`.

use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Symbol(u32);

#[derive(Default)]
struct Interner {
    names: Vec<String>,
    by_name: HashMap<String, u32>,
}

thread_local! {
    static INTERNER: RefCell<Interner> = RefCell::new(Interner::default());
}

impl Symbol {
    /// Mirrors `sp_sym_intern` (codegen.c:4470): look up or insert `name`,
    /// returning a stable id either way.
    pub fn intern(name: &str) -> Symbol {
        INTERNER.with(|i| {
            let mut i = i.borrow_mut();
            if let Some(&id) = i.by_name.get(name) {
                return Symbol(id);
            }
            let id = i.names.len() as u32;
            i.names.push(name.to_string());
            i.by_name.insert(name.to_string(), id);
            Symbol(id)
        })
    }

    /// Mirrors `sp_sym_to_s`.
    pub fn name(&self) -> String {
        INTERNER.with(|i| i.borrow().names[self.0 as usize].clone())
    }
}

impl std::fmt::Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name())
    }
}

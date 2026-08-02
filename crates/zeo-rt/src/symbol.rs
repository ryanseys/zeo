//! A Ruby symbol: an interned integer id. Interning is pure runtime via a
//! map -- every literal and every runtime-computed symbol goes through
//! the same `Symbol::intern`, so two occurrences of the same name always
//! compare equal, however they were produced.
//!
//! Genuinely process-wide-shared, not per-thread: real CRuby's
//! Symbol table is shared across every `Thread`/`Ractor` -- two threads
//! interning `:foo` must get the SAME id, which a `thread_local!` interner
//! could never guarantee (each thread would build its own independent
//! table). Migrated to a `LazyLock<Mutex<_>>` static for this reason, not
//! just as a mechanical `Rc`->`Arc` swap.
//!
//! Names are LEAKED, once per distinct symbol: Ruby symbols are immortal
//! (never garbage collected), so the leak is the intended lifetime -- and it
//! is what lets [`Symbol::name_str`] hand out allocation-free `&'static`
//! names on the dispatch hot path instead of cloning a `String` per send.

use crate::FMap;
use parking_lot::Mutex;
use std::sync::LazyLock;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Symbol(u32);

#[derive(Default)]
struct Interner {
    names: Vec<&'static str>,
    by_name: FMap<&'static str, u32>,
}

static INTERNER: LazyLock<Mutex<Interner>> = LazyLock::new(|| Mutex::new(Interner::default()));

impl Symbol {
    /// Look up or insert `name`, returning a stable id either way.
    pub fn intern(name: &str) -> Symbol {
        let mut i = INTERNER.lock();
        if let Some(&id) = i.by_name.get(name) {
            return Symbol(id);
        }
        let name: &'static str = Box::leak(name.to_string().into_boxed_str());
        let id = i.names.len() as u32;
        i.names.push(name);
        i.by_name.insert(name, id);
        Symbol(id)
    }

    /// The raw interner id -- stable for the process lifetime (the basis of
    /// `Symbol#object_id`'s derived value).
    pub fn to_u32(self) -> u32 {
        self.0
    }

    /// The interned text, allocation-free -- what the dispatch path reads.
    pub fn name_str(self) -> &'static str {
        INTERNER.lock().names[self.0 as usize]
    }

    /// How many distinct symbols exist. Nothing is ever removed from the
    /// interner, so this only grows -- which is what lets
    /// `ObjectSpace.count_symbols` report the total as `immortal_symbol`.
    pub fn count() -> usize {
        INTERNER.lock().names.len()
    }

    /// The Symbol with interner id `id` -- the inverse of [`Symbol::to_u32`],
    /// and what lets `Symbol.all_symbols` walk the table by index.
    pub fn from_u32(id: u32) -> Symbol {
        Symbol(id)
    }

    pub fn name(&self) -> String {
        self.name_str().to_string()
    }
}

impl std::fmt::Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name_str())
    }
}

/// Well-known symbols the runtime itself dispatches on (`to_s` per rendered
/// value, `initialize` per construction, `===` per case arm, ...): interned
/// once, then a lock-free copy -- replacing a global-mutex `Symbol::intern`
/// round-trip per call at those sites.
pub(crate) mod wk {
    use super::Symbol;
    use std::sync::OnceLock;

    macro_rules! wk_symbols {
        ($($name:ident => $text:literal),* $(,)?) => {
            $(pub(crate) fn $name() -> Symbol {
                static S: OnceLock<Symbol> = OnceLock::new();
                *S.get_or_init(|| Symbol::intern($text))
            })*
        };
    }

    wk_symbols! {
        backtrace => "backtrace",
        call => "call",
        case_eq => "===",
        chomp => "chomp",
        dig => "dig",
        each => "each",
        initialize => "initialize",
        inspect => "inspect",
        method_missing => "method_missing",
        public_send => "public_send",
        respond_to_missing => "respond_to_missing?",
        to_a => "to_a",
        to_ary => "to_ary",
        to_s => "to_s",
        write => "write",
    }
}

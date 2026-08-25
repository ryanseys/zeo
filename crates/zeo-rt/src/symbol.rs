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
//!
//! The mutex is on the WRITE path only. Reads go through the published slab
//! (see `SLAB`) with no lock at all: an entry is written once, never mutated,
//! and holds nothing but `'static` data, so a reader needs only to observe a
//! finished entry.

use crate::FMap;
use crate::encoding::EncodingId;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{LazyLock, OnceLock};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Symbol(u32);

/// One interned symbol: ruby interns by BYTES plus encoding, so the same
/// bytes under two encodings are two symbols and `to_s` can spell either
/// back exactly. `lossy` is the UTF-8 reading every internal by-name path
/// (dispatch, rendering) keys on -- for the DEFAULT encodings (US-ASCII,
/// UTF-8) it IS the bytes.
struct SymEntry {
    lossy: &'static str,
    bytes: &'static [u8],
    enc: EncodingId,
}

/// The PUBLISHED table, read with no lock at all.
///
/// An entry is written once and never mutated, and every field in it is
/// `'static`, so a reader needs no exclusion -- only to observe an entry
/// that is fully written. Each slot is a `OnceLock`, which is exactly that
/// guarantee, and the table is CHUNKED so growth never moves an entry a
/// reader is looking at the way a `Vec` reallocation would.
///
/// `INTERNER`'s mutex still serializes WRITERS (it owns the two by-name
/// maps); it is simply no longer on the read path. That read path is
/// `instance_method_visibility`, `responds_to` and `class_method_is_private`
/// -- all of which ask per ANCESTOR -- so it was a global mutex acquired
/// several times per dynamic call.
const SYM_CHUNK: usize = 4096;
const SYM_CHUNKS: usize = 1024;
type SymChunk = Box<[OnceLock<&'static SymEntry>]>;
static SLAB: [OnceLock<SymChunk>; SYM_CHUNKS] = [const { OnceLock::new() }; SYM_CHUNKS];
static SYM_COUNT: AtomicU32 = AtomicU32::new(0);

/// Publish one entry at `id`. Called only under `INTERNER`'s lock, which is
/// what makes "no slot is written twice" true.
fn publish(id: u32, entry: SymEntry) {
    let (c, i) = (id as usize / SYM_CHUNK, id as usize % SYM_CHUNK);
    assert!(
        c < SYM_CHUNKS,
        "symbol table exhausted at {} symbols -- ruby symbols are immortal, so \
         this means the program interned unboundedly many distinct names",
        SYM_CHUNK * SYM_CHUNKS
    );
    let chunk = SLAB[c].get_or_init(|| (0..SYM_CHUNK).map(|_| OnceLock::new()).collect());
    // Leaked to match the module's policy for names: symbols are immortal, so
    // the entry's lifetime is the process's.
    let _ = chunk[i].set(Box::leak(Box::new(entry)));
    SYM_COUNT.store(id + 1, Ordering::Release);
}

fn entry_of(id: u32) -> &'static SymEntry {
    let (c, i) = (id as usize / SYM_CHUNK, id as usize % SYM_CHUNK);
    SLAB[c]
        .get()
        .and_then(|chunk| chunk[i].get())
        .copied()
        .expect("every Symbol id was minted by the interner, which publishes before returning")
}

#[derive(Default)]
struct Interner {
    /// Default-encoding symbols (ASCII text normalizes to US-ASCII, valid
    /// UTF-8 to UTF-8 -- CRuby's own rule), keyed by text so the hot
    /// `intern(&str)` path stays one allocation-free probe.
    by_lossy: FMap<&'static str, u32>,
    /// Every OTHER `(encoding, bytes)` pair -- the rare path, reached only
    /// through `String#to_sym` on a non-UTF-8 string.
    by_key: FMap<(EncodingId, Vec<u8>), u32>,
}

static INTERNER: LazyLock<Mutex<Interner>> = LazyLock::new(|| Mutex::new(Interner::default()));

impl Symbol {
    /// Look up or insert `name`, returning a stable id either way.
    pub fn intern(name: &str) -> Symbol {
        let mut i = INTERNER.lock();
        if let Some(&id) = i.by_lossy.get(name) {
            return Symbol(id);
        }
        let name: &'static str = Box::leak(name.to_string().into_boxed_str());
        let enc = if name.is_ascii() {
            crate::encoding::US_ASCII
        } else {
            crate::encoding::UTF_8
        };
        let id = SYM_COUNT.load(Ordering::Relaxed);
        publish(
            id,
            SymEntry {
                lossy: name,
                bytes: name.as_bytes(),
                enc,
            },
        );
        i.by_lossy.insert(name, id);
        Symbol(id)
    }

    /// The Symbol for `name` ONLY if something already interned it.
    ///
    /// `rb_check_id` needs this: an extension calls it to skip a lookup that
    /// cannot succeed, and minting the ID here would make every such skip
    /// take the slow path forever after.
    pub fn interned(name: &str) -> Option<Symbol> {
        INTERNER.lock().by_lossy.get(name).copied().map(Symbol)
    }

    /// `String#to_sym`'s entry: intern by the string's exact bytes AND
    /// encoding. ASCII text in any ASCII-compatible encoding is the
    /// US-ASCII symbol and valid UTF-8 the UTF-8 one (both via the default
    /// path); anything else keys `(encoding, bytes)` so `to_s` restores the
    /// original string exactly.
    pub fn intern_bytes(bytes: &[u8], enc: EncodingId) -> Symbol {
        if ((bytes.is_ascii() && enc.ascii_compatible()) || enc == crate::encoding::UTF_8)
            && let Ok(text) = std::str::from_utf8(bytes)
        {
            return Symbol::intern(text);
        }
        let mut i = INTERNER.lock();
        if let Some(&id) = i.by_key.get(&(enc, bytes.to_vec())) {
            return Symbol(id);
        }
        let lossy: String = crate::collections::string_from_bytes(bytes.to_vec(), enc)
            .lock()
            .to_utf8_lossy()
            .into_owned();
        let lossy: &'static str = Box::leak(lossy.into_boxed_str());
        let bytes_static: &'static [u8] = Box::leak(bytes.to_vec().into_boxed_slice());
        let id = SYM_COUNT.load(Ordering::Relaxed);
        publish(
            id,
            SymEntry {
                lossy,
                bytes: bytes_static,
                enc,
            },
        );
        i.by_key.insert((enc, bytes.to_vec()), id);
        Symbol(id)
    }

    /// The raw interner id -- stable for the process lifetime (the basis of
    /// `Symbol#object_id`'s derived value).
    pub fn to_u32(self) -> u32 {
        self.0
    }

    /// The interned text -- what the dispatch path reads. Allocation-free
    /// AND lock-free: two indexes into the published slab.
    pub fn name_str(self) -> &'static str {
        entry_of(self.0).lossy
    }

    /// The exact source bytes -- what `Symbol#to_s` spells back.
    pub fn bytes(self) -> &'static [u8] {
        entry_of(self.0).bytes
    }

    /// The encoding the symbol was minted under.
    pub fn encoding(self) -> EncodingId {
        entry_of(self.0).enc
    }

    /// How many distinct symbols exist. Nothing is ever removed from the
    /// interner, so this only grows -- which is what lets
    /// `ObjectSpace.count_symbols` report the total as `immortal_symbol`.
    pub fn count() -> usize {
        SYM_COUNT.load(Ordering::Acquire) as usize
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
        cmp => "<=>",
        dig => "dig",
        each => "each",
        eq => "==",
        hash => "hash",
        initialize => "initialize",
        inspect => "inspect",
        method => "method",
        method_missing => "method_missing",
        plus => "+",
        public_send => "public_send",
        respond_to => "respond_to?",
        respond_to_missing => "respond_to_missing?",
        rt_clone => "clone",
        rt_dup => "dup",
        rt_freeze => "freeze",
        rt_initialize_copy => "initialize_copy",
        send => "send",
        to_a => "to_a",
        to_ary => "to_ary",
        to_s => "to_s",
        write => "write",
    }
}

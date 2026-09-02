//! `st_table`: MRI's general-purpose hash table.
//!
//! An extension uses this for its OWN bookkeeping -- a name-to-handler map,
//! a seen set -- and never to reach a Ruby Hash's storage. That split is what
//! makes it implementable: nothing here has to agree with how zeo stores a
//! `Hash`, only with what `st.h` promises.
//!
//! # The struct is public, so the layout is real
//!
//! `struct st_table` is defined in the vendored `ruby/st.h`, not forward
//! declared, so an extension can read `tbl->num_entries` and `sizeof` it.
//! [`StTable`] therefore has exactly those fields in exactly that order, and
//! `the_header_matches_the_vendored_struct` reads the header back to prove
//! it.
//!
//! What the header CANNOT promise is `entries` and `bins`: `struct
//! st_table_entry` is declared and never defined ("defined in st.c"), so no
//! extension can walk them. They stay null, and the real storage lives in a
//! side table keyed by the header's address. `num_entries` and `type` are
//! kept truthful because those two are readable.
//!
//! # Ordering is load-bearing
//!
//! `st_foreach` walks in INSERTION order -- that is what makes Ruby's Hash
//! ordered -- so the entries are a `Vec` and a deletion leaves a hole rather
//! than swapping the last row into it.

use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void};
use std::sync::Mutex;

/// MRI's `st_data_t`: pointer-width, and whatever the caller says it means.
pub type StData = usize;
pub type StIndex = usize;

/// `struct st_hash_type`, which the caller owns and this only reads.
#[repr(C)]
pub struct StHashType {
    pub compare: Option<unsafe extern "C" fn(StData, StData) -> c_int>,
    pub hash: Option<unsafe extern "C" fn(StData) -> StIndex>,
}

/// `struct st_table`, field for field. Only `type` and `num_entries` carry a
/// meaning an extension can read; the rest are what `sizeof` needs.
#[repr(C)]
pub struct StTable {
    pub entry_power: u8,
    pub bin_power: u8,
    pub size_ind: u8,
    pub rebuilds_num: u32,
    pub hash_type: *const StHashType,
    pub num_entries: StIndex,
    pub bins: *mut StIndex,
    pub entries_start: StIndex,
    pub entries_bound: StIndex,
    pub entries: *mut c_void,
}

/// `enum st_retval`. `ST_STOP` and anything a callback invents both end a
/// walk, which is what MRI does with an answer it does not know.
const ST_CONTINUE: c_int = 0;
const ST_STOP: c_int = 1;
const ST_DELETE: c_int = 2;
const ST_CHECK: c_int = 3;
const ST_REPLACE: c_int = 4;

/// One table's real storage. A deleted row is a `None` so the survivors keep
/// their insertion order and their indices.
#[derive(Default)]
struct Store {
    rows: Vec<Option<(StData, StData)>>,
    /// Hash to the row indices that carry it. A hash collision is normal and
    /// the compare function is what separates them.
    index: HashMap<StIndex, Vec<usize>>,
    live: usize,
    /// Which of the built-in key kinds this is, or `Custom` for a caller's
    /// own `st_hash_type`. It decides hashing and comparison.
    kind: Kind,
}

#[derive(Clone, Copy, PartialEq, Default)]
enum Kind {
    /// `st_init_numtable`: the key is the number.
    #[default]
    Num,
    /// `st_init_strtable`: the key is a `const char *`.
    Str,
    /// `st_init_strcasetable`: the same, compared without case.
    StrCase,
    /// `st_init_table(type)`: the caller's own hash and compare.
    Custom,
}

/// Every live table, by its header's address. A `Mutex` rather than a
/// thread-local: an extension may hand a table to another thread, and MRI's
/// own tables are not thread-local either.
static STORES: Mutex<Option<HashMap<usize, Store>>> = Mutex::new(None);

fn with_store<R>(tbl: *mut StTable, f: impl FnOnce(&mut Store) -> R) -> Option<R> {
    let mut guard = STORES.lock().ok()?;
    let map = guard.get_or_insert_with(HashMap::new);
    map.get_mut(&(tbl as usize)).map(f)
}

/// # Safety
///
/// `p` must be NUL-terminated, or zero.
unsafe fn key_bytes(p: StData) -> &'static [u8] {
    if p == 0 {
        return b"";
    }
    // SAFETY: a `Str` table's keys are `const char *` by construction, and
    // the caller owns them for as long as the row lives.
    unsafe { std::ffi::CStr::from_ptr(p as *const c_char).to_bytes() }
}

impl Store {
    /// The hash for a key, by the table's kind.
    ///
    /// # Safety
    ///
    /// A `Str` key must be NUL-terminated and a `Custom` table's `hash` must
    /// accept the key -- both the caller's own promise when it built the
    /// table.
    unsafe fn hash(&self, ty: *const StHashType, key: StData) -> StIndex {
        match self.kind {
            Kind::Num => num_hash(key),
            Kind::Str => bytes_hash(unsafe { key_bytes(key) }),
            Kind::StrCase => {
                let lower: Vec<u8> = unsafe { key_bytes(key) }
                    .iter()
                    .map(u8::to_ascii_lowercase)
                    .collect();
                bytes_hash(&lower)
            }
            Kind::Custom => match unsafe { ty.as_ref() }.and_then(|t| t.hash) {
                // SAFETY: the caller's own function, on the caller's own key.
                Some(f) => unsafe { f(key) },
                None => num_hash(key),
            },
        }
    }

    /// # Safety
    ///
    /// Same contract as [`Store::hash`].
    unsafe fn eq(&self, ty: *const StHashType, a: StData, b: StData) -> bool {
        match self.kind {
            Kind::Num => a == b,
            Kind::Str => unsafe { key_bytes(a) == key_bytes(b) },
            Kind::StrCase => unsafe { key_bytes(a).eq_ignore_ascii_case(key_bytes(b)) },
            Kind::Custom => match unsafe { ty.as_ref() }.and_then(|t| t.compare) {
                // MRI's convention: 0 means equal, as `strcmp` does.
                Some(f) => unsafe { f(a, b) == 0 },
                None => a == b,
            },
        }
    }

    /// # Safety
    ///
    /// Same contract as [`Store::hash`].
    unsafe fn find(&self, ty: *const StHashType, key: StData) -> Option<usize> {
        let h = unsafe { self.hash(ty, key) };
        self.index
            .get(&h)?
            .iter()
            .copied()
            .find(|i| self.rows[*i].is_some_and(|(k, _)| unsafe { self.eq(ty, k, key) }))
    }

    /// # Safety
    ///
    /// Same contract as [`Store::hash`].
    unsafe fn insert(&mut self, ty: *const StHashType, key: StData, val: StData) -> bool {
        if let Some(i) = unsafe { self.find(ty, key) } {
            // MRI keeps the ORIGINAL key and replaces only the value.
            let old = self.rows[i].expect("find only answers live rows").0;
            self.rows[i] = Some((old, val));
            return true;
        }
        unsafe { self.add(ty, key, val) };
        false
    }

    /// The unconditional append `st_add_direct` is: no lookup, so a duplicate
    /// key becomes a second row. MRI documents that, and extensions rely on
    /// it where they already know the key is absent.
    ///
    /// # Safety
    ///
    /// Same contract as [`Store::hash`].
    unsafe fn add(&mut self, ty: *const StHashType, key: StData, val: StData) {
        let h = unsafe { self.hash(ty, key) };
        self.rows.push(Some((key, val)));
        self.index.entry(h).or_default().push(self.rows.len() - 1);
        self.live += 1;
    }

    fn remove_at(&mut self, i: usize) -> Option<(StData, StData)> {
        let row = self.rows.get_mut(i)?.take()?;
        self.live -= 1;
        Some(row)
    }

    /// Every live row in insertion order, copied. The callback may insert,
    /// delete or free the table, so the walk cannot hold a borrow.
    fn snapshot(&self) -> Vec<(usize, StData, StData)> {
        self.rows
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.map(|(k, v)| (i, k, v)))
            .collect()
    }
}

/// MRI's `st_numhash` is the identity with the low bits spread, because its
/// keys are often pointers whose low bits are all zero.
fn num_hash(n: StData) -> StIndex {
    (n >> 3) ^ (n << 3)
}

/// FNV-1a. MRI uses siphash with a per-process random seed, so no extension
/// can depend on the VALUE of a hash -- only that equal keys hash equally.
///
/// `rb_str_hash` answers from here too, so a String used as an `st_table`
/// key and the same String hashed directly agree.
pub(super) fn bytes_hash_of(bytes: &[u8]) -> StIndex {
    bytes_hash(bytes)
}

fn bytes_hash(bytes: &[u8]) -> StIndex {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h as StIndex
}

/// Mint a table with `kind`'s key semantics.
fn new_table(kind: Kind, ty: *const StHashType) -> *mut StTable {
    let tbl = Box::into_raw(Box::new(StTable {
        entry_power: 0,
        bin_power: 0,
        size_ind: 0,
        rebuilds_num: 0,
        hash_type: ty,
        num_entries: 0,
        bins: std::ptr::null_mut(),
        entries_start: 0,
        entries_bound: 0,
        entries: std::ptr::null_mut(),
    }));
    if let Ok(mut guard) = STORES.lock() {
        guard.get_or_insert_with(HashMap::new).insert(
            tbl as usize,
            Store {
                kind,
                ..Store::default()
            },
        );
    }
    tbl
}

/// Keep the header's `num_entries` in step with the store, because an
/// extension reads it directly through the struct.
fn sync(tbl: *mut StTable) {
    let Some(live) = with_store(tbl, |s| s.live) else {
        return;
    };
    // SAFETY: `with_store` answered, so the table is one this module made
    // and has not freed.
    unsafe {
        (*tbl).num_entries = live;
        (*tbl).entries_bound = live;
    }
}

/// # Safety
///
/// `slot` may be null; when it is not, it is the caller's own word.
unsafe fn store_out(slot: *mut StData, v: StData) {
    if !slot.is_null() {
        // SAFETY: the caller's contract.
        unsafe { slot.write(v) };
    }
}

/// The `st_hash_type` a table was built with, for the `Custom` kind.
fn table_type(tbl: *mut StTable) -> *const StHashType {
    if tbl.is_null() {
        return std::ptr::null();
    }
    // SAFETY: every table this module hands out is a live `Box<StTable>`, and
    // the field is written once at construction.
    unsafe { (*tbl).hash_type }
}

/// `cext_fn!` cannot wrap these: they take and answer raw C types with no
/// `VALUE` in sight, so there is no `Signal` to raise and nothing to pin. A
/// call on a freed table answers the "not found" value rather than raising,
/// which is what MRI does with one too.
macro_rules! st_fn {
    ($(
        $(#[$meta:meta])*
        fn $name:ident($($arg:ident : $ty:ty),* $(,)?) -> $ret:ty $body:block
    )*) => {$(
        $(#[$meta])*
        #[unsafe(no_mangle)]
        // A C ABI entry point with MRI's own `st_*` contract.
        #[allow(clippy::missing_safety_doc)]
        pub unsafe extern "C" fn $name($($arg : $ty),*) -> $ret $body
    )*};
}

st_fn! {
    fn rb_st_init_numtable() -> *mut StTable {
        new_table(Kind::Num, std::ptr::null())
    }

    fn rb_st_init_numtable_with_size(_n: StIndex) -> *mut StTable {
        new_table(Kind::Num, std::ptr::null())
    }

    fn rb_st_init_strtable() -> *mut StTable {
        new_table(Kind::Str, std::ptr::null())
    }

    fn rb_st_init_strtable_with_size(_n: StIndex) -> *mut StTable {
        new_table(Kind::Str, std::ptr::null())
    }

    fn rb_st_init_strcasetable() -> *mut StTable {
        new_table(Kind::StrCase, std::ptr::null())
    }

    fn rb_st_init_strcasetable_with_size(_n: StIndex) -> *mut StTable {
        new_table(Kind::StrCase, std::ptr::null())
    }

    /// `st_init_table(type)`: the caller supplies hash and compare, and owns
    /// the `st_hash_type` for as long as the table lives -- MRI's contract
    /// too, since it stores the pointer rather than a copy.
    fn rb_st_init_table(ty: *const StHashType) -> *mut StTable {
        new_table(Kind::Custom, ty)
    }

    fn rb_st_init_table_with_size(ty: *const StHashType, _n: StIndex) -> *mut StTable {
        new_table(Kind::Custom, ty)
    }

    fn rb_st_free_table(tbl: *mut StTable) -> () {
        if tbl.is_null() {
            return;
        }
        if let Ok(mut guard) = STORES.lock() {
            guard.get_or_insert_with(HashMap::new).remove(&(tbl as usize));
        }
        // SAFETY: every table came from `Box::into_raw` in `new_table`.
        drop(unsafe { Box::from_raw(tbl) });
    }

    fn rb_st_clear(tbl: *mut StTable) -> () {
        with_store(tbl, |s| {
            s.rows.clear();
            s.index.clear();
            s.live = 0;
        });
        sync(tbl);
    }

    fn rb_st_table_size(tbl: *const StTable) -> usize {
        with_store(tbl.cast_mut(), |s| s.live).unwrap_or(0)
    }

    /// `st_memsize`: how much the table costs. The rows and the index are the
    /// whole of it, since the keys and values are the caller's.
    fn rb_st_memsize(tbl: *const StTable) -> usize {
        let rows = with_store(tbl.cast_mut(), |s| s.rows.len()).unwrap_or(0);
        std::mem::size_of::<StTable>() + rows * std::mem::size_of::<(StData, StData)>() * 2
    }

    /// Answers 1 when the key was already there and 0 when it was added --
    /// MRI's convention, and the opposite of what "did it work" would say.
    fn rb_st_insert(tbl: *mut StTable, key: StData, val: StData) -> c_int {
        let ty = table_type(tbl);
        let existed = with_store(tbl, |s| unsafe { s.insert(ty, key, val) }).unwrap_or(false);
        sync(tbl);
        c_int::from(existed)
    }

    /// `st_insert2(tbl, key, val, func)`: the same, but a NEW key is passed
    /// through `func` first. Extensions use it to take ownership of a key
    /// they only borrowed.
    fn rb_st_insert2(
        tbl: *mut StTable,
        key: StData,
        val: StData,
        func: Option<unsafe extern "C" fn(StData) -> StData>,
    ) -> c_int {
        let ty = table_type(tbl);
        let found = with_store(tbl, |s| unsafe { s.find(ty, key) }).flatten();
        if let Some(i) = found {
            with_store(tbl, |s| {
                let old = s.rows[i].expect("find only answers live rows").0;
                s.rows[i] = Some((old, val));
            });
            return 1;
        }
        // SAFETY: the caller's own function, on the caller's own key.
        let owned = match func {
            Some(f) => unsafe { f(key) },
            None => key,
        };
        with_store(tbl, |s| unsafe { s.add(ty, owned, val) });
        sync(tbl);
        0
    }

    /// `st_add_direct`: append without looking. The caller has already
    /// established the key is absent.
    fn rb_st_add_direct(tbl: *mut StTable, key: StData, val: StData) -> () {
        let ty = table_type(tbl);
        with_store(tbl, |s| unsafe { s.add(ty, key, val) });
        sync(tbl);
    }

    fn rb_st_lookup(tbl: *mut StTable, key: StData, out: *mut StData) -> c_int {
        let ty = table_type(tbl);
        let found = with_store(tbl, |s| {
            unsafe { s.find(ty, key) }.map(|i| s.rows[i].expect("a live row").1)
        })
        .flatten();
        match found {
            Some(v) => {
                unsafe { store_out(out, v) };
                1
            }
            None => 0,
        }
    }

    /// `st_get_key`: the STORED key rather than the value. They differ when
    /// the compare function calls two distinct keys equal, which is the whole
    /// reason this entry exists.
    fn rb_st_get_key(tbl: *mut StTable, key: StData, out: *mut StData) -> c_int {
        let ty = table_type(tbl);
        let found = with_store(tbl, |s| {
            unsafe { s.find(ty, key) }.map(|i| s.rows[i].expect("a live row").0)
        })
        .flatten();
        match found {
            Some(k) => {
                unsafe { store_out(out, k) };
                1
            }
            None => 0,
        }
    }

    /// `st_delete(tbl, &key, &value)`. `key` is IN OUT: the stored key is
    /// written back, which is how a caller frees a key it no longer owns.
    fn rb_st_delete(tbl: *mut StTable, key: *mut StData, val: *mut StData) -> c_int {
        if key.is_null() {
            return 0;
        }
        let ty = table_type(tbl);
        // SAFETY: the caller's own word.
        let wanted = unsafe { key.read() };
        let gone = with_store(tbl, |s| {
            unsafe { s.find(ty, wanted) }.and_then(|i| s.remove_at(i))
        })
        .flatten();
        sync(tbl);
        match gone {
            Some((k, v)) => {
                unsafe { store_out(key, k) };
                unsafe { store_out(val, v) };
                1
            }
            None => {
                unsafe { store_out(val, 0) };
                0
            }
        }
    }

    /// `st_delete_safe`: delete during an iteration by replacing the key with
    /// `never` rather than removing the row. A row is already a hole here, so
    /// the two are one operation and `never` is not needed.
    fn rb_st_delete_safe(
        tbl: *mut StTable,
        key: *mut StData,
        val: *mut StData,
        _never: StData,
    ) -> c_int {
        unsafe { rb_st_delete(tbl, key, val) }
    }

    /// `st_cleanup_safe`: compact the holes `st_delete_safe` left. They cost
    /// only space here, so this reclaims it and changes nothing observable.
    fn rb_st_cleanup_safe(tbl: *mut StTable, _never: StData) -> () {
        let ty = table_type(tbl);
        with_store(tbl, |s| {
            let live: Vec<(StData, StData)> = s.rows.iter().flatten().copied().collect();
            s.rows.clear();
            s.index.clear();
            s.live = 0;
            for (k, v) in live {
                unsafe { s.add(ty, k, v) };
            }
        });
        sync(tbl);
    }

    /// `st_shift`: take the OLDEST row out. Insertion order is what makes
    /// that well defined.
    fn rb_st_shift(tbl: *mut StTable, key: *mut StData, val: *mut StData) -> c_int {
        let gone = with_store(tbl, |s| {
            let first = s.rows.iter().position(Option::is_some)?;
            s.remove_at(first)
        })
        .flatten();
        sync(tbl);
        match gone {
            Some((k, v)) => {
                unsafe { store_out(key, k) };
                unsafe { store_out(val, v) };
                1
            }
            None => 0,
        }
    }

    fn rb_st_copy(tbl: *mut StTable) -> *mut StTable {
        let ty = table_type(tbl);
        let Some((kind, rows)) = with_store(tbl, |s| (s.kind, s.snapshot())) else {
            return std::ptr::null_mut();
        };
        let out = new_table(kind, ty);
        with_store(out, |s| {
            for (_, k, v) in rows {
                unsafe { s.add(ty, k, v) };
            }
        });
        sync(out);
        out
    }

    /// `st_foreach(tbl, f, arg)`. The walk runs over a SNAPSHOT, so a
    /// callback may insert, delete or free the table without invalidating
    /// it. Answers 0, as MRI does.
    fn rb_st_foreach(
        tbl: *mut StTable,
        f: Option<unsafe extern "C" fn(StData, StData, StData) -> c_int>,
        arg: StData,
    ) -> c_int {
        let Some(f) = f else { return 0 };
        for (i, k, v) in with_store(tbl, |s| s.snapshot()).unwrap_or_default() {
            // SAFETY: the caller's own callback, on its own key and value.
            match unsafe { f(k, v, arg) } {
                ST_CONTINUE => {}
                ST_DELETE => {
                    with_store(tbl, |s| s.remove_at(i));
                    sync(tbl);
                }
                ST_STOP => break,
                // MRI ends the walk on an answer it does not know too.
                _ => break,
            }
        }
        0
    }

    /// `st_foreach_safe` is `st_foreach` with a deletion-during-walk that is
    /// already safe here, because the walk runs over a snapshot.
    fn rb_st_foreach_safe(
        tbl: *mut StTable,
        f: Option<unsafe extern "C" fn(StData, StData, StData) -> c_int>,
        arg: StData,
    ) -> () {
        unsafe { rb_st_foreach(tbl, f, arg) };
    }

    /// `st_foreach_check(tbl, f, arg, never)`: the callback takes a fourth
    /// argument and may answer `ST_CHECK`, which asks the walk to verify the
    /// row is still there. The snapshot makes that always true, so
    /// `ST_CHECK` continues.
    fn rb_st_foreach_check(
        tbl: *mut StTable,
        f: Option<unsafe extern "C" fn(StData, StData, StData, c_int) -> c_int>,
        arg: StData,
        _never: StData,
    ) -> c_int {
        let Some(f) = f else { return 0 };
        for (i, k, v) in with_store(tbl, |s| s.snapshot()).unwrap_or_default() {
            // SAFETY: the caller's own callback.
            match unsafe { f(k, v, arg, 0) } {
                ST_CONTINUE | ST_CHECK => {}
                ST_DELETE => {
                    with_store(tbl, |s| s.remove_at(i));
                    sync(tbl);
                }
                ST_STOP => break,
                _ => break,
            }
        }
        0
    }

    /// `st_foreach_with_replace(tbl, f, replace, arg)`: `f` answers
    /// `ST_REPLACE` to ask `replace` for a new key and value.
    fn rb_st_foreach_with_replace(
        tbl: *mut StTable,
        f: Option<unsafe extern "C" fn(StData, StData, StData, c_int) -> c_int>,
        replace: Option<unsafe extern "C" fn(*mut StData, *mut StData, StData, c_int) -> c_int>,
        arg: StData,
    ) -> c_int {
        let Some(f) = f else { return 0 };
        for (i, k, v) in with_store(tbl, |s| s.snapshot()).unwrap_or_default() {
            // SAFETY: the caller's own callbacks.
            match unsafe { f(k, v, arg, 0) } {
                ST_CONTINUE | ST_CHECK => {}
                ST_DELETE => {
                    with_store(tbl, |s| s.remove_at(i));
                    sync(tbl);
                }
                ST_REPLACE => {
                    let (mut nk, mut nv) = (k, v);
                    if let Some(r) = replace {
                        unsafe { r(&raw mut nk, &raw mut nv, arg, 0) };
                    }
                    with_store(tbl, |s| s.rows[i] = Some((nk, nv)));
                }
                ST_STOP => break,
                _ => break,
            }
        }
        0
    }

    /// `st_update(tbl, key, func, arg)`: `func(&key, &value, arg, existing)`
    /// decides. It is the only entry that can insert, update and delete in
    /// one lookup, and it answers whether the key was present.
    fn rb_st_update(
        tbl: *mut StTable,
        key: StData,
        func: Option<unsafe extern "C" fn(*mut StData, *mut StData, StData, c_int) -> c_int>,
        arg: StData,
    ) -> c_int {
        let Some(func) = func else { return 0 };
        let ty = table_type(tbl);
        let found = with_store(tbl, |s| {
            unsafe { s.find(ty, key) }.map(|i| (i, s.rows[i].expect("a live row")))
        })
        .flatten();
        let (mut k, mut v) = found.map_or((key, 0), |(_, row)| row);
        let existing = c_int::from(found.is_some());
        // SAFETY: the caller's own callback, on words it may rewrite.
        let verdict = unsafe { func(&raw mut k, &raw mut v, arg, existing) };
        match (verdict, found) {
            (ST_DELETE, Some((i, _))) => {
                with_store(tbl, |s| s.remove_at(i));
            }
            (ST_CONTINUE, Some((i, _))) => {
                with_store(tbl, |s| s.rows[i] = Some((k, v)));
            }
            (ST_CONTINUE, None) => {
                with_store(tbl, |s| unsafe { s.add(ty, k, v) });
            }
            _ => {}
        }
        sync(tbl);
        existing
    }

    /// `st_keys(tbl, buf, n)`: the first `n` keys in insertion order.
    /// Answers how many were written.
    fn rb_st_keys(tbl: *mut StTable, buf: *mut StData, max: StIndex) -> StIndex {
        write_out(tbl, buf, max, |(_, k, _)| *k)
    }

    fn rb_st_keys_check(
        tbl: *mut StTable,
        buf: *mut StData,
        max: StIndex,
        _never: StData,
    ) -> StIndex {
        write_out(tbl, buf, max, |(_, k, _)| *k)
    }

    fn rb_st_values(tbl: *mut StTable, buf: *mut StData, max: StIndex) -> StIndex {
        write_out(tbl, buf, max, |(_, _, v)| *v)
    }

    fn rb_st_values_check(
        tbl: *mut StTable,
        buf: *mut StData,
        max: StIndex,
        _never: StData,
    ) -> StIndex {
        write_out(tbl, buf, max, |(_, _, v)| *v)
    }

    // ---- the hash primitives -------------------------------------------

    fn rb_st_numhash(n: StData) -> StIndex {
        num_hash(n)
    }

    /// MRI's `st_numcmp` answers `a != b`, so 0 IS equal -- the `strcmp`
    /// convention, and the opposite of a predicate.
    fn rb_st_numcmp(a: StData, b: StData) -> c_int {
        c_int::from(a != b)
    }

    fn rb_st_hash(p: *const c_void, len: usize, seed: StIndex) -> StIndex {
        if p.is_null() {
            return seed;
        }
        // SAFETY: the caller promised `len` readable bytes.
        let bytes = unsafe { std::slice::from_raw_parts(p.cast::<u8>(), len) };
        bytes_hash(bytes) ^ seed
    }

    fn rb_st_hash_start(h: StIndex) -> StIndex {
        h
    }

    fn rb_st_hash_end(h: StIndex) -> StIndex {
        h
    }

    fn rb_st_hash_uint(h: StIndex, n: StIndex) -> StIndex {
        (h ^ num_hash(n)).wrapping_mul(0x100_0000_01b3)
    }

    fn rb_st_hash_uint32(h: StIndex, n: u32) -> StIndex {
        (h ^ num_hash(n as StIndex)).wrapping_mul(0x100_0000_01b3)
    }

    /// `rb_memhash` and `rb_hash_start` are the same family, with the same
    /// caveat: MRI seeds them randomly per process, so no extension can
    /// depend on the value -- only on equal bytes hashing equally.
    fn rb_memhash(p: *const c_void, len: isize) -> StIndex {
        unsafe { rb_st_hash(p, len.max(0) as usize, 0) }
    }

    fn rb_hash_start(h: StIndex) -> StIndex {
        h
    }

    /// `st_locale_insensitive_strcasecmp`: ASCII case folding only, which is
    /// what "locale insensitive" means and why MRI has its own rather than
    /// calling `strcasecmp`.
    fn rb_st_locale_insensitive_strcasecmp(a: *const c_char, b: *const c_char) -> c_int {
        casecmp(unsafe { key_bytes(a as StData) }, unsafe {
            key_bytes(b as StData)
        })
    }

    fn rb_st_locale_insensitive_strncasecmp(
        a: *const c_char,
        b: *const c_char,
        n: usize,
    ) -> c_int {
        let (x, y) = unsafe { (key_bytes(a as StData), key_bytes(b as StData)) };
        casecmp(&x[..n.min(x.len())], &y[..n.min(y.len())])
    }
}

/// Every live row, as raw words. `rb_mark_tbl` and its neighbours read a
/// table whose keys or values are `VALUE`s, and this is the only way in.
pub(super) fn rows_of(tbl: *mut StTable) -> Vec<(StData, StData)> {
    with_store(tbl, |s| s.rows.iter().flatten().copied().collect()).unwrap_or_default()
}

fn write_out(
    tbl: *mut StTable,
    buf: *mut StData,
    max: StIndex,
    pick: impl Fn(&(usize, StData, StData)) -> StData,
) -> StIndex {
    if buf.is_null() {
        return 0;
    }
    let rows = with_store(tbl, |s| s.snapshot()).unwrap_or_default();
    let n = rows.len().min(max);
    for (i, row) in rows.iter().take(n).enumerate() {
        // SAFETY: the caller promised `max` writable words.
        unsafe { buf.add(i).write(pick(row)) };
    }
    n
}

/// ASCII-only ordering, sign-compatible with `strcmp`.
fn casecmp(a: &[u8], b: &[u8]) -> c_int {
    for (x, y) in a.iter().zip(b) {
        let (x, y) = (x.to_ascii_lowercase(), y.to_ascii_lowercase());
        if x != y {
            return c_int::from(x) - c_int::from(y);
        }
    }
    (a.len() as c_int) - (b.len() as c_int)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The struct is public in `st.h`, so a field in the wrong order or of
    /// the wrong width is a silently wrong `tbl->num_entries` read. Reading
    /// the header back is the only check that survives a re-vendor.
    #[test]
    fn the_header_matches_the_vendored_struct() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/cext/include/ruby/st.h");
        let text = std::fs::read_to_string(path).expect("the vendored header is present");
        let body = text
            .split("struct st_table {")
            .nth(1)
            .and_then(|s| s.split("};").next())
            .expect("st.h still defines struct st_table");
        // The fields, in the order this module lays them out.
        let want = [
            "entry_power",
            "bin_power",
            "size_ind",
            "rebuilds_num",
            "type",
            "num_entries",
            "bins",
            "entries_start",
            "entries_bound",
            "entries",
        ];
        let mut at = 0;
        for field in want {
            let found = body[at..]
                .find(field)
                .unwrap_or_else(|| panic!("st_table has no `{field}` after the previous field"));
            at += found + field.len();
        }
    }

    fn drop_table(t: *mut StTable) {
        unsafe { rb_st_free_table(t) };
    }

    #[test]
    fn a_numtable_inserts_looks_up_and_deletes() {
        let t = unsafe { rb_st_init_numtable() };
        assert_eq!(unsafe { rb_st_insert(t, 1, 100) }, 0, "a new key answers 0");
        assert_eq!(unsafe { rb_st_insert(t, 2, 200) }, 0);
        assert_eq!(
            unsafe { rb_st_insert(t, 1, 111) },
            1,
            "a known key answers 1"
        );
        assert_eq!(unsafe { rb_st_table_size(t) }, 2);
        // The header's own field, which an extension reads directly.
        assert_eq!(unsafe { (*t).num_entries }, 2);

        let mut out = 0;
        assert_eq!(unsafe { rb_st_lookup(t, 1, &raw mut out) }, 1);
        assert_eq!(out, 111, "the second insert replaced the value");
        assert_eq!(unsafe { rb_st_lookup(t, 9, &raw mut out) }, 0);

        let (mut k, mut v) = (2, 0);
        assert_eq!(unsafe { rb_st_delete(t, &raw mut k, &raw mut v) }, 1);
        assert_eq!(v, 200);
        assert_eq!(unsafe { rb_st_table_size(t) }, 1);
        assert_eq!(unsafe { (*t).num_entries }, 1);
        drop_table(t);
    }

    /// `st_foreach` walks in insertion order, and a deletion must not
    /// reorder what is left -- that is what makes Ruby's Hash ordered.
    #[test]
    fn a_walk_keeps_insertion_order_across_a_deletion() {
        let t = unsafe { rb_st_init_numtable() };
        for i in 1..=5 {
            unsafe { rb_st_insert(t, i, i * 10) };
        }
        let (mut k, mut v) = (3, 0);
        unsafe { rb_st_delete(t, &raw mut k, &raw mut v) };

        let mut keys = [0usize; 8];
        let n = unsafe { rb_st_keys(t, keys.as_mut_ptr(), 8) };
        assert_eq!(&keys[..n], &[1, 2, 4, 5]);

        let mut vals = [0usize; 8];
        let n = unsafe { rb_st_values(t, vals.as_mut_ptr(), 8) };
        assert_eq!(&vals[..n], &[10, 20, 40, 50]);
        drop_table(t);
    }

    /// `st_shift` takes the oldest, which only means anything if the holes
    /// a deletion leaves do not move the survivors.
    #[test]
    fn shift_takes_the_oldest_row() {
        let t = unsafe { rb_st_init_numtable() };
        unsafe { rb_st_insert(t, 7, 70) };
        unsafe { rb_st_insert(t, 8, 80) };
        let (mut k, mut v) = (0, 0);
        assert_eq!(unsafe { rb_st_shift(t, &raw mut k, &raw mut v) }, 1);
        assert_eq!((k, v), (7, 70));
        assert_eq!(unsafe { rb_st_shift(t, &raw mut k, &raw mut v) }, 1);
        assert_eq!((k, v), (8, 80));
        assert_eq!(unsafe { rb_st_shift(t, &raw mut k, &raw mut v) }, 0);
        drop_table(t);
    }

    #[test]
    fn a_strtable_keys_on_the_bytes_and_a_strcasetable_ignores_case() {
        let lower = c"key";
        let upper = c"KEY";
        let t = unsafe { rb_st_init_strtable() };
        unsafe { rb_st_insert(t, lower.as_ptr() as StData, 1) };
        let mut out = 0;
        assert_eq!(
            unsafe { rb_st_lookup(t, upper.as_ptr() as StData, &raw mut out) },
            0
        );
        // A DIFFERENT pointer with the same bytes still finds it.
        let same = c"key";
        assert_eq!(
            unsafe { rb_st_lookup(t, same.as_ptr() as StData, &raw mut out) },
            1
        );
        drop_table(t);

        let t = unsafe { rb_st_init_strcasetable() };
        unsafe { rb_st_insert(t, lower.as_ptr() as StData, 1) };
        assert_eq!(
            unsafe { rb_st_lookup(t, upper.as_ptr() as StData, &raw mut out) },
            1
        );
        drop_table(t);
    }

    /// `st_get_key` answers the STORED key, which is the point: a case table
    /// finds a row through `MIXED` and hands back the `Mixed` it holds.
    #[test]
    fn get_key_answers_the_stored_key() {
        let t = unsafe { rb_st_init_strcasetable() };
        let stored = c"Mixed";
        unsafe { rb_st_insert(t, stored.as_ptr() as StData, 1) };
        let mut out = 0;
        let asked = c"MIXED";
        assert_eq!(
            unsafe { rb_st_get_key(t, asked.as_ptr() as StData, &raw mut out) },
            1
        );
        assert_eq!(out, stored.as_ptr() as StData);
        drop_table(t);
    }

    #[test]
    fn a_copy_is_independent_and_keeps_the_order() {
        let t = unsafe { rb_st_init_numtable() };
        for i in 1..=3 {
            unsafe { rb_st_insert(t, i, i) };
        }
        let c = unsafe { rb_st_copy(t) };
        unsafe { rb_st_insert(t, 4, 4) };
        assert_eq!(
            unsafe { rb_st_table_size(c) },
            3,
            "the copy followed the original"
        );
        let mut keys = [0usize; 4];
        let n = unsafe { rb_st_keys(c, keys.as_mut_ptr(), 4) };
        assert_eq!(&keys[..n], &[1, 2, 3]);
        drop_table(t);
        drop_table(c);
    }

    /// `st_numcmp` is `strcmp`-shaped: 0 is EQUAL. A predicate would make
    /// every custom table's lookups answer backwards.
    #[test]
    fn numcmp_answers_zero_for_equal() {
        assert_eq!(unsafe { rb_st_numcmp(5, 5) }, 0);
        assert_ne!(unsafe { rb_st_numcmp(5, 6) }, 0);
    }

    #[test]
    fn casecmp_orders_like_strcmp() {
        assert_eq!(casecmp(b"abc", b"ABC"), 0);
        assert!(casecmp(b"abc", b"abd") < 0);
        assert!(casecmp(b"abd", b"abc") > 0);
        assert!(casecmp(b"ab", b"abc") < 0);
        assert!(casecmp(b"abc", b"ab") > 0);
    }

    /// A freed table's address may be handed back by the allocator, so the
    /// store has to go with it -- otherwise a new table would be born
    /// holding the old one's rows.
    #[test]
    fn freeing_a_table_forgets_its_rows() {
        let t = unsafe { rb_st_init_numtable() };
        unsafe { rb_st_insert(t, 1, 1) };
        let addr = t as usize;
        drop_table(t);
        let held = STORES
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|m| m.contains_key(&addr)));
        assert_eq!(held, Some(false), "the store outlived the table");
    }

    /// `st_update` is the only entry that inserts, updates and deletes, and
    /// it decides from the callback's answer rather than the caller's.
    #[test]
    fn update_inserts_then_replaces_then_deletes() {
        unsafe extern "C" fn set_to(
            _k: *mut StData,
            v: *mut StData,
            arg: StData,
            _existing: c_int,
        ) -> c_int {
            unsafe { v.write(arg) };
            ST_CONTINUE
        }
        unsafe extern "C" fn drop_it(
            _k: *mut StData,
            _v: *mut StData,
            _arg: StData,
            _existing: c_int,
        ) -> c_int {
            ST_DELETE
        }
        let t = unsafe { rb_st_init_numtable() };
        assert_eq!(unsafe { rb_st_update(t, 1, Some(set_to), 10) }, 0, "absent");
        assert_eq!(
            unsafe { rb_st_update(t, 1, Some(set_to), 20) },
            1,
            "present"
        );
        let mut out = 0;
        unsafe { rb_st_lookup(t, 1, &raw mut out) };
        assert_eq!(out, 20);
        assert_eq!(unsafe { rb_st_update(t, 1, Some(drop_it), 0) }, 1);
        assert_eq!(unsafe { rb_st_table_size(t) }, 0);
        drop_table(t);
    }

    /// A callback that deletes during the walk must not derail it, and
    /// `ST_STOP` must actually stop.
    #[test]
    fn a_walk_honours_delete_and_stop() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static SEEN: AtomicUsize = AtomicUsize::new(0);
        unsafe extern "C" fn drop_evens(k: StData, _v: StData, _a: StData) -> c_int {
            SEEN.fetch_add(1, Ordering::Relaxed);
            if k.is_multiple_of(2) {
                ST_DELETE
            } else {
                ST_CONTINUE
            }
        }
        unsafe extern "C" fn stop_at_two(_k: StData, _v: StData, _a: StData) -> c_int {
            if SEEN.fetch_add(1, Ordering::Relaxed) >= 1 {
                ST_STOP
            } else {
                ST_CONTINUE
            }
        }
        let t = unsafe { rb_st_init_numtable() };
        for i in 1..=4 {
            unsafe { rb_st_insert(t, i, i) };
        }
        SEEN.store(0, Ordering::Relaxed);
        unsafe { rb_st_foreach(t, Some(drop_evens), 0) };
        assert_eq!(SEEN.load(Ordering::Relaxed), 4, "the walk saw every row");
        assert_eq!(unsafe { rb_st_table_size(t) }, 2);

        SEEN.store(0, Ordering::Relaxed);
        unsafe { rb_st_foreach(t, Some(stop_at_two), 0) };
        assert_eq!(SEEN.load(Ordering::Relaxed), 2, "ST_STOP did not stop it");
        drop_table(t);
    }
}

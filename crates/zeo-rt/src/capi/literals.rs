//! Literal construction: strings (frozen-interned and fresh), symbols,
//! arrays, hashes, ranges.

use super::dispatch::status_out;
use crate::encoding::{EncodingId, StrBuf};
use crate::{RubyValue, Symbol};

/// A frozen, interned string literal -- one object per `(bytes, encoding)`
/// process-wide, exactly the `LitPool` identity rule.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_str_lit(ptr: *const u8, len: usize, enc: u8, out: *mut RubyValue) {
    let bytes = unsafe { super::byte_slice(ptr, len) }.to_vec();
    let buf = StrBuf::from_bytes(bytes, EncodingId(enc));
    let v = RubyValue::Str(crate::value::collections::intern_frozen(buf));
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// A FRESH, mutable string from literal bytes -- every evaluation of a
/// non-frozen literal allocates, as ruby's do.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_str_new(ptr: *const u8, len: usize, enc: u8, out: *mut RubyValue) {
    let bytes = unsafe { super::byte_slice(ptr, len) }.to_vec();
    let buf = StrBuf::from_bytes(bytes, EncodingId(enc));
    let v = RubyValue::Str(std::sync::Arc::new(crate::collections::Freezable::new(buf)));
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// A fresh, mutable string BORROWING its literal bytes -- the non-frozen
/// literal's constructor. No byte copy at all until a mutation promotes
/// the buffer (see `StrBuf::from_static`); each evaluation still mints a
/// distinct object, as ruby's literals do.
///
/// # Safety
/// `ptr` must point at bytes that live for the whole process: `.rodata`
/// on the AOT path, and JIT/eval unit memory (never unloaded -- the
/// `EvalProgram` process-lifetime contract) on the JIT path.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_str_lit_ro(
    ptr: *const u8,
    len: usize,
    enc: u8,
    out: *mut RubyValue,
) {
    let bytes: &'static [u8] = if len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(ptr, len) }
    };
    let buf = StrBuf::from_static(bytes, EncodingId(enc));
    let v = RubyValue::Str(std::sync::Arc::new(crate::collections::Freezable::new(buf)));
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// Intern a symbol name -- `zeo_unit_init` fills the program's `zeo_syms`
/// table through this.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_sym_intern(ptr: *const u8, len: usize) -> u32 {
    Symbol::intern(unsafe { super::str_slice(ptr, len) }).to_u32()
}

/// Intern the unit's whole symbol table in one call: `rows` is a rodata
/// `Str` table, `out` the `.bss` `zeo_syms` id array `zeo_unit_init` used
/// to fill with one `zeo_rt_sym_intern` call per name.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_syms_init(rows: *const zeo_abi::abi::Str, n: usize, out: *mut u32) {
    for i in 0..n {
        let row = unsafe { rows.add(i).read() };
        let id = Symbol::intern(unsafe { super::str_slice(row.ptr, row.len) }).to_u32();
        unsafe { out.add(i).write(id) };
    }
}

/// A `Symbol` value from its interned id.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_sym_value(id: u32, out: *mut RubyValue) {
    unsafe { out.write(RubyValue::Symbol(Symbol::from_u32(id))) };
}

/// A fresh array with room for `cap` elements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_array_new(cap: usize, out: *mut RubyValue) {
    let v = RubyValue::Array(crate::value::collections::array_new(Vec::with_capacity(
        cap,
    )));
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// Push the value moved from `v` -- literal construction's append (the
/// receiver is the fresh, unfrozen literal, so no frozen check).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_array_push(a: *const RubyValue, v: *mut RubyValue) {
    super::leakcheck::consumed(unsafe { &*v });
    let value = unsafe { std::ptr::read(v) };
    match unsafe { &*a } {
        RubyValue::Array(arr) => {
            crate::array_push(arr, value);
        }
        other => panic!("zeo_rt_array_push on a non-array: {other:?}"),
    }
}

/// The array's length (a live view, for fused iteration).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_array_len(a: *const RubyValue) -> usize {
    match unsafe { &*a } {
        RubyValue::Array(arr) => arr.lock().len(),
        other => panic!("zeo_rt_array_len on a non-array: {other:?}"),
    }
}

/// `Array#[]` with a proven-Int index: ruby's own negative-from-end and
/// out-of-range->nil (`array_get`). Infallible; emitted only under the
/// emitter's Array-tag + Int-tag + gate guard.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_array_aref_int(a: *const RubyValue, i: i64, out: *mut RubyValue) {
    let v = match unsafe { &*a } {
        RubyValue::Array(arr) => crate::array_get(arr, i),
        other => panic!("zeo_rt_array_aref_int on a non-array: {other:?}"),
    };
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// `Array#[]=` with a proven-Int index: the row's scalar branch (frozen
/// check first, nil-pad growth, the too-small IndexError). `v` is
/// BORROWED; the expression's value (`v` itself) writes to `out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_array_aset_int(
    a: *const RubyValue,
    i: i64,
    v: *const RubyValue,
    out: *mut RubyValue,
) -> i32 {
    let (recv, value) = unsafe { (&*a, &*v) };
    let r = match recv {
        RubyValue::Array(arr) => {
            crate::builtins::array::array_aset_int_checked(arr, recv, i, value)
        }
        other => panic!("zeo_rt_array_aset_int on a non-array: {other:?}"),
    };
    status_out(r, out)
}

/// The fused `sum` loop's state beside its RubyValue accumulator: the
/// Kahan-Babuska compensation for the float lane, and the sticky-generic
/// flag -- [`SumAcc::Generic`] never leaves the generic lane, which the
/// value alone cannot encode (a generic `+` may answer a plain Int).
/// Zero bytes = the starting Int lane, so the emitter zero-inits it.
#[repr(C)]
pub struct SumState {
    comp: f64,
    generic: u8,
}

use crate::builtins::enumerable::SumAcc;

fn sum_acc_of(v: RubyValue, st: &SumState) -> SumAcc {
    match (st.generic, v) {
        (0, RubyValue::Int(n)) => SumAcc::Int(n),
        (0, RubyValue::Float(f)) => SumAcc::Float {
            sum: f,
            compensation: st.comp,
        },
        (_, other) => SumAcc::Generic(other),
    }
}

/// One step of the fused `arr.sum { .. }` loop: fold the block value
/// (MOVED from `v`) into the accumulator through the very [`SumAcc`]
/// ladder `Enumerable#sum` uses -- one definition of the arithmetic,
/// compensation and all. On an error (a generic `+` raised) the
/// accumulator slot is left untouched.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_sum_step(
    acc: *mut RubyValue,
    st: *mut SumState,
    v: *mut RubyValue,
) -> i32 {
    super::leakcheck::consumed(unsafe { &*v });
    let val = unsafe { std::ptr::read(v) };
    let state = unsafe { &mut *st };
    // Clone, not move: `add` consumes its receiver, and the slot must
    // still hold a live value if the generic leg raises.
    let cur = sum_acc_of(unsafe { (*acc).clone() }, state);
    match cur.add(val) {
        Ok(next) => {
            super::leakcheck::consumed(unsafe { &*acc });
            unsafe { std::ptr::drop_in_place(acc) };
            let (value, comp, generic) = match next {
                SumAcc::Int(n) => (RubyValue::Int(n), 0.0, 0),
                SumAcc::Float { sum, compensation } => (RubyValue::Float(sum), compensation, 0),
                SumAcc::Generic(g) => (g, 0.0, 1),
            };
            super::leakcheck::created(&value);
            unsafe { acc.write(value) };
            state.comp = comp;
            state.generic = generic;
            zeo_abi::abi::STATUS_OK
        }
        Err(sig) => {
            crate::signal::set_pending(sig);
            zeo_abi::abi::STATUS_SIGNAL
        }
    }
}

/// Finish the fused `sum`: the accumulator (MOVED from `acc`, which is
/// nil'd for the epilogue's unconditional release) becomes the loop's
/// value -- the float lane folds its compensation in here, CRuby's rule.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_sum_finish(
    acc: *mut RubyValue,
    st: *const SumState,
    out: *mut RubyValue,
) {
    super::leakcheck::consumed(unsafe { &*acc });
    let cur = unsafe { std::ptr::read(acc) };
    unsafe { acc.write(RubyValue::Nil) };
    let v = sum_acc_of(cur, unsafe { &*st }).finish();
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// One element, cloned out (`nil` past the end -- Ruby's `[]`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_array_get(a: *const RubyValue, i: usize, out: *mut RubyValue) {
    let v = match unsafe { &*a } {
        RubyValue::Array(arr) => arr.lock().get(i).cloned().unwrap_or(RubyValue::Nil),
        other => panic!("zeo_rt_array_get on a non-array: {other:?}"),
    };
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// A fresh empty hash.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_hash_new(out: *mut RubyValue) {
    let v = RubyValue::Hash(crate::value::collections::hash_new(Vec::new()));
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

/// Store `k => v` (both moved) -- literal construction's insert. Fallible
/// because the key's own `hash` is user code: it can raise, and then the
/// literal raises rather than storing a key nothing could look up.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_hash_set(
    h: *const RubyValue,
    k: *mut RubyValue,
    v: *mut RubyValue,
) -> i32 {
    use zeo_abi::abi::{STATUS_OK, STATUS_SIGNAL};
    super::leakcheck::consumed(unsafe { &*k });
    super::leakcheck::consumed(unsafe { &*v });
    let key = unsafe { std::ptr::read(k) };
    let value = unsafe { std::ptr::read(v) };
    match unsafe { &*h } {
        RubyValue::Hash(hash) => {
            crate::value::collections::hash_set(hash, key, value);
        }
        other => panic!("zeo_rt_hash_set on a non-hash: {other:?}"),
    }
    match crate::value::collections::take_key_raise() {
        Some(sig) => {
            crate::signal::set_pending(sig);
            STATUS_SIGNAL
        }
        None => STATUS_OK,
    }
}

/// A range literal (`a..b` / `a...b`); endpoints moved (nil = open). The
/// comparability check can raise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_range_new(
    begin: *mut RubyValue,
    end: *mut RubyValue,
    exclusive: i8,
    out: *mut RubyValue,
) -> i32 {
    let ep = |p: *mut RubyValue| {
        if p.is_null() {
            None
        } else {
            super::leakcheck::consumed(unsafe { &*p });
            match unsafe { std::ptr::read(p) } {
                RubyValue::Nil => None,
                v => Some(v),
            }
        }
    };
    status_out(
        crate::value::range_checked(ep(begin), ep(end), exclusive != 0),
        out,
    )
}

/// A multiple assignment's split: destructure `value` against a
/// `before/splat/after` target shape into `out`'s
/// `n_before + has_splat + n_after` slots (each OWNED; the splat slot, when
/// present, gets a fresh Array). `value` coerces exactly as the rustc
/// emission does -- an Array destructures directly, anything else goes
/// through the `to_ary` rule ([`crate::block_auto_splat`], which can
/// raise). Slots are nil-filled BEFORE the coercion, so an error path
/// releases safely.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_multi_split(
    value: *const RubyValue,
    n_before: usize,
    has_splat: u8,
    n_after: usize,
    out: *mut RubyValue,
) -> i32 {
    use zeo_abi::abi::{STATUS_OK, STATUS_SIGNAL};
    let has_splat = has_splat != 0;
    let n_out = n_before + usize::from(has_splat) + n_after;
    for i in 0..n_out {
        unsafe { out.add(i).write(RubyValue::Nil) };
    }
    let v = unsafe { &*value };
    let elems: Vec<RubyValue> = match v {
        RubyValue::Array(a) => a.lock().iter().cloned().collect(),
        other => match crate::block_auto_splat(std::slice::from_ref(other)) {
            Ok(cow) => cow.into_owned(),
            Err(sig) => {
                crate::signal::set_pending(sig);
                return STATUS_SIGNAL;
            }
        },
    };
    let (before, splat, after) =
        crate::value::collections::multi_assign(&elems, n_before, has_splat, n_after);
    let mut s = 0usize;
    let mut put = |v: RubyValue| {
        super::leakcheck::created(&v);
        unsafe { out.add(s).write(v) };
        s += 1;
    };
    for v in before {
        put(v);
    }
    if has_splat {
        put(RubyValue::Array(crate::value::collections::array_new(
            splat,
        )));
    }
    for v in after {
        put(v);
    }
    debug_assert_eq!(s, n_out);
    STATUS_OK
}

/// Append literal UTF-8 bytes to the (mutable) string being interpolated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_str_append_lit(s: *const RubyValue, ptr: *const u8, len: usize) {
    let RubyValue::Str(rs) = (unsafe { &*s }) else {
        panic!("str_append_lit on a non-string")
    };
    let text = unsafe { super::str_slice(ptr, len) };
    rs.lock().push_str(text);
}

/// Append a RAW-BYTE literal segment (a `\xNN` escape): the bytes go in
/// untouched, so an invalid sequence stays invalid exactly as CRuby's
/// does.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_str_append_bytes(s: *const RubyValue, ptr: *const u8, len: usize) {
    let RubyValue::Str(rs) = (unsafe { &*s }) else {
        panic!("str_append_bytes on a non-string")
    };
    let bytes = unsafe { super::byte_slice(ptr, len) };
    rs.lock().push_bytes(bytes);
}

/// Append an interpolated value: `try_display_string` dispatches a
/// user-defined `to_s`, whose raise propagates (catchable at the
/// interpolation site, CRuby's rule).
///
/// A String argument goes in as BYTES through `push_buf`, so the result's
/// encoding is negotiated the way `+` and `Array#join` negotiate it and an
/// incompatible pair raises. Rendering it as text instead tagged every
/// interpolation UTF-8, so `"#{latin1}"` came back re-encoded and
/// `"#{utf8}#{latin1}"` silently produced mojibake where ruby raises.
///
/// A non-String `to_s` result still goes in as text. Every renderer zeo has
/// answers UTF-8 or ASCII, so only a user `to_s` returning a differently
/// encoded String is left flattened -- `display_with` flattens it first.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_str_append_value(s: *const RubyValue, v: *const RubyValue) -> i32 {
    use zeo_abi::abi::{STATUS_OK, STATUS_SIGNAL};
    let RubyValue::Str(rs) = (unsafe { &*s }) else {
        panic!("str_append_value on a non-string")
    };
    let fail = |sig| {
        crate::signal::set_pending(sig);
        STATUS_SIGNAL
    };
    if let RubyValue::Str(other) = unsafe { &*v } {
        // The source is copied out and its guard ended before the destination
        // is locked -- for the same-object case (`s = "x"; s << "#{s}"`),
        // where the two locks are one, and equally for two distinct strings,
        // where holding both would let `a << b` on one thread and `b << a` on
        // another take them in opposite orders.
        let copy = other.lock().clone();
        let pushed = rs.lock().push_buf(&copy);
        return match pushed {
            Ok(()) => STATUS_OK,
            Err(_) => {
                let left = rs.lock().clone();
                fail(crate::builtins::string::encode::concat_incompat(
                    &left, &copy,
                ))
            }
        };
    }
    match unsafe { &*v }.try_display_string() {
        Ok(text) => {
            rs.lock().push_str(&text);
            STATUS_OK
        }
        Err(sig) => fail(sig),
    }
}

/// Where a run-time `eval`'s regexp-literal site ids begin, and the same
/// hazard `flipflop::EVAL_BASE` names: a program's sites are dense from zero
/// and minted by ONE compile, and a snippet's fresh compiler starts at zero
/// again. Sharing the space made a snippet's literal answer the program's
/// cached regexp -- a silently wrong match, not an error.
const EVAL_SITE_BASE: u32 = zeo_abi::abi::EVAL_SITE_BASE;

static NEXT_EVAL_SITE: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(EVAL_SITE_BASE);

/// Reserve `n` consecutive regexp-literal site ids for one compiled snippet.
pub fn reserve_regexp_sites(n: u32) -> u32 {
    NEXT_EVAL_SITE.fetch_add(n, std::sync::atomic::Ordering::Relaxed)
}

/// A NON-INTERPOLATED regexp literal: ONE frozen object per SITE, keyed
/// by the emitter-assigned site id. A bad pattern raises `RegexpError`
/// with the backtrace stamped at the literal (no cause chaining; that is
/// `raise`'s own semantics).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_regexp_lit(
    site: u32,
    ptr: *const u8,
    len: usize,
    ignore_case: i8,
    extended: i8,
    multiline: i8,
    enc: u8,
    out: *mut RubyValue,
) -> i32 {
    use std::sync::{Mutex, OnceLock};
    use zeo_abi::abi::{STATUS_OK, STATUS_SIGNAL};
    static SITES: OnceLock<Mutex<crate::FMap<u32, crate::regexp::RRegexp>>> = OnceLock::new();
    let sites = SITES.get_or_init(|| Mutex::new(crate::FMap::default()));
    if let Some(re) = sites.lock().unwrap().get(&site) {
        let v = RubyValue::Regexp(re.clone());
        super::leakcheck::created(&v);
        unsafe { out.write(v) };
        return STATUS_OK;
    }
    let source = unsafe { super::str_slice(ptr, len) };
    match crate::regexp::regexp_new_enc(
        source,
        ignore_case != 0,
        extended != 0,
        multiline != 0,
        regexp_encoding(enc),
    ) {
        Ok(re) => {
            // Frozen BEFORE publishing, the literal rule.
            re.set_frozen();
            sites.lock().unwrap().insert(site, re.clone());
            let v = RubyValue::Regexp(re);
            super::leakcheck::created(&v);
            unsafe { out.write(v) };
            STATUS_OK
        }
        Err(msg) => {
            crate::signal::set_pending(regexp_error(msg));
            STATUS_SIGNAL
        }
    }
}

/// An INTERPOLATED regexp literal, built from the pattern string the
/// emitter assembled (BORROWED). Frozen at birth -- real Ruby since 3.0,
/// interpolated ones included; `Regexp.new` stays unfrozen by not passing
/// through here.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_regexp_interp(
    pat: *const RubyValue,
    ignore_case: i8,
    extended: i8,
    multiline: i8,
    enc: u8,
    out: *mut RubyValue,
) -> i32 {
    use zeo_abi::abi::{STATUS_OK, STATUS_SIGNAL};
    let RubyValue::Str(s) = (unsafe { &*pat }) else {
        panic!("regexp_interp on a non-string pattern")
    };
    let source = s.lock().to_utf8_lossy().into_owned();
    match crate::regexp::regexp_new_enc(
        &source,
        ignore_case != 0,
        extended != 0,
        multiline != 0,
        regexp_encoding(enc),
    ) {
        Ok(re) => {
            re.set_frozen();
            let v = RubyValue::Regexp(re);
            super::leakcheck::created(&v);
            unsafe { out.write(v) };
            STATUS_OK
        }
        Err(msg) => {
            crate::signal::set_pending(regexp_error(msg));
            STATUS_SIGNAL
        }
    }
}

/// The `RegexpError` raise both wrappers share: construct + stamp the
/// backtrace at the literal, no cause chaining (rustc's `emit_boxed_new`).
fn regexp_error(msg: String) -> crate::Signal {
    let exc = crate::dispatch::construct_exception_value("RegexpError", &msg);
    crate::builtins::exception::attach_backtrace(&exc);
    crate::Signal::Raise(exc)
}

/// The wire byte for [`zeo_abi::RegexpEncoding`].
fn regexp_encoding(b: u8) -> zeo_abi::RegexpEncoding {
    match b {
        0 => zeo_abi::RegexpEncoding::Source,
        1 => zeo_abi::RegexpEncoding::None,
        2 => zeo_abi::RegexpEncoding::EucJp,
        3 => zeo_abi::RegexpEncoding::Windows31j,
        4 => zeo_abi::RegexpEncoding::Utf8,
        other => panic!("regexp literal: unknown encoding byte {other}"),
    }
}

/// The `$~` family: `kind` selects Data(0) / Group(1, `n`) / Pre(2) /
/// Post(3) / LastGroup(4) -- one entry for the whole `last_match*` family.
/// Infallible; absent state answers nil.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_last_match_ref(kind: u8, n: usize, out: *mut RubyValue) {
    let v = match kind {
        0 => crate::lastmatch::last_match(),
        1 => crate::lastmatch::last_match_group(n),
        2 => crate::lastmatch::last_match_pre(),
        3 => crate::lastmatch::last_match_post(),
        4 => crate::lastmatch::last_match_last_group(),
        other => panic!("last_match_ref: unknown kind {other}"),
    };
    super::leakcheck::created(&v);
    unsafe { out.write(v) };
}

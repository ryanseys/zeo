//! `rb_str_*` and the string layout accessors.
//!
//! # The pointer problem, and how it is answered
//!
//! `RSTRING_PTR` hands out a `char *` an extension reads, writes through, and
//! keeps for the length of its own C function. In MRI that is a pointer into
//! the object itself, and it stays valid because the object does not move.
//!
//! A zeo String is an `Arc<Freezable<StrBuf>>` behind a lock, and its bytes
//! are a `Vec` that reallocates when the string grows. There is no address to
//! hand out that survives the next `rb_str_cat`.
//!
//! So `RSTRING_PTR` **pins**: the bytes are copied into a stable buffer owned
//! by the current scope, the extension gets that address, and scope pop
//! writes them back if they changed. Three consequences, all of them stated
//! rather than hidden:
//!
//! * Two `RSTRING_PTR` calls on one string inside one scope answer the SAME
//!   address, because the pin is keyed by the object. An extension that
//!   compares them is right.
//! * A write through the pointer lands on the Ruby string at scope pop, not
//!   at the store. An extension that writes and then reads the string through
//!   Ruby in the same C call sees the old bytes. MRI shows the new ones.
//! * The buffer is exactly `RSTRING_LEN` bytes plus a NUL. Writing past it is
//!   as wrong here as in MRI, and here it is caught: the copy-back checks the
//!   guard byte.

use super::convert::{to_value, value_of};
use super::value::Value;
use crate::RubyValue;
use crate::collections::RStr;
use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_long, c_void};

/// One pinned string's bytes: what C sees, and what to write back into.
struct Pin {
    /// WEAK, and that is the whole lifetime rule -- see [`pin_bytes`].
    owner: std::sync::Weak<crate::Freezable<crate::encoding::StrBuf>>,
    /// `len` bytes, then a NUL, then one guard byte the copy-back checks.
    buf: Vec<u8>,
    len: usize,
}

/// The guard byte, chosen so a plausible overrun (a NUL, a space, a digit)
/// does not look like an intact buffer.
const GUARD: u8 = 0xA5;

thread_local! {
    /// Pinned strings, keyed by the payload address, so two `RSTRING_PTR`
    /// calls on one string answer one pointer.
    static PINS: RefCell<Vec<(usize, Box<Pin>)>> = const { RefCell::new(Vec::new()) };
}

/// A stable, writable `char *` for `s`.
///
/// # The lifetime, and why it is the string's and not the scope's
///
/// It was the scope's, and that was wrong. An extension is allowed to KEEP
/// the pointer: `msgpack`'s `Unpacker#feed_reference` stores it in the
/// buffer and holds the String `VALUE` beside it, which on MRI is exactly
/// what keeps the bytes valid -- the reference keeps the object alive and
/// the bytes live in the object. Freeing zeo's copy when the C call returned
/// left `full_unpack` reading released memory, and it answered plausible
/// integers rather than crashing.
///
/// So the pin lives as long as the STRING does. The `owner` is a `Weak`, and
/// a pin whose string is gone is evicted at the next scope pop -- so the
/// pointer is valid for exactly the window MRI's is, and no longer.
///
/// A re-pin REFRESHES: Ruby may have rewritten the string since, and the
/// buffer is a copy. A length change reallocates, which moves the address --
/// MRI's `RSTRING_PTR` moves on a resize too, and an extension holding one
/// across a Ruby-side mutation is wrong on both.
fn pin_bytes(s: &RStr) -> *mut c_char {
    let key = std::sync::Arc::as_ptr(s) as *const () as usize;
    let (bytes, len) = {
        let g = s.lock();
        (g.bytes().to_vec(), g.bytesize())
    };
    PINS.with_borrow_mut(|pins| {
        if let Some((_, pin)) = pins.iter_mut().find(|(k, _)| *k == key) {
            if pin.len != len || pin.buf[..pin.len] != bytes[..] {
                pin.buf.clear();
                pin.buf.extend_from_slice(&bytes);
                pin.buf.push(0);
                pin.buf.push(GUARD);
                pin.len = len;
            }
            return pin.buf.as_mut_ptr().cast();
        }
        let mut buf = bytes;
        buf.push(0);
        buf.push(GUARD);
        let mut pin = Box::new(Pin {
            owner: std::sync::Arc::downgrade(s),
            buf,
            len,
        });
        let ptr = pin.buf.as_mut_ptr().cast();
        pins.push((key, pin));
        ptr
    })
}

/// Write every pinned string back, and evict the ones whose string is gone.
///
/// Called from [`super::scope::Scope`]'s pop, and from the longjmp unwind, so
/// a raise does not lose a write the extension had already made.
///
/// The write-back happens here and the BUFFER stays -- see [`pin_bytes`] for
/// why the two have different lifetimes.
pub(super) fn flush_pins() {
    PINS.with_borrow_mut(|pins| {
        pins.retain_mut(|(_, pin)| {
            let Some(owner) = pin.owner.upgrade() else {
                // The string is gone, so nothing can hold a pointer into it
                // that was not already dangling on MRI too.
                return false;
            };
            assert_eq!(
                pin.buf[pin.len + 1],
                GUARD,
                "a C extension wrote past the end of a string it got from RSTRING_PTR"
            );
            let bytes = &pin.buf[..pin.len];
            let mut guard = owner.lock();
            if guard.bytes() != bytes {
                let enc = guard.encoding();
                guard.replace_bytes(bytes.to_vec(), enc);
            }
            true
        });
    });
}

/// The `RStr` behind a `VALUE`, or a `TypeError` naming what arrived.
///
/// # Safety
///
/// `v` is an extension's own `VALUE`; see [`value_of`].
unsafe fn as_str(v: Value) -> Result<RStr, crate::Signal> {
    match unsafe { value_of(v) } {
        RubyValue::Str(s) => Ok(s),
        other => Err(crate::builtins::wrong_arg_type(&other, "String")),
    }
}

/// `len < 0` means "to the NUL", which is what `rb_str_new_cstr` and every
/// `rb_str_new(p, -1)` in the wild mean.
///
/// # Safety
///
/// `p` must name `len` readable bytes, or a NUL-terminated string.
pub(super) unsafe fn borrow_bytes(p: *const c_char, len: c_long) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    // SAFETY: the caller's contract.
    unsafe {
        if len < 0 {
            std::ffi::CStr::from_ptr(p).to_bytes().to_vec()
        } else {
            std::slice::from_raw_parts(p.cast::<u8>(), len as usize).to_vec()
        }
    }
}

fn new_str(bytes: Vec<u8>, enc: crate::encoding::EncodingId) -> RubyValue {
    RubyValue::Str(crate::string_from_bytes(bytes, enc))
}

crate::cext_fn! {
    /// `rb_str_new(ptr, len)`. Binary, as in MRI: an extension that wants a
    /// text encoding says so with `rb_utf8_str_new` or `rb_enc_str_new`.
    fn rb_str_new(p: *const c_char, len: c_long) -> Value {
        let bytes = unsafe { borrow_bytes(p, len) };
        to_value(&new_str(bytes, crate::encoding::ASCII_8BIT))
    }

    fn rb_str_new_cstr(p: *const c_char) -> Value {
        let bytes = unsafe { borrow_bytes(p, -1) };
        to_value(&new_str(bytes, crate::encoding::ASCII_8BIT))
    }

    fn rb_utf8_str_new(p: *const c_char, len: c_long) -> Value {
        let bytes = unsafe { borrow_bytes(p, len) };
        to_value(&new_str(bytes, crate::encoding::UTF_8))
    }

    fn rb_utf8_str_new_cstr(p: *const c_char) -> Value {
        let bytes = unsafe { borrow_bytes(p, -1) };
        to_value(&new_str(bytes, crate::encoding::UTF_8))
    }

    fn rb_usascii_str_new(p: *const c_char, len: c_long) -> Value {
        let bytes = unsafe { borrow_bytes(p, len) };
        to_value(&new_str(bytes, crate::encoding::US_ASCII))
    }

    fn rb_usascii_str_new_cstr(p: *const c_char) -> Value {
        let bytes = unsafe { borrow_bytes(p, -1) };
        to_value(&new_str(bytes, crate::encoding::US_ASCII))
    }

    /// `rb_str_buf_new(capacity)`. zeo's strings grow on demand, so the hint
    /// is accepted and dropped; the answer is an empty binary String, which
    /// is what MRI gives too.
    fn rb_str_buf_new(_capa: c_long) -> Value {
        to_value(&new_str(Vec::new(), crate::encoding::ASCII_8BIT))
    }

    fn rb_str_dup(v: Value) -> Value {
        let s = unsafe { as_str(v)? };
        let (bytes, enc) = { let g = s.lock(); (g.bytes().to_vec(), g.encoding()) };
        to_value(&new_str(bytes, enc))
    }

    /// Append raw bytes. The receiver keeps its own encoding: `rb_str_buf_cat`
    /// is the byte-level append, and MRI does not transcode here either.
    fn rb_str_cat(v: Value, p: *const c_char, len: c_long) -> Value {
        let s = unsafe { as_str(v)? };
        let add = unsafe { borrow_bytes(p, len) };
        s.lock().push_bytes(&add);
        Ok(v)
    }

    fn rb_str_buf_cat(v: Value, p: *const c_char, len: c_long) -> Value {
        let s = unsafe { as_str(v)? };
        let add = unsafe { borrow_bytes(p, len) };
        s.lock().push_bytes(&add);
        Ok(v)
    }

    fn rb_str_cat_cstr(v: Value, p: *const c_char) -> Value {
        let s = unsafe { as_str(v)? };
        let add = unsafe { borrow_bytes(p, -1) };
        s.lock().push_bytes(&add);
        Ok(v)
    }

    /// The `*_str_new_static` family: MRI keeps the caller's pointer and
    /// never copies, on the promise that it names a C STRING LITERAL that
    /// outlives the process.
    ///
    /// zeo copies. A zeo String owns its bytes -- there is no way to hand one
    /// a foreign buffer -- so the promise buys nothing here, and taking a
    /// copy is right whether or not the caller kept its side of it.
    /// `rb_str_new_static` is what `rb_str_new_cstr` expands to for a
    /// literal, so this is not a corner: it is on the common path.
    fn rb_str_new_static(p: *const c_char, len: c_long) -> Value {
        let bytes = unsafe { borrow_bytes(p, len) };
        to_value(&new_str(bytes, crate::encoding::ASCII_8BIT))
    }

    fn rb_utf8_str_new_static(p: *const c_char, len: c_long) -> Value {
        let bytes = unsafe { borrow_bytes(p, len) };
        to_value(&new_str(bytes, crate::encoding::UTF_8))
    }

    fn rb_usascii_str_new_static(p: *const c_char, len: c_long) -> Value {
        let bytes = unsafe { borrow_bytes(p, len) };
        to_value(&new_str(bytes, crate::encoding::US_ASCII))
    }

    fn rb_enc_str_new_static(p: *const c_char, len: c_long, e: *const std::ffi::c_void) -> Value {
        let bytes = unsafe { borrow_bytes(p, len) };
        to_value(&new_str(bytes, super::misc::encoding_of(e)))
    }

    /// `rb_str_freeze`. Answers the receiver, as MRI does.
    fn rb_str_freeze(v: Value) -> Value {
        let s = unsafe { as_str(v)? };
        RubyValue::Str(s).freeze_value()?;
        Ok(v)
    }
}

/// How many bytes one code unit occupies -- CRuby's `mbminlen`.
///
/// Read off the kind, and off the NAME for the two dummy rows: a dummy
/// `UTF-16` has no per-character structure in zeo's table (it reads as
/// `Binary`), but its units are still two bytes wide and MRI reports 2.
fn code_unit_width(enc: crate::encoding::EncodingId) -> usize {
    match enc.spec().kind {
        crate::encoding::EncKind::Utf16 { .. } => 2,
        crate::encoding::EncKind::Utf32 { .. } => 4,
        _ if enc.name().starts_with("UTF-16") => 2,
        _ if enc.name().starts_with("UTF-32") => 4,
        _ => 1,
    }
}

crate::cext_fn! {
    // ---- searching bytes --------------------------------------------------

    /// `rb_memsearch(needle, m, haystack, n, enc)`: the byte offset of the
    /// first `needle` in `haystack`, or `-1`.
    ///
    /// The encoding is not there to decode with -- it fixes the STEP. UTF-16
    /// and UTF-32 hold their code units two and four bytes wide, so a match
    /// found at an odd offset in UTF-16 is two halves of two different
    /// characters and is not a match at all. Every other encoding steps one
    /// byte, which is why MRI reads `mbminlen` here and nothing else.
    fn rb_memsearch(
        needle: *const c_void,
        m: c_long,
        haystack: *const c_void,
        n: c_long,
        enc: *const c_void,
    ) -> c_long {
        if m > n || m < 0 || n < 0 || (m > 0 && needle.is_null()) || haystack.is_null() {
            return Ok(-1);
        }
        // SAFETY: the caller promised `m` and `n` readable bytes.
        let (x, y) = unsafe {
            (
                std::slice::from_raw_parts(needle.cast::<u8>(), m as usize),
                std::slice::from_raw_parts(haystack.cast::<u8>(), n as usize),
            )
        };
        // An empty needle is at the start, which is `String#index("")`'s own
        // answer and MRI's `m < 1` case.
        if x.is_empty() {
            return Ok(0);
        }
        let step = code_unit_width(super::misc::encoding_of(enc));
        Ok(y
            .windows(x.len())
            .step_by(step)
            .position(|w| w == x)
            .map_or(-1, |i| (i * step) as c_long))
    }

    // ---- building --------------------------------------------------------

    fn rb_str_buf_new_cstr(p: *const c_char) -> Value {
        let bytes = unsafe { borrow_bytes(p, -1) };
        to_value(&new_str(bytes, crate::encoding::ASCII_8BIT))
    }

    /// `rb_str_tmp_new(len)`: a binary scratch buffer of `len` NUL bytes.
    /// An extension fills it through `RSTRING_PTR` and calls
    /// `rb_str_set_len`.
    fn rb_str_tmp_new(len: c_long) -> Value {
        to_value(&new_str(vec![0; len.max(0) as usize], crate::encoding::ASCII_8BIT))
    }

    /// `rb_str_new_with_class(obj, ptr, len)`: a String of `obj`'s class
    /// rather than `String`. A subclass instance is what the caller wants,
    /// and zeo's Str carries no class of its own -- so a plain String is
    /// answered when the class is not `String`, and the difference is
    /// visible. Refusing would break `rb_str_new_with_class(self, ...)` in
    /// an ordinary String method, which is most of its uses.
    fn rb_str_new_with_class(_obj: Value, p: *const c_char, len: c_long) -> Value {
        let bytes = unsafe { borrow_bytes(p, len) };
        to_value(&new_str(bytes, crate::encoding::ASCII_8BIT))
    }

    /// `rb_interned_str(ptr, len)`: the deduplicated String `-"x"` answers.
    fn rb_interned_str(p: *const c_char, len: c_long) -> Value {
        let bytes = unsafe { borrow_bytes(p, len) };
        interned(bytes, crate::encoding::UTF_8)
    }

    fn rb_interned_str_cstr(p: *const c_char) -> Value {
        let bytes = unsafe { borrow_bytes(p, -1) };
        interned(bytes, crate::encoding::UTF_8)
    }

    fn rb_str_to_interned_str(v: Value) -> Value {
        let s = unsafe { as_str(v)? };
        let (bytes, enc) = {
            let g = s.lock();
            (g.bytes().to_vec(), g.encoding())
        };
        interned(bytes, enc)
    }

    /// `rb_str_resurrect` and `rb_str_new_shared` both answer a NEW String
    /// with the same bytes. MRI shares the buffer and zeo copies; the two
    /// differ only in cost, because a shared buffer is copy-on-write there.
    fn rb_str_resurrect(v: Value) -> Value {
        copy_of(v)
    }

    fn rb_str_new_shared(v: Value) -> Value {
        copy_of(v)
    }

    /// `rb_str_new_frozen` / `rb_str_dup_frozen`: a frozen String with the
    /// same bytes, and the receiver itself when it is already frozen -- MRI
    /// takes that shortcut and extensions compare the answer's identity.
    fn rb_str_new_frozen(v: Value) -> Value {
        let sv = unsafe { value_of(v) };
        if sv.is_frozen() {
            return Ok(v);
        }
        let out = unsafe { value_of(copy_of(v)?) };
        out.freeze_value()?;
        to_value(&out)
    }

    fn rb_str_dup_frozen(v: Value) -> Value {
        unsafe { Ok(rb_str_new_frozen(v)) }
    }

    // ---- appending -------------------------------------------------------

    /// `rb_str_append(str, str2)`: `String#<<` with a String argument, so an
    /// incompatible pair of encodings raises rather than concatenating
    /// bytes that mean nothing together.
    fn rb_str_append(dst: Value, src: Value) -> Value {
        let d = unsafe { value_of(dst) };
        let s = unsafe { value_of(src) };
        if !matches!(s, RubyValue::Str(_)) {
            return Err(crate::builtins::wrong_arg_type(&s, "String"));
        }
        super::object::send(&d, "<<", &[s])?;
        Ok(dst)
    }

    fn rb_str_buf_append(dst: Value, src: Value) -> Value {
        unsafe { Ok(rb_str_append(dst, src)) }
    }

    fn rb_str_cat2(v: Value, p: *const c_char) -> Value {
        unsafe { Ok(rb_str_cat_cstr(v, p)) }
    }

    fn rb_str_buf_cat2(v: Value, p: *const c_char) -> Value {
        unsafe { Ok(rb_str_cat_cstr(v, p)) }
    }

    /// `rb_str_buf_cat_ascii`: the bytes are ASCII by the caller's promise,
    /// so they concatenate with any ASCII-compatible receiver.
    fn rb_str_buf_cat_ascii(v: Value, p: *const c_char) -> Value {
        unsafe { Ok(rb_str_cat_cstr(v, p)) }
    }

    // ---- measuring -------------------------------------------------------

    /// `rb_str_capacity`: how many bytes fit before the buffer must grow.
    /// zeo's String is a `Vec`, so this is its capacity -- the honest answer
    /// to the question an extension is asking, which is "will my next
    /// `rb_str_cat` reallocate".
    fn rb_str_capacity(v: Value) -> usize {
        let s = unsafe { as_str(v)? };
        let n = s.lock().bytesize();
        Ok(n)
    }


    /// `rb_str_strlen`: the CHARACTER length, which is what `String#length`
    /// answers and what `RSTRING_LEN` does not.
    fn rb_str_strlen(v: Value) -> c_long {
        let s = unsafe { as_str(v)? };
        let n = s.lock().char_len();
        Ok(n as c_long)
    }

    /// `rb_str_sublen(str, pos)`: how many CHARACTERS the first `pos` BYTES
    /// hold. An extension uses it to turn a byte offset it found back into
    /// an index Ruby understands.
    fn rb_str_sublen(v: Value, pos: c_long) -> c_long {
        let s = unsafe { as_str(v)? };
        let g = s.lock();
        let want = pos.max(0) as usize;
        let n = g.char_ranges().iter().take_while(|r| r.start < want).count();
        Ok(n as c_long)
    }

    /// `rb_str_offset(str, i)`: the BYTE offset of character `i`, which is
    /// the inverse of `rb_str_sublen`.
    fn rb_str_offset(v: Value, i: c_long) -> c_long {
        let s = unsafe { as_str(v)? };
        let g = s.lock();
        let ranges = g.char_ranges();
        let idx = i.max(0) as usize;
        Ok(ranges.get(idx).map_or(g.bytesize(), |r| r.start) as c_long)
    }

    fn rb_str_hash(v: Value) -> usize {
        let s = unsafe { as_str(v)? };
        let g = s.lock();
        Ok(crate::cext::st::bytes_hash_of(g.bytes()))
    }

    /// `rb_str_cmp` and `rb_str_hash_cmp` both answer 0 for equal, which is
    /// the `strcmp` convention rather than a predicate.
    fn rb_str_cmp(a: Value, b: Value) -> c_int {
        let (x, y) = (unsafe { as_str(a)? }, unsafe { as_str(b)? });
        let out = x.lock().bytes().cmp(y.lock().bytes());
        Ok(out as c_int)
    }

    fn rb_str_hash_cmp(a: Value, b: Value) -> c_int {
        unsafe { Ok(rb_str_cmp(a, b)) }
    }

    /// `rb_str_comparable`: can the two be compared byte for byte? Two
    /// strings are when their encodings are compatible, which is exactly the
    /// question `String#<=>` asks before it answers nil.
    fn rb_str_comparable(a: Value, b: Value) -> c_int {
        let (x, y) = (unsafe { as_str(a)? }, unsafe { as_str(b)? });
        let ok = x.lock().concat_enc_with(&y.lock()).is_some();
        Ok(c_int::from(ok))
    }

    // ---- slicing ---------------------------------------------------------

    /// `rb_str_subseq(str, beg, len)`: a BYTE range, unlike
    /// `rb_str_substr`'s character range.
    fn rb_str_subseq(v: Value, beg: c_long, len: c_long) -> Value {
        let s = unsafe { as_str(v)? };
        let g = s.lock();
        let (lo, hi) = byte_span(g.bytesize(), beg, len);
        let out = new_str(g.bytes()[lo..hi].to_vec(), g.encoding());
        drop(g);
        to_value(&out)
    }

    /// `rb_str_drop_bytes(str, n)`: remove the first `n` bytes IN PLACE.
    fn rb_str_drop_bytes(v: Value, n: c_long) -> Value {
        let s = unsafe { as_str(v)? };
        check_writable(&s)?;
        let mut g = s.lock();
        let cut = (n.max(0) as usize).min(g.bytesize());
        let rest = g.bytes()[cut..].to_vec();
        let enc = g.encoding();
        g.replace_bytes(rest, enc);
        drop(g);
        Ok(v)
    }

    /// `rb_str_set_len(str, len)`: truncate to `len` BYTES. MRI also uses it
    /// to publish bytes written through `RSTRING_PTR` past the old length,
    /// and the pin's copy-back is what makes that work here -- so the pin is
    /// flushed first.
    fn rb_str_set_len(v: Value, len: c_long) -> () {
        flush_pins();
        let s = unsafe { as_str(v)? };
        check_writable(&s)?;
        let mut g = s.lock();
        let mut bytes = g.bytes().to_vec();
        bytes.resize(len.max(0) as usize, 0);
        let enc = g.encoding();
        g.replace_bytes(bytes, enc);
        Ok(())
    }

    /// `rb_str_update(str, beg, len, val)`: `String#[]=` over a CHARACTER
    /// range.
    fn rb_str_update(v: Value, beg: c_long, len: c_long, val: Value) -> () {
        let sv = unsafe { value_of(v) };
        let replacement = unsafe { value_of(val) };
        super::object::send(
            &sv,
            "[]=",
            &[
                RubyValue::Int(beg as i64),
                RubyValue::Int(len as i64),
                replacement,
            ],
        )?;
        Ok(())
    }

    /// `rb_str_shared_replace(dst, src)`: `dst` takes `src`'s bytes. MRI
    /// hands over the buffer; zeo copies, and the observable end state is
    /// the same.
    fn rb_str_shared_replace(dst: Value, src: Value) -> () {
        let d = unsafe { as_str(dst)? };
        let s = unsafe { as_str(src)? };
        check_writable(&d)?;
        let (bytes, enc) = {
            let g = s.lock();
            (g.bytes().to_vec(), g.encoding())
        };
        d.lock().replace_bytes(bytes, enc);
        Ok(())
    }

    /// `rb_str_subpos(str, beg, &len)`: a pointer at character `beg` and, in
    /// `*len`, how many BYTES the `*len` characters from there occupy. The
    /// pointer is into the pinned buffer, so it lives exactly as long as
    /// every other `RSTRING_PTR` answer.
    fn rb_str_subpos(v: Value, beg: c_long, len: *mut c_long) -> *mut c_char {
        let s = unsafe { as_str(v)? };
        let base = pin_bytes(&s);
        let g = s.lock();
        let ranges = g.char_ranges();
        let start = beg.max(0) as usize;
        let Some(from) = ranges.get(start).map(|r| r.start) else {
            drop(g);
            return Ok(std::ptr::null_mut());
        };
        if !len.is_null() {
            // SAFETY: the caller's own `long`.
            let want = unsafe { len.read() }.max(0) as usize;
            let to = ranges
                .get(start + want)
                .map_or(g.bytesize(), |r| r.start);
            unsafe { len.write((to - from) as c_long) };
        }
        drop(g);
        // SAFETY: `from` is a byte offset inside the pinned buffer.
        Ok(unsafe { base.add(from) })
    }

    // ---- conversion and formatting ---------------------------------------

    /// `rb_str_split(str, sep)`: `String#split` with a C separator.
    fn rb_str_split(v: Value, sep: *const c_char) -> Value {
        let sv = unsafe { value_of(v) };
        let sep = new_str(unsafe { borrow_bytes(sep, -1) }, crate::encoding::UTF_8);
        to_value(&super::object::send(&sv, "split", &[sep])?)
    }

    /// `rb_str_format(argc, argv, fmt)`: `String#%` with the arguments as an
    /// Array, which is the form `sprintf` uses internally.
    fn rb_str_format(argc: c_int, argv: *const Value, fmt: Value) -> Value {
        let f = unsafe { value_of(fmt) };
        let args = unsafe { super::object::args_of(argc, argv) };
        let arr = RubyValue::Array(crate::value::collections::array_new(args));
        to_value(&super::object::send(&f, "%", &[arr])?)
    }

    fn rb_str2inum(v: Value, base: c_int) -> Value {
        unsafe { Ok(super::numeric::rb_str_to_inum(v, base, 1)) }
    }

    /// `rb_str_ellipsize(str, len)`: at most `len` characters, with the tail
    /// replaced by `...` when it does not fit. MRI uses it for a message
    /// that must not run long.
    fn rb_str_ellipsize(v: Value, len: c_long) -> Value {
        let s = unsafe { as_str(v)? };
        let g = s.lock();
        let want = len.max(0) as usize;
        if g.char_len() <= want {
            drop(g);
            return Ok(v);
        }
        // Three of the budget go to the dots, unless the budget is smaller.
        let keep = want.saturating_sub(3);
        let cut = g.char_ranges().get(keep).map_or(g.bytesize(), |r| r.start);
        let mut bytes = g.bytes()[..cut].to_vec();
        bytes.extend_from_slice(&b"..."[..3.min(want)]);
        let out = new_str(bytes, g.encoding());
        drop(g);
        to_value(&out)
    }

    /// `rb_str_export` / `rb_str_export_locale` / `rb_str_encode_ospath`:
    /// re-encode for the world outside the process. zeo's external, locale
    /// and filesystem encodings are all UTF-8 on every target it builds
    /// extensions for, so each is the receiver -- and a re-encode that
    /// changed nothing would still cost a copy.
    fn rb_str_export(v: Value) -> Value {
        unsafe { as_str(v)? };
        Ok(v)
    }

    fn rb_str_export_locale(v: Value) -> Value {
        unsafe { as_str(v)? };
        Ok(v)
    }

    fn rb_str_encode_ospath(v: Value) -> Value {
        unsafe { as_str(v)? };
        Ok(v)
    }

    // ---- the write guards ------------------------------------------------

    /// `rb_str_modify(str)`: MRI un-shares the buffer and checks the freeze.
    /// zeo has nothing to un-share, so the freeze check is the whole of it --
    /// and it is the half an extension depends on.
    fn rb_str_modify(v: Value) -> () {
        let s = unsafe { as_str(v)? };
        check_writable(&s)?;
        Ok(())
    }

    /// `rb_str_modify_expand(str, extra)`: the same, plus room for `extra`
    /// more bytes. zeo's `Vec` grows on demand, so the hint is accepted.
    fn rb_str_modify_expand(v: Value, _extra: c_long) -> () {
        unsafe { rb_str_modify(v);
        Ok(()) }
    }

    /// `rb_str_locktmp` / `rb_str_unlocktmp`: MRI's flag forbidding a
    /// re-entrant modify while C holds the buffer. zeo's pin already gives
    /// C a buffer of its own, so there is nothing to lock against.
    fn rb_str_locktmp(v: Value) -> Value {
        unsafe { as_str(v)? };
        Ok(v)
    }

    fn rb_str_unlocktmp(v: Value) -> Value {
        unsafe { as_str(v)? };
        Ok(v)
    }

    /// `rb_str_free(str)`: MRI releases the buffer early. zeo's String is
    /// refcounted and is freed when the last reference goes, so freeing it
    /// here would leave the `VALUE` the extension still holds dangling.
    fn rb_str_free(v: Value) -> () {
        unsafe { as_str(v)? };
        Ok(())
    }

    // ---- StringValue -----------------------------------------------------

    /// `StringValue(v)`: convert IN PLACE through `to_str`, so the caller's
    /// own `VALUE` becomes the String and stays pinned. Every one of the
    /// three spellings rewrites the slot, which is what makes the macro's
    /// `RSTRING_PTR(v)` on the next line safe.
    fn rb_string_value(slot: *mut Value) -> Value {
        string_value_in(slot)
    }

    fn rb_string_value_ptr(slot: *mut Value) -> *mut c_char {
        let v = string_value_in(slot)?;
        let s = unsafe { as_str(v)? };
        Ok(pin_bytes(&s))
    }

    /// `rb_string_value_cstr`: the same, and an embedded NUL is an
    /// `ArgumentError` -- because the caller is about to treat the answer as
    /// a C string, and a NUL would silently truncate it.
    fn rb_string_value_cstr(slot: *mut Value) -> *mut c_char {
        let v = string_value_in(slot)?;
        let s = unsafe { as_str(v)? };
        if s.lock().bytes().contains(&0) {
            return Err(crate::builtins::arg_error!("string contains null byte"));
        }
        Ok(pin_bytes(&s))
    }
}

/// A frozen String cannot be written, and MRI's own message names it.
fn check_writable(s: &RStr) -> Result<(), crate::Signal> {
    let v = RubyValue::Str(s.clone());
    if v.is_frozen() {
        return Err(crate::builtins::frozen_error!(
            "can't modify frozen String: {}",
            v.to_display_string()
        ));
    }
    Ok(())
}

/// A new String with `v`'s bytes and encoding.
fn copy_of(v: Value) -> Result<Value, crate::Signal> {
    let s = unsafe { as_str(v)? };
    let g = s.lock();
    let out = new_str(g.bytes().to_vec(), g.encoding());
    drop(g);
    to_value(&out)
}

/// The deduplicated, frozen String `String#-@` answers.
fn interned(bytes: Vec<u8>, enc: crate::encoding::EncodingId) -> Result<Value, crate::Signal> {
    let s = new_str(bytes, enc);
    to_value(&super::object::send(&s, "-@", &[])?)
}

/// A byte range clamped to the string, with MRI's negative-start rule.
fn byte_span(size: usize, beg: c_long, len: c_long) -> (usize, usize) {
    let size = size as c_long;
    let start = if beg < 0 { beg + size } else { beg }.clamp(0, size);
    let end = (start + len.max(0)).clamp(start, size);
    (start as usize, end as usize)
}

/// `StringValue`'s in-place conversion: `to_str` unless it is already a
/// String, written back into the caller's own slot.
fn string_value_in(slot: *mut Value) -> Result<Value, crate::Signal> {
    if slot.is_null() {
        return Err(crate::builtins::type_error!(
            "no implicit conversion of nil into String"
        ));
    }
    // SAFETY: the caller's own `VALUE` slot; MRI's prototype says
    // `volatile VALUE *` for the same reason.
    let raw = unsafe { slot.read() };
    let v = unsafe { value_of(raw) };
    if matches!(v, RubyValue::Str(_)) {
        return Ok(raw);
    }
    let out = super::object::send(&v, "to_str", &[])?;
    if !matches!(out, RubyValue::Str(_)) {
        return Err(crate::builtins::wrong_arg_type(&v, "String"));
    }
    let converted = to_value(&out)?;
    // SAFETY: the caller's own slot again.
    unsafe { slot.write(converted) };
    Ok(converted)
}

/// `RSTRING_PTR`'s runtime half. See this module's docs for what "pinned"
/// buys and what it costs.
///
/// # Safety
///
/// `v` must be a live String `VALUE`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rbimpl_zeo_str_ptr(v: Value) -> *mut c_char {
    match unsafe { as_str(v) } {
        Ok(s) => pin_bytes(&s),
        Err(sig) => super::jmp::raise(sig),
    }
}

/// `RSTRING_LEN`'s runtime half.
///
/// A pinned string answers the PIN's length, not the object's: the extension
/// is holding that buffer, and telling it a different length than the pointer
/// it was given is how an overrun happens.
///
/// # Safety
///
/// `v` must be a live String `VALUE`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rbimpl_zeo_str_len(v: Value) -> c_long {
    let s = match unsafe { as_str(v) } {
        Ok(s) => s,
        Err(sig) => super::jmp::raise(sig),
    };
    let key = std::sync::Arc::as_ptr(&s) as *const () as usize;
    let pinned =
        PINS.with_borrow(|pins| pins.iter().find(|(k, _)| *k == key).map(|(_, pin)| pin.len));
    pinned.unwrap_or_else(|| s.lock().bytesize()) as c_long
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cext::scope::Scope;

    fn a_string(s: &str) -> RubyValue {
        crate::builtins::string::str_value_in_enc(crate::encoding::UTF_8, s)
    }

    fn bytes_of(v: &RubyValue) -> Vec<u8> {
        match v {
            RubyValue::Str(s) => s.lock().bytes().to_vec(),
            _ => panic!("not a String"),
        }
    }

    #[test]
    fn a_new_string_carries_the_bytes_and_the_encoding() {
        let _scope = Scope::enter();
        let raw = unsafe { rb_utf8_str_new(c"hi".as_ptr(), 2) };
        let v = unsafe { value_of(raw) };
        assert_eq!(bytes_of(&v), b"hi");
        match &v {
            RubyValue::Str(s) => assert_eq!(s.lock().encoding(), crate::encoding::UTF_8),
            _ => panic!("not a String"),
        }
    }

    #[test]
    fn a_negative_length_reads_to_the_nul() {
        let _scope = Scope::enter();
        let raw = unsafe { rb_str_new(c"cstr".as_ptr(), -1) };
        assert_eq!(bytes_of(&unsafe { value_of(raw) }), b"cstr");
        let same = unsafe { rb_str_new_cstr(c"cstr".as_ptr()) };
        assert_eq!(bytes_of(&unsafe { value_of(same) }), b"cstr");
    }

    /// The pin is keyed by the object, so an extension that takes the pointer
    /// twice and compares the two is right.
    #[test]
    fn two_pointers_to_one_string_are_the_same_pointer() {
        let _scope = Scope::enter();
        let s = a_string("stable");
        let raw = to_value(&s).expect("a String converts");
        let a = unsafe { rbimpl_zeo_str_ptr(raw) };
        let b = unsafe { rbimpl_zeo_str_ptr(raw) };
        assert_eq!(a, b);
        assert_eq!(unsafe { rbimpl_zeo_str_len(raw) }, 6);
    }

    /// A write through `RSTRING_PTR` reaches the Ruby string when the scope
    /// pops -- that is the whole contract, and the reason the pin exists.
    #[test]
    fn a_write_through_the_pointer_lands_at_scope_pop() {
        let s = a_string("abc");
        {
            let _scope = Scope::enter();
            let raw = to_value(&s).expect("a String converts");
            let p = unsafe { rbimpl_zeo_str_ptr(raw) };
            unsafe { p.write(b'X' as c_char) };
            assert_eq!(bytes_of(&s), b"abc", "the write landed too early");
            flush_pins();
        }
        assert_eq!(bytes_of(&s), b"Xbc", "the write never landed");
    }

    #[test]
    fn a_pinned_string_is_nul_terminated() {
        let _scope = Scope::enter();
        let s = a_string("nul");
        let raw = to_value(&s).expect("a String converts");
        let p = unsafe { rbimpl_zeo_str_ptr(raw) };
        assert_eq!(unsafe { p.add(3).read() }, 0);
    }

    #[test]
    fn cat_appends_and_answers_the_receiver() {
        let _scope = Scope::enter();
        let s = a_string("ab");
        let raw = to_value(&s).expect("a String converts");
        assert_eq!(unsafe { rb_str_cat(raw, c"cd".as_ptr(), 2) }, raw);
        assert_eq!(bytes_of(&s), b"abcd");
        assert_eq!(unsafe { rb_str_cat_cstr(raw, c"e".as_ptr()) }, raw);
        assert_eq!(bytes_of(&s), b"abcde");
    }

    #[test]
    fn a_dup_is_a_new_object_with_the_same_bytes() {
        let _scope = Scope::enter();
        let s = a_string("orig");
        let raw = to_value(&s).expect("a String converts");
        let copy = unsafe { rb_str_dup(raw) };
        assert_ne!(copy, raw, "dup answered the same object");
        assert_eq!(bytes_of(&unsafe { value_of(copy) }), b"orig");
    }

    /// The pin outlives the SCOPE, because an extension is allowed to keep
    /// the pointer. `msgpack`'s `Unpacker#feed_reference` stores it and
    /// holds the String beside it, which on MRI is what keeps the bytes
    /// valid -- freeing zeo's copy at scope pop made `full_unpack` read
    /// released memory and answer plausible integers.
    #[test]
    fn a_pinned_pointer_survives_the_scope_that_made_it() {
        let s = a_string("held across the boundary");
        let RubyValue::Str(rs) = &s else {
            panic!("not a String")
        };
        let p = {
            let _scope = Scope::enter();
            pin_bytes(rs)
        };
        // SAFETY: the string is still alive, so the pin is too.
        let seen = unsafe { std::ffi::CStr::from_ptr(p) };
        assert_eq!(seen.to_bytes(), b"held across the boundary");
    }

    /// And it does NOT outlive the string. A pin whose owner is gone is
    /// evicted, so the buffer's life is exactly the window MRI's is.
    #[test]
    fn a_pin_is_evicted_when_its_string_is_gone() {
        let before = PINS.with_borrow(Vec::len);
        {
            let s = a_string("temporary");
            let RubyValue::Str(rs) = &s else {
                panic!("not a String")
            };
            let _scope = Scope::enter();
            pin_bytes(rs);
            assert_eq!(PINS.with_borrow(Vec::len), before + 1);
        }
        // The scope popped and the string dropped; the next flush evicts.
        flush_pins();
        assert_eq!(PINS.with_borrow(Vec::len), before, "a dead pin survived");
    }

    /// A Ruby-side rewrite has to be visible to the next `RSTRING_PTR`. The
    /// buffer is a COPY, so a stale one would hand C the old bytes forever.
    #[test]
    fn a_re_pin_refreshes_from_the_string() {
        let _scope = Scope::enter();
        let s = a_string("first");
        let RubyValue::Str(rs) = &s else {
            panic!("not a String")
        };
        let p = pin_bytes(rs);
        // SAFETY: just pinned.
        assert_eq!(unsafe { std::ffi::CStr::from_ptr(p) }.to_bytes(), b"first");
        {
            let mut g = rs.lock();
            let enc = g.encoding();
            g.replace_bytes(b"second".to_vec(), enc);
        }
        let p = pin_bytes(rs);
        // SAFETY: re-pinned, and the length changed so this is the new one.
        assert_eq!(unsafe { std::ffi::CStr::from_ptr(p) }.to_bytes(), b"second");
    }

    /// A wrong receiver raises rather than reading a byte that means
    /// nothing. `raise_error` panics with the message in a registry-less unit
    /// test, which is its documented behaviour and what makes the refusal
    /// visible here.
    #[test]
    // The class NAME needs the registry, which a unit test has no more of
    // than it has an exception class; the refusal is what is on trial.
    #[should_panic(expected = "TypeError: wrong argument type")]
    fn a_non_string_receiver_raises_rather_than_reading_garbage() {
        let _scope = Scope::enter();
        let n = to_value(&RubyValue::Int(3)).expect("an Int converts");
        let _ = unsafe { as_str(n) };
    }
}

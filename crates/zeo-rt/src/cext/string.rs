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
use std::ffi::{c_char, c_long};

/// One pinned string's bytes: what C sees, and what to write back into.
struct Pin {
    owner: RStr,
    /// `len` bytes, then a NUL, then one guard byte the copy-back checks.
    buf: Vec<u8>,
    len: usize,
}

/// The guard byte, chosen so a plausible overrun (a NUL, a space, a digit)
/// does not look like an intact buffer.
const GUARD: u8 = 0xA5;

thread_local! {
    /// Pinned strings, innermost scope last. Keyed by the payload address, so
    /// two `RSTRING_PTR` calls on one string answer one pointer.
    static PINS: RefCell<Vec<(usize, Box<Pin>)>> = const { RefCell::new(Vec::new()) };
}

/// A stable, writable `char *` for `s`, valid until the enclosing scope pops.
fn pin_bytes(s: &RStr) -> *mut c_char {
    let key = std::sync::Arc::as_ptr(s) as *const () as usize;
    PINS.with_borrow_mut(|pins| {
        if let Some((_, pin)) = pins.iter_mut().find(|(k, _)| *k == key) {
            return pin.buf.as_mut_ptr().cast();
        }
        let bytes = s.lock().bytes().to_vec();
        let len = bytes.len();
        let mut buf = bytes;
        buf.push(0);
        buf.push(GUARD);
        let mut pin = Box::new(Pin {
            owner: s.clone(),
            buf,
            len,
        });
        let ptr = pin.buf.as_mut_ptr().cast();
        pins.push((key, pin));
        ptr
    })
}

/// Write every pinned string back, and drop the pins.
///
/// Called from [`super::scope::Scope`]'s pop, and from the longjmp unwind, so
/// a raise does not lose a write the extension had already made.
pub(super) fn flush_pins() {
    let pins = PINS.with_borrow_mut(std::mem::take);
    for (_, pin) in pins {
        assert_eq!(
            pin.buf[pin.len + 1],
            GUARD,
            "a C extension wrote past the end of a string it got from RSTRING_PTR"
        );
        let bytes = &pin.buf[..pin.len];
        let mut guard = pin.owner.lock();
        if guard.bytes() != bytes {
            let enc = guard.encoding();
            guard.replace_bytes(bytes.to_vec(), enc);
        }
    }
}

/// The `RStr` behind a `VALUE`, or a `TypeError` naming what arrived.
///
/// # Safety
///
/// `v` is an extension's own `VALUE`; see [`value_of`].
unsafe fn as_str(v: Value) -> Result<RStr, crate::Signal> {
    match unsafe { value_of(v) } {
        RubyValue::Str(s) => Ok(s),
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "wrong argument type {} (expected String)",
                crate::dispatch::class_name(other.class_id()).unwrap_or("Object".into())
            ),
        )),
    }
}

/// `len < 0` means "to the NUL", which is what `rb_str_new_cstr` and every
/// `rb_str_new(p, -1)` in the wild mean.
///
/// # Safety
///
/// `p` must name `len` readable bytes, or a NUL-terminated string.
unsafe fn borrow(p: *const c_char, len: c_long) -> Vec<u8> {
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
        let bytes = unsafe { borrow(p, len) };
        to_value(&new_str(bytes, crate::encoding::ASCII_8BIT))
    }

    fn rb_str_new_cstr(p: *const c_char) -> Value {
        let bytes = unsafe { borrow(p, -1) };
        to_value(&new_str(bytes, crate::encoding::ASCII_8BIT))
    }

    fn rb_utf8_str_new(p: *const c_char, len: c_long) -> Value {
        let bytes = unsafe { borrow(p, len) };
        to_value(&new_str(bytes, crate::encoding::UTF_8))
    }

    fn rb_utf8_str_new_cstr(p: *const c_char) -> Value {
        let bytes = unsafe { borrow(p, -1) };
        to_value(&new_str(bytes, crate::encoding::UTF_8))
    }

    fn rb_usascii_str_new(p: *const c_char, len: c_long) -> Value {
        let bytes = unsafe { borrow(p, len) };
        to_value(&new_str(bytes, crate::encoding::US_ASCII))
    }

    fn rb_usascii_str_new_cstr(p: *const c_char) -> Value {
        let bytes = unsafe { borrow(p, -1) };
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
        let add = unsafe { borrow(p, len) };
        s.lock().push_bytes(&add);
        Ok(v)
    }

    fn rb_str_buf_cat(v: Value, p: *const c_char, len: c_long) -> Value {
        let s = unsafe { as_str(v)? };
        let add = unsafe { borrow(p, len) };
        s.lock().push_bytes(&add);
        Ok(v)
    }

    fn rb_str_cat_cstr(v: Value, p: *const c_char) -> Value {
        let s = unsafe { as_str(v)? };
        let add = unsafe { borrow(p, -1) };
        s.lock().push_bytes(&add);
        Ok(v)
    }

    /// `rb_str_freeze`. Answers the receiver, as MRI does.
    fn rb_str_freeze(v: Value) -> Value {
        let s = unsafe { as_str(v)? };
        RubyValue::Str(s).freeze_value()?;
        Ok(v)
    }
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

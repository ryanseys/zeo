//! The Rust half of `csrc/cext_fmt.c`.
//!
//! MRI's format strings carry one extension C does not have: `PRIsVALUE`
//! expands to a conversion followed by a `\v` sentinel, and means "the
//! argument is a `VALUE`, print its `to_s`". The walk that finds the sentinel
//! has to be C -- it reads a `va_list` -- but the text it needs is a Ruby
//! method call, which is here.
//!
//! # The lifetime the C side needs
//!
//! `zeo_cext_value_text` hands back a `const char *` the formatter passes
//! straight to `snprintf`. So it has to outlive the call and must not leak
//! once per format: `rb_raise` in a loop would grow the process without
//! bound. It is owned by the current scope, and freed with the string pins.

use super::convert::{to_value, value_of};
use super::value::Value;
use crate::RubyValue;
use std::cell::RefCell;
use std::ffi::{CString, c_char, c_long};

thread_local! {
    /// Text handed to a C formatter, alive until the scope pops. A format
    /// call names a handful at most, so this never grows far.
    static TEXTS: RefCell<Vec<CString>> = const { RefCell::new(Vec::new()) };
}

/// Drop every formatted string. Called from the scope pop with the pins.
pub(super) fn flush_texts() {
    TEXTS.with_borrow_mut(Vec::clear);
}

/// `to_s`, or `inspect` when the format carried the `+` flag.
///
/// # Safety
///
/// `v` must be a live `VALUE`; the formatter read it out of the caller's own
/// `va_list` at the type `PRIsVALUE` names.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_cext_value_text(v: Value, inspect: i32) -> *const c_char {
    let recv = unsafe { value_of(v) };
    let meth = if inspect != 0 { "inspect" } else { "to_s" };
    // A raise from inside `to_s` cannot travel: the caller is a C formatter
    // mid-`va_list`, and longjmping out of it would strand the walk. The
    // class name is the honest fallback and is what MRI's own `rb_inspect`
    // failure path shows.
    let text = super::object::send(&recv, meth, &[])
        .map(|s| s.to_display_string())
        .unwrap_or_else(|_| {
            format!(
                "#<{}>",
                crate::dispatch::class_name(recv.class_id()).unwrap_or("Object".into())
            )
        });
    hold(text)
}

/// Keep `text` alive for the current scope and answer its bytes.
fn hold(text: String) -> *const c_char {
    let c = CString::new(text).unwrap_or_else(|e| {
        // An embedded NUL cannot go through a `char *`. Truncating at it is
        // what C would see anyway, so it is the honest answer.
        let mut bytes = e.into_vec();
        bytes.truncate(bytes.iter().position(|b| *b == 0).unwrap_or(0));
        CString::new(bytes).expect("the NUL was just removed")
    });
    TEXTS.with_borrow_mut(|t| {
        t.push(c);
        t.last().expect("just pushed").as_ptr()
    })
}

/// `rb_sprintf`'s answer: a new String from the formatted bytes.
///
/// # Safety
///
/// `p` must name `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_cext_str_new_len(p: *const c_char, len: c_long) -> Value {
    let bytes = unsafe { super::string::borrow_bytes(p, len) };
    let s = RubyValue::Str(crate::string_from_bytes(bytes, crate::encoding::UTF_8));
    match to_value(&s) {
        Ok(v) => v,
        Err(sig) => super::jmp::raise(sig),
    }
}

/// `rb_str_catf`'s answer: the same bytes appended to an existing String.
///
/// # Safety
///
/// `p` must name `len` readable bytes and `str` must be a live String
/// `VALUE`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_cext_str_cat_len(str: Value, p: *const c_char, len: c_long) -> Value {
    let bytes = unsafe { super::string::borrow_bytes(p, len) };
    let RubyValue::Str(s) = (unsafe { value_of(str) }) else {
        super::jmp::raise(crate::dispatch::raise_error(
            "TypeError",
            "rb_str_catf needs a String".into(),
        ))
    };
    let mut g = s.lock();
    let mut all = g.bytes().to_vec();
    all.extend_from_slice(&bytes);
    let enc = g.encoding();
    g.replace_bytes(all, enc);
    drop(g);
    str
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pointer the C formatter reads has to survive until the scope
    /// pops, and two calls must not answer one buffer.
    #[test]
    fn two_texts_are_two_live_pointers() {
        let scope = super::super::scope::Scope::enter();
        let a = hold("first".into());
        let b = hold("second".into());
        assert_ne!(a, b);
        // SAFETY: both were just held by this scope.
        unsafe {
            assert_eq!(std::ffi::CStr::from_ptr(a).to_bytes(), b"first");
            assert_eq!(std::ffi::CStr::from_ptr(b).to_bytes(), b"second");
        }
        drop(scope);
        assert_eq!(
            TEXTS.with_borrow(Vec::len),
            0,
            "the scope did not free them"
        );
    }

    /// A NUL cannot cross a `char *`, and truncating is what C would read.
    /// Answering a null pointer instead would print `(null)`.
    #[test]
    fn an_embedded_nul_truncates_rather_than_failing() {
        let _scope = super::super::scope::Scope::enter();
        let p = hold("ab\0cd".into());
        // SAFETY: just held.
        assert_eq!(unsafe { std::ffi::CStr::from_ptr(p) }.to_bytes(), b"ab");
    }
}

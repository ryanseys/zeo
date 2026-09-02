//! `ruby/util.h`: the small C helpers MRI ships beside the object API.
//!
//! None of these touch a Ruby object. They exist because MRI wanted one
//! implementation of `strtod` and `qsort_r` across every platform it builds
//! on, and a gem that reaches for `ruby_strdup` is asking for `strdup` --
//! `bcrypt` does exactly that, and nothing else.
//!
//! # `ruby_xmalloc` is the allocator, and it matters here
//!
//! `ruby_strdup`'s result is freed with `ruby_xfree`, so it has to come from
//! `ruby_xmalloc` and not from libc's `malloc`. The two use different
//! headers ([`super::alloc`]), and crossing them corrupts the heap.

use super::object::cstr;
use std::ffi::{c_char, c_int, c_void};

crate::cext_fn! {
    /// `ruby_strdup(s)`: a copy freed with `ruby_xfree`, NOT with libc's
    /// `free` -- see this module's docs.
    fn ruby_strdup(s: *const c_char) -> *mut c_char {
        let bytes = unsafe { super::string::borrow_bytes(s, -1) };
        // SAFETY: the block is `ruby_xfree`'s to release, which is the
        // contract this entry carries.
        let out = unsafe { super::alloc::xmalloc(bytes.len() + 1) }.cast::<u8>();
        // SAFETY: `out` names `len + 1` freshly allocated bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len());
            out.add(bytes.len()).write(0);
        }
        Ok(out.cast())
    }

    /// `ruby_strtod(s, &end)`: `strtod` with MRI's own rules -- no hex
    /// floats, and `_` is not a digit separator here (that is Ruby's
    /// LITERAL syntax, not this).
    fn ruby_strtod(s: *const c_char, end: *mut *mut c_char) -> f64 {
        let text = unsafe { cstr(s) };
        let (value, used) = scan_double(&text);
        if !end.is_null() {
            // SAFETY: the caller's own `char **`.
            unsafe { end.write(s.cast_mut().add(used)) };
        }
        Ok(value)
    }

    /// `ruby_scan_hex` / `ruby_scan_oct` / `ruby_scan_digits`: read at most
    /// `len` digits and report how many were consumed. A caller parses an
    /// escape with them and needs the count to advance.
    fn ruby_scan_hex(s: *const c_char, len: usize, consumed: *mut usize) -> usize {
        scan_radix(s, len as isize, 16, consumed, std::ptr::null_mut())
    }

    fn ruby_scan_oct(s: *const c_char, len: usize, consumed: *mut usize) -> usize {
        scan_radix(s, len as isize, 8, consumed, std::ptr::null_mut())
    }

    /// `ruby_scan_digits(s, len, base, &consumed, &overflow)`: the same, in
    /// any base, and it reports OVERFLOW separately -- a caller cannot tell
    /// a saturated value from a real one without it.
    fn ruby_scan_digits(
        s: *const c_char,
        len: isize,
        base: c_int,
        consumed: *mut usize,
        overflow: *mut c_int,
    ) -> usize {
        scan_radix(s, len, base.max(2) as u32, consumed, overflow)
    }

    /// `ruby_qsort(base, n, size, cmp, arg)`: the system `qsort_r`, which is
    /// what MRI's own `ruby_qsort` reduces to on every platform zeo ships.
    /// An extension that sorts through this row must see the C library's
    /// order for a tie, the same one `Array#sort` sees.
    fn ruby_qsort(
        base: *mut c_void,
        n: usize,
        size: usize,
        cmp: unsafe extern "C-unwind" fn(*const c_void, *const c_void, *mut c_void) -> c_int,
        arg: *mut c_void,
    ) -> () {
        if base.is_null() || size == 0 || n < 2 {
            return Ok(());
        }
        let mut args = BsdQsortArgs {
            cmp,
            arg,
            raised: None,
        };
        let d: *mut c_void = std::ptr::from_mut(&mut args).cast();
        // SAFETY: the caller promised `n` elements of `size` bytes at `base`,
        // and `args` outlives the call.
        unsafe {
            #[cfg(any(target_vendor = "apple", target_os = "freebsd"))]
            libc::qsort_r(base, n, size, d, Some(bsd_qsort_cmp));
            #[cfg(not(any(target_vendor = "apple", target_os = "freebsd")))]
            libc::qsort_r(base, n, size, Some(bsd_qsort_cmp), d);
        }
        // A raise from the comparator waited here for libc to hand the
        // stack back.
        args.raised.map_or(Ok(()), Err)
    }

    /// `ruby_each_words(str, f, arg)`: split on commas and whitespace and
    /// call `f` per word. MRI parses `RUBYOPT`-shaped lists with it.
    fn ruby_each_words(
        s: *const c_char,
        f: unsafe extern "C-unwind" fn(*const c_char, c_int, *mut c_void),
        arg: *mut c_void,
    ) -> () {
        for word in unsafe { cstr(s) }.split([',', ' ', '\t', '\n']) {
            if word.is_empty() {
                continue;
            }
            let c = super::symbol::cstr_for_owned(word);
            // SAFETY: the caller's own callback, on a NUL-terminated word.
            super::unwind::protect(|| unsafe { f(c, word.len() as c_int, arg) })?;
        }
        Ok(())
    }

    fn ruby_getcwd() -> *mut c_char {
        let dir = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let c = super::symbol::cstr_for_owned(&dir);
        Ok(c.cast_mut())
    }

    /// `ruby_setenv` / `ruby_unsetenv`: through `ENV`, so a program that
    /// read `ENV["X"]` before and after sees the change -- a bare
    /// `setenv(3)` would not reach zeo's own copy.
    fn ruby_setenv(name: *const c_char, value: *const c_char) -> () {
        let (n, v) = (unsafe { cstr(name) }, unsafe { cstr(value) });
        env_call("[]=", &[n, v])
    }

    fn ruby_unsetenv(name: *const c_char) -> () {
        env_call("delete", &[unsafe { cstr(name) }])
    }

    /// `ruby_thread_has_gvl_p()`: is this thread holding the lock? Every
    /// thread that can reach a C extension holds it, because zeo only
    /// releases it inside `rb_thread_call_without_gvl` -- and a body running
    /// there is not supposed to ask.
    fn ruby_thread_has_gvl_p() -> c_int {
        Ok(1)
    }

    /// `ruby_free_at_exit_p()`: does the VM free everything at exit?
    /// MRI's `--enable-free-at-exit`, which a gem reads to decide whether to
    /// bother freeing its own. zeo's process exit does not run finalizers
    /// over the whole heap, so the honest answer is no.
    fn ruby_free_at_exit_p() -> bool {
        Ok(false)
    }
}

/// `ENV.[]=` / `ENV.delete`, so zeo's own copy moves with the C caller's.
fn env_call(meth: &str, args: &[String]) -> Result<(), zeo_rt::Signal> {
    let Some(env) = zeo_rt::constants::const_get(zeo_abi::OBJECT_CLASS.0, "ENV") else {
        return Ok(());
    };
    let vals: Vec<zeo_rt::RubyValue> = args
        .iter()
        .map(|a| zeo_rt::builtins::string::str_value_in_enc(zeo_rt::encoding::UTF_8, a))
        .collect();
    super::object::send(&env, meth, &vals)?;
    Ok(())
}

/// Read digits in `base` from `s`, at most `len` of them (`len < 0` means to
/// the NUL). Answers the value, and reports how many bytes were consumed and
/// whether it overflowed.
fn scan_radix(
    s: *const c_char,
    len: isize,
    base: u32,
    consumed: *mut usize,
    overflow: *mut c_int,
) -> Result<usize, zeo_rt::Signal> {
    let text = unsafe { cstr(s) };
    let cap = if len < 0 {
        text.len()
    } else {
        (len as usize).min(text.len())
    };
    let mut value: usize = 0;
    let mut over = false;
    let mut used = 0;
    for ch in text[..cap].chars() {
        let Some(d) = ch.to_digit(base) else { break };
        match value
            .checked_mul(base as usize)
            .and_then(|v| v.checked_add(d as usize))
        {
            Some(v) => value = v,
            // Keep consuming so the caller's cursor lands past the whole
            // number; the flag is how it learns the value is not the one.
            None => over = true,
        }
        used += ch.len_utf8();
    }
    if !consumed.is_null() {
        // SAFETY: the caller's own `size_t`.
        unsafe { consumed.write(used) };
    }
    if !overflow.is_null() {
        // SAFETY: the caller's own `int`.
        unsafe { overflow.write(c_int::from(over)) };
    }
    Ok(value)
}

/// `strtod`'s scan: leading space, an optional sign, digits with at most one
/// point, and an optional exponent. Answers the value and how many bytes it
/// used, so a caller's `end` pointer lands where C's would.
fn scan_double(text: &str) -> (f64, usize) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let start = i;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    let mut saw_digit = false;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
        saw_digit = true;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
            saw_digit = true;
        }
    }
    if !saw_digit {
        // Nothing numeric at all: C leaves `end` at the START, not past the
        // whitespace it skipped.
        return (0.0, 0);
    }
    // An exponent counts only if it has at least one digit after it --
    // otherwise `1e` is the number 1 and `e` is the caller's.
    if i < bytes.len() && (bytes[i] | 0x20) == b'e' {
        let mut j = i + 1;
        if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }
        if j < bytes.len() && bytes[j].is_ascii_digit() {
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    let value = text[start..i].parse::<f64>().unwrap_or(0.0);
    (value, i)
}

/// MRI's `bsd_qsort_r_args`: the extension's comparator carries its own
/// `arg`, so the one libc passes is this triple.
struct BsdQsortArgs {
    cmp: unsafe extern "C-unwind" fn(*const c_void, *const c_void, *mut c_void) -> c_int,
    arg: *mut c_void,
    /// The comparator's raise, parked until `qsort_r` returns: an unwind
    /// must not cross libc's frames. Every later comparison answers 0.
    raised: Option<zeo_rt::Signal>,
}

/// The comparator's raise, caught here and parked in the triple.
fn compare(d: *mut c_void, a: *const c_void, b: *const c_void) -> c_int {
    // SAFETY: `ruby_qsort` handed libc a `*mut BsdQsortArgs` and two pointers
    // into the caller's own buffer; libc hands all three back unchanged.
    let args = unsafe { &mut *d.cast::<BsdQsortArgs>() };
    if args.raised.is_some() {
        return 0;
    }
    match super::unwind::protect(|| unsafe { (args.cmp)(a, b, args.arg) }) {
        Ok(order) => order,
        Err(sig) => {
            args.raised = Some(sig);
            0
        }
    }
}

/// MRI's `cmp_bsd_qsort`, on the argument order this platform's `qsort_r`
/// uses. `extern "C"`, as libc's signature demands: nothing unwinds out.
#[cfg(any(target_vendor = "apple", target_os = "freebsd"))]
unsafe extern "C" fn bsd_qsort_cmp(d: *mut c_void, a: *const c_void, b: *const c_void) -> c_int {
    compare(d, a, b)
}

#[cfg(not(any(target_vendor = "apple", target_os = "freebsd")))]
unsafe extern "C" fn bsd_qsort_cmp(a: *const c_void, b: *const c_void, d: *mut c_void) -> c_int {
    compare(d, a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `end` has to land exactly where C's `strtod` would: past the number
    /// and no further. A caller loops on it.
    #[test]
    fn a_double_scan_reports_what_it_consumed() {
        for (text, want, used) in [
            ("3.5", 3.5, 3),
            ("  -2.5e-2xyz", -0.025, 9),
            ("1e", 1.0, 1),
            ("42", 42.0, 2),
            (".5", 0.5, 2),
        ] {
            assert_eq!(scan_double(text), (want, used), "{text:?}");
        }
        // Nothing numeric leaves `end` at the start, whitespace included.
        assert_eq!(scan_double("   abc"), (0.0, 0));
        assert_eq!(scan_double(""), (0.0, 0));
    }
}

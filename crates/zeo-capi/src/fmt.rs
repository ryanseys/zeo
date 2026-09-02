//! `rb_sprintf` and everything that formats a message.
//!
//! MRI's format string is C's plus one extension, and the extension is the
//! whole reason this file exists. `PRIsVALUE` expands to `"li\v"` -- a
//! conversion that looks like `%li` followed by a literal vertical tab -- and
//! MRI's own `vsnprintf` treats that pair as "the argument is a VALUE, print
//! what `to_s` answers". A `%+` flag asks for `inspect` instead.
//!
//!     rb_raise(rb_eTypeError, "no %"PRIsVALUE" for %+"PRIsVALUE, name, obj);
//!
//! Handing that to the system `vsnprintf` prints the VALUE's bit pattern as a
//! decimal integer, so the message reads `no 4 for 8791234560` and the `\v`
//! lands in the output. So the format is walked here, one conversion at a
//! time: each is copied out, its argument is read from the `VaList` at the
//! type the spec names, and the pair is handed to `snprintf` on its own.
//!
//! Walking rather than delegating is also what makes the reads safe. A
//! `VaList` has to be read at exactly the type the caller pushed, and the
//! conversion spec is the only description of that -- so the spec is parsed
//! fully, length modifiers included, rather than assumed.

use super::convert::{to_value, value_of};
use super::misc::{Encoding, encoding_of};
use super::object::send;
use super::value::Value;
use std::ffi::{CStr, CString, VaList, c_char, c_int, c_long, c_longlong, c_uint, c_ulong, c_void};
use zeo_rt::{RubyValue, Signal};

/// MRI truncates a formatted message too; this is its ceiling for one
/// conversion.
const CONVERSION_MAX: usize = 4096;

/// The sentinel `PRIsVALUE` leaves after its conversion.
const VALUE_MARK: u8 = b'\x0b';

/// How wide the integer argument is. The spec's length modifier is the only
/// thing that says, and reading a `long` where an `int` was pushed is exactly
/// the bug this file exists to avoid.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Len {
    Int,
    Char,
    Short,
    Long,
    LLong,
    Size,
    Ptrdiff,
}

/// One conversion's argument, read at the type its spec names.
enum Arg {
    /// Every integer width, re-spelled as `ll` for `snprintf`.
    Int(c_longlong),
    Float(f64),
    Char(c_int),
    Str(*const c_char),
    Ptr(*const c_void),
}

/// Which of width and precision came from a `*` argument.
#[derive(Clone, Copy, Default)]
struct Stars {
    width: Option<c_int>,
    precision: Option<c_int>,
}

/// One conversion through the system `snprintf`, `*` arguments re-passed in
/// the order the spec names them.
fn emit(out: &mut Vec<u8>, spec: &[u8], stars: Stars, arg: Arg) {
    let Ok(spec) = CString::new(spec) else { return };
    let mut tmp = [0u8; CONVERSION_MAX];
    let buf = tmp.as_mut_ptr().cast::<c_char>();
    macro_rules! call {
        ($v:expr) => {
            // SAFETY: `spec` is one conversion whose argument types are the
            // ones pushed here; `tmp` is `CONVERSION_MAX` writable bytes.
            unsafe {
                match (stars.width, stars.precision) {
                    (None, None) => libc::snprintf(buf, CONVERSION_MAX, spec.as_ptr(), $v),
                    (Some(w), None) => libc::snprintf(buf, CONVERSION_MAX, spec.as_ptr(), w, $v),
                    (None, Some(p)) => libc::snprintf(buf, CONVERSION_MAX, spec.as_ptr(), p, $v),
                    (Some(w), Some(p)) => {
                        libc::snprintf(buf, CONVERSION_MAX, spec.as_ptr(), w, p, $v)
                    }
                }
            }
        };
    }
    let n = match arg {
        Arg::Int(n) => call!(n),
        Arg::Float(d) => call!(d),
        Arg::Char(c) => call!(c),
        Arg::Str(s) => call!(s),
        Arg::Ptr(p) => call!(p),
    };
    if n > 0 {
        out.extend_from_slice(&tmp[..(n as usize).min(CONVERSION_MAX - 1)]);
    }
}

/// The spec with its length modifier re-spelled as `ll`, so one `snprintf`
/// call covers every width without the spec and the read disagreeing.
fn widened(spec: &[u8]) -> Vec<u8> {
    let (conv, head) = spec.split_last().expect("a spec ends in its conversion");
    let mut wide: Vec<u8> = head
        .iter()
        .copied()
        .filter(|c| !matches!(c, b'h' | b'l' | b'z' | b't' | b'j' | b'q'))
        .collect();
    wide.extend_from_slice(b"ll");
    wide.push(*conv);
    wide
}

/// Walk `fmt`, reading one argument per conversion.
///
/// A format this cannot parse is copied through verbatim, which is what
/// MRI's own fallback does and is always better than dropping the message an
/// extension was trying to raise with.
///
/// # Safety
///
/// `fmt` must be NUL-terminated, and `ap` must hold the arguments its
/// conversions name, at the types they name.
pub(super) unsafe fn vformat(fmt: *const c_char, ap: &mut VaList<'_>) -> Result<Vec<u8>, Signal> {
    let mut out = Vec::new();
    if fmt.is_null() {
        return Ok(out);
    }
    // SAFETY: the caller's contract.
    let fmt = unsafe { CStr::from_ptr(fmt) }.to_bytes();
    let mut p = 0;
    while p < fmt.len() {
        if fmt[p] != b'%' {
            let run = fmt[p..]
                .iter()
                .position(|c| *c == b'%')
                .unwrap_or(fmt.len() - p);
            out.extend_from_slice(&fmt[p..p + run]);
            p += run;
            continue;
        }
        // `%%` is a literal, and consumes no argument.
        if fmt.get(p + 1) == Some(&b'%') {
            out.push(b'%');
            p += 2;
            continue;
        }

        let mut spec = vec![b'%'];
        let mut stars = Stars::default();
        let mut plus = false;
        let mut len = Len::Int;
        p += 1;
        // Flags.
        while let Some(c @ (b'-' | b'+' | b' ' | b'#' | b'0')) = fmt.get(p) {
            plus |= *c == b'+';
            spec.push(*c);
            p += 1;
        }
        // Width, then precision. `*` takes an `int` argument each.
        while let Some(c @ (b'0'..=b'9' | b'*')) = fmt.get(p) {
            if *c == b'*' {
                // SAFETY: the spec names an `int` here.
                stars.width = Some(unsafe { ap.next_arg::<c_int>() });
            }
            spec.push(*c);
            p += 1;
        }
        if fmt.get(p) == Some(&b'.') {
            spec.push(b'.');
            p += 1;
            while let Some(c @ (b'0'..=b'9' | b'*')) = fmt.get(p) {
                if *c == b'*' {
                    // SAFETY: the spec names an `int` here.
                    stars.precision = Some(unsafe { ap.next_arg::<c_int>() });
                }
                spec.push(*c);
                p += 1;
            }
        }
        // Length modifiers.
        let (modifier, took) = match (fmt.get(p), fmt.get(p + 1)) {
            (Some(b'h'), Some(b'h')) => (Len::Char, 2),
            (Some(b'h'), _) => (Len::Short, 1),
            (Some(b'l'), Some(b'l')) => (Len::LLong, 2),
            (Some(b'l'), _) => (Len::Long, 1),
            (Some(b'z'), _) => (Len::Size, 1),
            (Some(b't'), _) => (Len::Ptrdiff, 1),
            (Some(b'j' | b'q'), _) => (Len::LLong, 1),
            _ => (Len::Int, 0),
        };
        if took > 0 {
            len = modifier;
            spec.extend_from_slice(&fmt[p..p + took]);
            p += took;
        }

        let Some(&conv) = fmt.get(p) else {
            // A trailing `%`: copy what was collected and stop.
            out.extend_from_slice(&spec);
            break;
        };
        spec.push(conv);
        p += 1;

        // MRI's extension: the conversion is followed by the sentinel, so
        // the argument is a VALUE and not the integer the spec claims.
        if fmt.get(p) == Some(&VALUE_MARK) && matches!(conv, b'i' | b'd' | b'u') {
            p += 1;
            // SAFETY: `PRIsVALUE` names a `VALUE`.
            let v = unsafe { ap.next_arg::<c_ulong>() } as Value;
            let text = unsafe { value_text(v, plus)? };
            let text = CString::new(text).unwrap_or_else(|e| {
                // An embedded NUL cannot go through a `char *`, and
                // truncating at it is what C would print.
                let mut bytes = e.into_vec();
                bytes.truncate(bytes.iter().position(|b| *b == 0).unwrap_or(0));
                CString::new(bytes).expect("the NUL was just removed")
            });
            // Reuse the flags and width, as `%s`. The length modifier goes:
            // `%ls` is a wide-string conversion, and `+` was the mark.
            let mut sspec: Vec<u8> = spec[..spec.len() - 1]
                .iter()
                .copied()
                .filter(|c| !matches!(c, b'+' | b'h' | b'l' | b'z' | b't' | b'j' | b'q'))
                .collect();
            sspec.push(b's');
            emit(&mut out, &sspec, stars, Arg::Str(text.as_ptr()));
            continue;
        }

        // SAFETY: each read is at the type the spec names.
        let arg = unsafe {
            match conv {
                b'd' | b'i' => Arg::Int(match len {
                    Len::Char => c_longlong::from(ap.next_arg::<c_int>() as i8),
                    Len::Short => c_longlong::from(ap.next_arg::<c_int>() as i16),
                    Len::Int => c_longlong::from(ap.next_arg::<c_int>()),
                    Len::Long => c_longlong::from(ap.next_arg::<c_long>()),
                    Len::LLong => ap.next_arg::<c_longlong>(),
                    Len::Size => ap.next_arg::<usize>() as c_longlong,
                    Len::Ptrdiff => ap.next_arg::<isize>() as c_longlong,
                }),
                b'o' | b'u' | b'x' | b'X' => Arg::Int(match len {
                    Len::Char => c_longlong::from(ap.next_arg::<c_uint>() as u8),
                    Len::Short => c_longlong::from(ap.next_arg::<c_uint>() as u16),
                    Len::Int => c_longlong::from(ap.next_arg::<c_uint>()),
                    Len::Long => ap.next_arg::<c_ulong>() as c_longlong,
                    Len::LLong => ap.next_arg::<c_longlong>(),
                    Len::Size => ap.next_arg::<usize>() as c_longlong,
                    Len::Ptrdiff => ap.next_arg::<isize>() as c_longlong,
                }),
                b'f' | b'F' | b'e' | b'E' | b'g' | b'G' | b'a' | b'A' => {
                    Arg::Float(ap.next_arg::<f64>())
                }
                b'c' => Arg::Char(ap.next_arg::<c_int>()),
                b's' => {
                    let s = ap.next_arg::<*const c_char>();
                    Arg::Str(if s.is_null() { c"(null)".as_ptr() } else { s })
                }
                b'p' => Arg::Ptr(ap.next_arg::<*const c_void>()),
                _ => {
                    // Not a conversion this knows. Copy it through rather
                    // than guessing at an argument that may not be there.
                    out.extend_from_slice(&spec);
                    continue;
                }
            }
        };
        match arg {
            Arg::Int(_) => emit(&mut out, &widened(&spec), stars, arg),
            _ => emit(&mut out, &spec, stars, arg),
        }
    }
    Ok(out)
}

/// `to_s`, or `inspect` when the format carried the `+` flag.
///
/// # Safety
///
/// `v` must be a live `VALUE`.
unsafe fn value_text(v: Value, inspect: bool) -> Result<String, Signal> {
    let recv = unsafe { value_of(v) };
    let meth = if inspect { "inspect" } else { "to_s" };
    Ok(send(&recv, meth, &[])?.to_display_string())
}

/// Copy `msg` into a C buffer of `cap` bytes, NUL-terminated and truncated
/// as `vsnprintf` does; answer the full length, as it does.
///
/// # Safety
///
/// `buf` must name `cap` writable bytes.
unsafe fn write_c(buf: *mut c_char, cap: usize, msg: &[u8]) -> c_int {
    if cap > 0 {
        let n = msg.len().min(cap - 1);
        // SAFETY: the caller's contract; `n + 1 <= cap`.
        unsafe {
            std::ptr::copy_nonoverlapping(msg.as_ptr(), buf.cast::<u8>(), n);
            buf.add(n).write(0);
        }
    }
    msg.len() as c_int
}

pub(super) fn new_str(bytes: Vec<u8>, enc: zeo_rt::encoding::EncodingId) -> Result<Value, Signal> {
    to_value(&RubyValue::Str(zeo_rt::string_from_bytes(bytes, enc)))
}

/// # Safety
///
/// `str` must be a live String `VALUE`.
unsafe fn cat(str: Value, bytes: &[u8]) -> Result<Value, Signal> {
    let RubyValue::Str(s) = (unsafe { value_of(str) }) else {
        return Err(zeo_rt::builtins::type_error!("rb_str_catf needs a String"));
    };
    let mut g = s.lock();
    let mut all = g.bytes().to_vec();
    all.extend_from_slice(bytes);
    let enc = g.encoding();
    g.replace_bytes(all, enc);
    drop(g);
    Ok(str)
}

crate::cext_fn! {
    fn ruby_vsnprintf(buf: *mut c_char, cap: usize, fmt: *const c_char, ap: VaList<'_>) -> c_int {
        let mut ap = ap;
        let msg = unsafe { vformat(fmt, &mut ap)? };
        Ok(unsafe { write_c(buf, cap, &msg) })
    }

    fn rb_vsprintf(fmt: *const c_char, ap: VaList<'_>) -> Value {
        let mut ap = ap;
        new_str(unsafe { vformat(fmt, &mut ap)? }, zeo_rt::encoding::UTF_8)
    }

    /// The same formatting, with the caller naming the answer's encoding
    /// instead of taking the default.
    fn rb_enc_vsprintf(enc: Encoding, fmt: *const c_char, ap: VaList<'_>) -> Value {
        let mut ap = ap;
        new_str(unsafe { vformat(fmt, &mut ap)? }, encoding_of(enc))
    }

    fn rb_str_vcatf(str: Value, fmt: *const c_char, ap: VaList<'_>) -> Value {
        let mut ap = ap;
        unsafe { cat(str, &vformat(fmt, &mut ap)?) }
    }
}

crate::cext_va_fn! {
    fn ruby_snprintf(buf: *mut c_char, cap: usize, fmt: *const c_char; ap) -> c_int {
        let msg = unsafe { vformat(fmt, ap)? };
        Ok(unsafe { write_c(buf, cap, &msg) })
    }

    fn rb_sprintf(fmt: *const c_char; ap) -> Value {
        new_str(unsafe { vformat(fmt, ap)? }, zeo_rt::encoding::UTF_8)
    }

    fn rb_enc_sprintf(enc: Encoding, fmt: *const c_char; ap) -> Value {
        new_str(unsafe { vformat(fmt, ap)? }, encoding_of(enc))
    }

    fn rb_str_catf(str: Value, fmt: *const c_char; ap) -> Value {
        unsafe { cat(str, &vformat(fmt, ap)?) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Rust test cannot build a `VaList`; a C-variadic entry of its own
    /// can, so the table drives `ruby_snprintf` through one.
    fn formatted(
        fmt: &CStr,
        push: impl FnOnce(*mut c_char, usize, *const c_char) -> c_int,
    ) -> String {
        let mut buf = [0 as c_char; 256];
        let n = push(buf.as_mut_ptr(), buf.len(), fmt.as_ptr());
        let text = unsafe { CStr::from_ptr(buf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            n as usize,
            text.len(),
            "the answered length is not the text's"
        );
        text
    }

    macro_rules! snprintf {
        ($fmt:expr $(, $arg:expr)*) => {
            formatted($fmt, |b, c, f| unsafe { ruby_snprintf(b, c, f $(, $arg)*) })
        };
    }

    #[test]
    fn plain_text_and_the_percent_escape() {
        assert_eq!(snprintf!(c"no conversions"), "no conversions");
        assert_eq!(snprintf!(c"100%% done"), "100% done");
        assert_eq!(snprintf!(c""), "");
    }

    #[test]
    fn every_integer_width_reads_at_its_own_type() {
        assert_eq!(snprintf!(c"%d|%i", -7 as c_int, 8 as c_int), "-7|8");
        assert_eq!(snprintf!(c"%u", c_uint::MAX), "4294967295");
        assert_eq!(snprintf!(c"%ld", c_long::MIN), c_long::MIN.to_string());
        assert_eq!(snprintf!(c"%lu", c_ulong::MAX), c_ulong::MAX.to_string());
        assert_eq!(
            snprintf!(c"%lld", c_longlong::MIN),
            c_longlong::MIN.to_string()
        );
        assert_eq!(snprintf!(c"%zu", usize::MAX), usize::MAX.to_string());
        assert_eq!(snprintf!(c"%td", -3isize), "-3");
        assert_eq!(
            snprintf!(c"%hhd|%hd", 300 as c_int, 70000 as c_int),
            "44|4464"
        );
        assert_eq!(
            snprintf!(
                c"%x|%X|%o|%#x",
                255 as c_uint,
                255 as c_uint,
                8 as c_uint,
                255 as c_uint
            ),
            "ff|FF|10|0xff"
        );
    }

    #[test]
    fn width_precision_and_star_arguments() {
        assert_eq!(snprintf!(c"[%5d]", 42 as c_int), "[   42]");
        assert_eq!(snprintf!(c"[%-5d]", 42 as c_int), "[42   ]");
        assert_eq!(snprintf!(c"[%05d]", 42 as c_int), "[00042]");
        assert_eq!(snprintf!(c"[%*d]", 6 as c_int, 42 as c_int), "[    42]");
        assert_eq!(
            snprintf!(c"[%.*s]", 3 as c_int, c"abcdef".as_ptr()),
            "[abc]"
        );
        assert_eq!(
            snprintf!(c"[%*.*f]", 8 as c_int, 2 as c_int, 1.23456f64),
            "[    1.23]"
        );
        assert_eq!(snprintf!(c"[%.3s]", c"abcdef".as_ptr()), "[abc]");
    }

    #[test]
    fn floats_chars_strings_and_pointers() {
        assert_eq!(
            snprintf!(c"%f|%.1e|%g", 1.5f64, 1234.5f64, 0.5f64),
            "1.500000|1.2e+03|0.5"
        );
        assert_eq!(snprintf!(c"%c%c", b'o' as c_int, b'k' as c_int), "ok");
        assert_eq!(snprintf!(c"%s", c"text".as_ptr()), "text");
        assert_eq!(snprintf!(c"%s", std::ptr::null::<c_char>()), "(null)");
        assert_eq!(snprintf!(c"%p", 0x10usize as *const c_void), "0x10");
    }

    #[test]
    fn what_cannot_be_parsed_is_copied_through() {
        assert_eq!(snprintf!(c"trailing %"), "trailing %");
        assert_eq!(snprintf!(c"%y then %d", 5 as c_int), "%y then 5");
    }

    #[test]
    fn a_short_buffer_truncates_and_answers_the_full_length() {
        let mut buf = [0 as c_char; 4];
        let n = unsafe {
            ruby_snprintf(
                buf.as_mut_ptr(),
                buf.len(),
                c"%s".as_ptr(),
                c"abcdef".as_ptr(),
            )
        };
        assert_eq!(n, 6);
        assert_eq!(unsafe { CStr::from_ptr(buf.as_ptr()) }.to_bytes(), b"abc");
        assert_eq!(
            unsafe { ruby_snprintf(buf.as_mut_ptr(), 0, c"abc".as_ptr()) },
            3
        );
    }

    /// `PRIsVALUE` on an immediate: `to_s`, and `inspect` under `+`. A heap
    /// `VALUE` needs the class registry a unit test does not install.
    #[test]
    fn prisvalue_prints_to_s_and_inspect_under_plus() {
        let _scope = super::super::scope::Scope::enter();
        let five = super::super::value::fixnum(5) as c_ulong;
        assert_eq!(snprintf!(c"n=%li\x0b", five), "n=5");
        assert_eq!(snprintf!(c"n=%+li\x0b", five), "n=5");
        assert_eq!(snprintf!(c"[%4li\x0b]", five), "[   5]");
        let nil = super::super::value::Q_NIL as c_ulong;
        assert_eq!(snprintf!(c"[%li\x0b]", nil), "[]");
        assert_eq!(snprintf!(c"[%+li\x0b]", nil), "[nil]");
    }
}

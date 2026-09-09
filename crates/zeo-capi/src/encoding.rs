//! `ruby/encoding.h`: the encoding an extension reads bytes through.
//!
//! # Why this file exists
//!
//! `cargo xtask check-c-headers api` reads every public header, not
//! `<ruby.h>` alone. A gem that includes `ruby/encoding.h` -- `fast_blank`
//! is 40 lines of C that does -- would otherwise LINK and then jump to a
//! null address on its first call. A symbol the census cannot see is a
//! symbol nothing promises, and the promise is the whole point of
//! `cext/stubs.rs`. `cext/load.rs` opens with `RTLD_NOW`, so a symbol that
//! is missing all the same is a `LoadError` naming it.
//!
//! # `rb_encoding *` is a token, not a struct
//!
//! MRI's is an `OnigEncodingType` an extension may dereference for the
//! `min_enc_len`/`max_enc_len` pair and the function table. zeo's is an
//! `EncodingId` plus one, so it is never null for a real encoding and never a
//! valid pointer -- an extension that dereferences it faults immediately
//! rather than reading a plausible wrong number. That is deliberate: the
//! alternative is a `precise_mbclen` that silently disagrees with zeo's own
//! decoder.
//!
//! Everything an extension actually needs from that struct is here as a
//! function: the byte length of one character, the codepoint at a position,
//! the character count of a range.

use super::convert::{to_value, value_of};
use super::misc::{Encoding, encoding_of};
use super::object::{cstr, send};
use super::value::Value;
use std::ffi::{c_char, c_int, c_long, c_uint};
use zeo_rt::builtins::wrong_arg_type;
use zeo_rt::encoding::EncodingId;
use zeo_rt::{RubyValue, Signal};

/// `coderange.h`'s enum values. The coderange rides in the flags word as
/// `FL_USER8`/`FL_USER9` (`FL_USHIFT` is 12), so 7BIT is `1<<20` -- NOT a
/// small ordinal. An extension compares against the enum, so anything else
/// reads as UNKNOWN-adjacent garbage: json's generator refused every valid
/// String as "illegal/malformed utf-8" when this answered 0x10.
const ENC_CODERANGE_7BIT: c_int = 1 << 20;
const ENC_CODERANGE_VALID: c_int = 1 << 21;
const ENC_CODERANGE_BROKEN: c_int = (1 << 20) | (1 << 21);

/// `rb_encoding *` for an id. Never null, and never dereferenceable.
fn token(id: EncodingId) -> Encoding {
    (id.0 as usize + 1) as Encoding
}

/// # Safety
///
/// `p` and `e` must bracket a readable run of bytes.
unsafe fn span<'a>(p: *const c_char, e: *const c_char) -> &'a [u8] {
    if p.is_null() || e.is_null() || e <= p {
        return &[];
    }
    // SAFETY: the caller's contract.
    let len = unsafe { e.offset_from(p) } as usize;
    unsafe { std::slice::from_raw_parts(p.cast::<u8>(), len) }
}

/// The byte length of the FIRST character in `bytes`, under `enc`.
///
/// Answers `None` for an empty run. A truncated final character answers its
/// available length rather than the length the lead byte promised -- which is
/// what makes a caller's `s += n` terminate instead of walking past `e`.
fn first_char_len(bytes: &[u8], enc: EncodingId) -> Option<usize> {
    if bytes.is_empty() {
        return None;
    }
    let buf = zeo_rt::encoding::StrBuf::from_bytes(bytes.to_vec(), enc);
    Some(buf.char_ranges().first().map_or(1, |r| r.len()))
}

/// The codepoint of the first character, and its byte length.
fn first_codepoint(bytes: &[u8], enc: EncodingId) -> Option<(u32, usize)> {
    let n = first_char_len(bytes, enc)?;
    let buf = zeo_rt::encoding::StrBuf::from_bytes(bytes[..n].to_vec(), enc);
    // `char_vec` maps a byte no encoding can decode onto a replacement, so
    // the raw byte is the honest answer for a one-byte undecodable run.
    let cp = buf
        .char_vec()
        .first()
        .map_or_else(|| u32::from(bytes[0]), |c| *c as u32);
    Some((cp, n))
}

/// The encoding a `VALUE` carries. A non-String answers binary, which is what
/// MRI answers for a `VALUE` with no encoding of its own.
///
/// # Safety
///
/// `v` must be a live `VALUE`.
unsafe fn enc_of_value(v: Value) -> EncodingId {
    match unsafe { value_of(v) } {
        RubyValue::Str(s) => s.lock().encoding(),
        RubyValue::Symbol(s) => s.encoding(),
        _ => zeo_rt::encoding::ASCII_8BIT,
    }
}

crate::cext_fn! {
    // ---- naming ----------------------------------------------------------

    fn rb_enc_find(name: *const c_char) -> Encoding {
        let n = unsafe { cstr(name) };
        Ok(token(zeo_rt::encoding::find(&n).unwrap_or(zeo_rt::encoding::ASCII_8BIT)))
    }

    /// `rb_default_external_encoding()`: `Encoding.default_external`, which is
    /// the encoding bytes crossing the process boundary are tagged with.
    fn rb_default_external_encoding() -> Encoding {
        Ok(token(zeo_rt::encoding::default_external()))
    }

    /// `rb_default_internal_encoding()`: `Encoding.default_internal`, NULL
    /// when it is nil -- which is ruby's default and MRI's own answer for it.
    fn rb_default_internal_encoding() -> Encoding {
        Ok(match zeo_rt::encoding::default_internal() {
            Some(id) => token(id),
            None => std::ptr::null(),
        })
    }

    /// `rb_find_encoding(v)`: the encoding an `Encoding`, a String naming one,
    /// or an encoding-carrying object names -- and NULL rather than a raise
    /// when it names none. That is the whole difference from
    /// `rb_to_encoding`, and an extension branches on the null.
    fn rb_find_encoding(v: Value) -> Encoding {
        Ok(match unsafe { enc_arg(v) } {
            Ok(id) => token(id),
            Err(_) => std::ptr::null(),
        })
    }

    /// `rb_define_dummy_encoding(name)`: the index of a dummy encoding by
    /// that name, registering one if it is new.
    ///
    /// zeo's registry is generated and fixed at build time -- the same reason
    /// `rb_enc_alias` refuses -- so the registering half cannot happen. The
    /// asking half can, and it is the half that runs: the dummy encodings a
    /// gem asks for (`ISO-2022-JP` and its neighbours) are already in the
    /// table, because it was derived from ruby's own. A name that is not
    /// there says so rather than answering an index that resolves to nothing.
    fn rb_define_dummy_encoding(name: *const c_char) -> c_int {
        let n = unsafe { cstr(name) };
        match zeo_rt::encoding::find(&n) {
            Some(id) => Ok(c_int::from(id.0)),
            None => Err(zeo_rt::builtins::arg_error!(
                "zeo's encoding registry is generated and fixed; it cannot \
                 define the dummy encoding {n}"
            )),
        }
    }

    /// `rb_enc_find_index(name)`: the index, or -1 for a name no encoding
    /// answers to. The -1 is what separates it from `rb_enc_find`, which
    /// cannot report a miss.
    fn rb_enc_find_index(name: *const c_char) -> c_int {
        let n = unsafe { cstr(name) };
        Ok(zeo_rt::encoding::find(&n).map_or(-1, |e| c_int::from(e.0)))
    }

    /// `rb_enc_alias(alias, orig)`: register another name for an encoding.
    /// zeo's registry is derived from the oracle and is fixed at build
    /// time, so an alias cannot be added -- and answering a
    /// success would leave `Encoding.find` unable to resolve the name the
    /// caller believes it just made.
    fn rb_enc_alias(alias: *const c_char, _orig: *const c_char) -> c_int {
        Err(zeo_rt::builtins::arg_error!("zeo's encoding registry is generated and fixed; it cannot alias {}",
                unsafe { cstr(alias) }))
    }

    fn rb_enc_from_encoding(enc: Encoding) -> Value {
        let id = encoding_of(enc);
        to_value(&zeo_rt::builtins::encoding::encoding_value(id))
    }

    fn rb_enc_dummy_p(enc: Encoding) -> c_int {
        Ok(c_int::from(encoding_of(enc).is_dummy()))
    }

    /// `rb_enc_unicode_p`: is this a Unicode encoding? UTF-8, UTF-16 and
    /// UTF-32 are; every other one is not, including the ones that can
    /// represent the same characters.
    fn rb_enc_unicode_p(enc: Encoding) -> c_int {
        let name = encoding_of(enc).name();
        Ok(c_int::from(name.starts_with("UTF-") || name.starts_with("UTF_")))
    }

    fn rb_enc_capable(v: Value) -> c_int {
        // Everything with bytes of its own carries an encoding.
        Ok(c_int::from(matches!(
            unsafe { value_of(v) },
            RubyValue::Str(_) | RubyValue::Symbol(_) | RubyValue::Regexp(_)
        )))
    }

    // ---- reading the encoding off an object ------------------------------

    fn rb_enc_set_index(v: Value, index: c_int) -> () {
        let id = EncodingId(index.max(0) as u8);
        if let RubyValue::Str(s) = unsafe { value_of(v) } {
            s.lock().set_encoding(id);
        }
        Ok(())
    }

    /// `rb_enc_copy(dst, src)`: give `dst` the encoding `src` carries.
    fn rb_enc_copy(dst: Value, src: Value) -> () {
        let id = unsafe { enc_of_value(src) };
        if let RubyValue::Str(s) = unsafe { value_of(dst) } {
            s.lock().set_encoding(id);
        }
        Ok(())
    }

    /// `rb_enc_check(a, b)`: the encoding the two can be combined in, and an
    /// `Encoding::CompatibilityError` when there is none. `rb_enc_compatible`
    /// asks the same question and answers NULL instead of raising, which is
    /// the whole difference.
    fn rb_enc_check(a: Value, b: Value) -> Encoding {
        match unsafe { compatible(a, b) } {
            Some(id) => Ok(token(id)),
            None => Err(incompatible(a, b)),
        }
    }

    fn rb_enc_compatible(a: Value, b: Value) -> Encoding {
        Ok(unsafe { compatible(a, b) }.map_or(std::ptr::null(), token))
    }

    fn rb_enc_str_asciionly_p(v: Value) -> c_int {
        let s = unsafe { value_of(v) };
        let RubyValue::Str(s) = &s else {
            return Err(wrong_arg_type(&s, "String"));
        };
        let only = s.lock().ascii_only();
        Ok(c_int::from(only))
    }

    /// `rb_enc_str_coderange(str)`: `ENC_CODERANGE_7BIT`, `VALID` or
    /// `BROKEN`. An extension compares the answer against the enum, and the
    /// patched `RB_ENC_CODERANGE` forwards here, so the values must be
    /// `coderange.h`'s own flag masks.
    fn rb_enc_str_coderange(v: Value) -> c_int {
        let s = unsafe { value_of(v) };
        let RubyValue::Str(s) = &s else {
            return Err(wrong_arg_type(&s, "String"));
        };
        let g = s.lock();
        Ok(if g.ascii_only() {
            ENC_CODERANGE_7BIT
        } else if g.valid_encoding() {
            ENC_CODERANGE_VALID
        } else {
            ENC_CODERANGE_BROKEN
        })
    }

    // ---- walking bytes ---------------------------------------------------

    /// `rb_enc_mbclen(p, e, enc)`: the byte length of the character at `p`,
    /// and 1 for a broken one -- so a caller's `p += n` always advances.
    fn rb_enc_mbclen(p: *const c_char, e: *const c_char, enc: Encoding) -> c_int {
        let bytes = unsafe { span(p, e) };
        Ok(first_char_len(bytes, encoding_of(enc)).unwrap_or(1) as c_int)
    }

    /// `rb_enc_fast_mbclen`: the same without the bounds check, which zeo
    /// keeps anyway. The only difference MRI documents is speed.
    fn rb_enc_fast_mbclen(p: *const c_char, e: *const c_char, enc: Encoding) -> c_int {
        unsafe { Ok(rb_enc_mbclen(p, e, enc)) }
    }

    /// `rb_enc_precise_mbclen(p, e, enc)`: the length, or a NEGATIVE code.
    /// MRI's encoding: `ONIGENC_CONSTRUCT_MBCLEN_NEEDMORE(n)` is `-1 - n` and
    /// `INVALID` is `-1`. A caller distinguishes "truncated, need n more"
    /// from "not a character at all", and collapsing them would make a
    /// streaming decoder loop.
    fn rb_enc_precise_mbclen(p: *const c_char, e: *const c_char, enc: Encoding) -> c_int {
        let bytes = unsafe { span(p, e) };
        if bytes.is_empty() {
            return Ok(-1);
        }
        let id = encoding_of(enc);
        let n = first_char_len(bytes, id).unwrap_or(1);
        let buf = zeo_rt::encoding::StrBuf::from_bytes(bytes[..n].to_vec(), id);
        if buf.valid_encoding() {
            return Ok(n as c_int);
        }
        // A lead byte that promised more than the run holds is NEEDMORE; one
        // that is not a lead byte at all is INVALID.
        let want = wanted_len(bytes[0], id);
        if want > bytes.len() {
            return Ok(-1 - (want - bytes.len()) as c_int);
        }
        Ok(-1)
    }

    /// `rb_enc_codepoint_len(p, e, &len, enc)`: the codepoint AND how many
    /// bytes it took. A broken sequence raises, as MRI's does -- the
    /// non-raising spelling is `rb_enc_precise_mbclen`.
    fn rb_enc_codepoint_len(
        p: *const c_char,
        e: *const c_char,
        len: *mut c_int,
        enc: Encoding,
    ) -> c_uint {
        let bytes = unsafe { span(p, e) };
        let Some((cp, n)) = first_codepoint(bytes, encoding_of(enc)) else {
            return Err(zeo_rt::builtins::arg_error!("empty string"));
        };
        if !len.is_null() {
            // SAFETY: the caller's own `int`.
            unsafe { len.write(n as c_int) };
        }
        Ok(cp)
    }

    /// `rb_enc_strlen(p, e, enc)`: how many CHARACTERS the range holds.
    fn rb_enc_strlen(p: *const c_char, e: *const c_char, enc: Encoding) -> c_long {
        let bytes = unsafe { span(p, e) };
        let buf = zeo_rt::encoding::StrBuf::from_bytes(bytes.to_vec(), encoding_of(enc));
        Ok(buf.char_len() as c_long)
    }

    /// `rb_enc_nth(p, e, n, enc)`: a pointer at the nth CHARACTER, or `e`
    /// when the range is shorter.
    fn rb_enc_nth(p: *const c_char, e: *const c_char, n: c_long, enc: Encoding) -> *mut c_char {
        let bytes = unsafe { span(p, e) };
        let buf = zeo_rt::encoding::StrBuf::from_bytes(bytes.to_vec(), encoding_of(enc));
        let at = buf
            .char_ranges()
            .get(n.max(0) as usize)
            .map_or(bytes.len(), |r| r.start);
        // SAFETY: `at` is an offset inside the caller's own range.
        Ok(unsafe { p.cast_mut().add(at) })
    }

    /// `rb_enc_codelen(code, enc)`: how many bytes this codepoint needs.
    fn rb_enc_codelen(code: c_int, enc: Encoding) -> c_int {
        let id = encoding_of(enc);
        let Some(bytes) = codepoint_bytes(code, id) else {
            return Err(zeo_rt::builtins::range_error!("invalid codepoint 0x{code:X} in {}", id.name()));
        };
        Ok(bytes.len() as c_int)
    }

    /// `rb_enc_uint_chr(code, enc)`: the one-character String.
    fn rb_enc_uint_chr(code: c_uint, enc: Encoding) -> Value {
        let id = encoding_of(enc);
        let Some(bytes) = codepoint_bytes(code as c_int, id) else {
            return Err(zeo_rt::builtins::range_error!("invalid codepoint 0x{code:X} in {}", id.name()));
        };
        to_value(&RubyValue::Str(zeo_rt::string_from_bytes(bytes, id)))
    }

    fn rb_enc_tolower(c: c_int, _enc: Encoding) -> c_int {
        Ok(char::from_u32(c.max(0) as u32)
            .and_then(|ch| ch.to_lowercase().next())
            .map_or(c, |ch| ch as c_int))
    }

    fn rb_enc_toupper(c: c_int, _enc: Encoding) -> c_int {
        Ok(char::from_u32(c.max(0) as u32)
            .and_then(|ch| ch.to_uppercase().next())
            .map_or(c, |ch| ch as c_int))
    }

    /// `rb_enc_ascget(p, e, &len, enc)`: the ASCII character at `p`, or -1
    /// when it is not one. An extension parsing a delimiter uses it so a
    /// multibyte lead byte cannot be mistaken for the delimiter.
    fn rb_enc_ascget(
        p: *const c_char,
        e: *const c_char,
        len: *mut c_int,
        enc: Encoding,
    ) -> c_int {
        let bytes = unsafe { span(p, e) };
        let id = encoding_of(enc);
        let Some((cp, n)) = first_codepoint(bytes, id) else {
            return Ok(-1);
        };
        if cp > 0x7f || !id.ascii_compatible() {
            return Ok(-1);
        }
        if !len.is_null() {
            // SAFETY: the caller's own `int`.
            unsafe { len.write(n as c_int) };
        }
        Ok(cp as c_int)
    }

    // ---- building strings ------------------------------------------------

    fn rb_enc_str_buf_cat(v: Value, p: *const c_char, len: c_long, _enc: Encoding) -> Value {
        let bytes = unsafe { super::string::borrow_bytes(p, len) };
        let s = unsafe { value_of(v) };
        let RubyValue::Str(s) = &s else {
            return Err(wrong_arg_type(&s, "String"));
        };
        let mut g = s.lock();
        let mut all = g.bytes().to_vec();
        all.extend_from_slice(&bytes);
        let enc = g.encoding();
        g.replace_bytes(all, enc);
        drop(g);
        Ok(v)
    }

    fn rb_enc_interned_str_cstr(p: *const c_char, enc: Encoding) -> Value {
        let bytes = unsafe { super::string::borrow_bytes(p, -1) };
        let s = RubyValue::Str(zeo_rt::string_from_bytes(bytes, encoding_of(enc)));
        to_value(&send(&s, "-@", &[])?)
    }

    fn rb_enc_reg_new(p: *const c_char, len: c_long, enc: Encoding, options: c_int) -> Value {
        let bytes = unsafe { super::string::borrow_bytes(p, len) };
        let src = RubyValue::Str(zeo_rt::string_from_bytes(bytes, encoding_of(enc)));
        let cls = zeo_rt::constants::const_get(zeo_abi::OBJECT_CLASS.0, "Regexp").ok_or_else(|| {
            zeo_rt::builtins::name_error!("uninitialized constant Regexp")
        })?;
        to_value(&send(&cls, "new", &[src, RubyValue::Int(i64::from(options))])?)
    }

    // ---- the defaults ----------------------------------------------------

    fn rb_enc_default_external() -> Value {
        to_value(&zeo_rt::builtins::encoding::encoding_value(
            zeo_rt::encoding::default_external(),
        ))
    }

    fn rb_enc_default_internal() -> Value {
        to_value(&match zeo_rt::encoding::default_internal() {
            Some(id) => zeo_rt::builtins::encoding::encoding_value(id),
            None => RubyValue::Nil,
        })
    }

    fn rb_enc_set_default_external(v: Value) -> () {
        zeo_rt::encoding::set_default_external(unsafe { enc_arg(v)? });
        Ok(())
    }

    fn rb_enc_set_default_internal(v: Value) -> () {
        let id = match unsafe { value_of(v) } {
            RubyValue::Nil => None,
            _ => Some(unsafe { enc_arg(v)? }),
        };
        zeo_rt::encoding::set_default_internal(id);
        Ok(())
    }

    // ---- symbol names ----------------------------------------------------

    /// `rb_enc_symname_p(name, enc)`: could this text be written as a bare
    /// `:symbol`? The `2` spelling takes an explicit length, so an embedded
    /// NUL is a name and not a terminator.
    fn rb_enc_symname_p(name: *const c_char, _enc: Encoding) -> c_int {
        let s = unsafe { cstr(name) };
        Ok(c_int::from(super::builtins::symname_is_plain(&s)))
    }

    fn rb_enc_symname2_p(name: *const c_char, len: c_long, _enc: Encoding) -> c_int {
        let bytes = unsafe { super::string::borrow_bytes(name, len) };
        let s = String::from_utf8_lossy(&bytes);
        Ok(c_int::from(super::builtins::symname_is_plain(&s)))
    }

    // ---- path walking ----------------------------------------------------

    /// `rb_enc_path_next(p, e, enc)`: the next path separator at or after
    /// `p`, or `e`.
    fn rb_enc_path_next(p: *const c_char, e: *const c_char, _enc: Encoding) -> *mut c_char {
        let bytes = unsafe { span(p, e) };
        let at = bytes.iter().position(|b| *b == b'/').unwrap_or(bytes.len());
        // SAFETY: an offset inside the caller's own range.
        Ok(unsafe { p.cast_mut().add(at) })
    }

    /// `rb_enc_path_skip_prefix`: past a leading run of separators.
    fn rb_enc_path_skip_prefix(p: *const c_char, e: *const c_char, _enc: Encoding) -> *mut c_char {
        let bytes = unsafe { span(p, e) };
        let at = bytes.iter().take_while(|b| **b == b'/').count();
        // SAFETY: as above.
        Ok(unsafe { p.cast_mut().add(at) })
    }

    /// `rb_enc_path_end`: past a TRAILING run of separators, so a path
    /// keeps its root -- `"/"` ends at 1, not 0.
    fn rb_enc_path_end(p: *const c_char, e: *const c_char, _enc: Encoding) -> *mut c_char {
        let bytes = unsafe { span(p, e) };
        let mut end = bytes.len();
        while end > 1 && bytes[end - 1] == b'/' {
            end -= 1;
        }
        // SAFETY: as above.
        Ok(unsafe { p.cast_mut().add(end) })
    }

    fn rb_enc_path_last_separator(
        p: *const c_char,
        e: *const c_char,
        _enc: Encoding,
    ) -> *mut c_char {
        let bytes = unsafe { span(p, e) };
        match bytes.iter().rposition(|b| *b == b'/') {
            // SAFETY: as above.
            Some(at) => Ok(unsafe { p.cast_mut().add(at) }),
            None => Ok(std::ptr::null_mut()),
        }
    }

    // ---- the boundary encodings ------------------------------------------

    /// `rb_locale_encoding` / `rb_filesystem_encoding`: the encodings the
    /// world outside the process uses. Both are UTF-8 on every target zeo
    /// builds extensions for, which is also what `Encoding.locale_charmap`
    /// answers.
    fn rb_locale_encoding() -> Encoding {
        Ok(token(zeo_rt::encoding::UTF_8))
    }

    fn rb_locale_encindex() -> c_int {
        Ok(c_int::from(zeo_rt::encoding::UTF_8.0))
    }

    fn rb_filesystem_encoding() -> Encoding {
        Ok(token(zeo_rt::encoding::UTF_8))
    }

    fn rb_filesystem_encindex() -> c_int {
        Ok(c_int::from(zeo_rt::encoding::UTF_8.0))
    }

    fn rb_locale_charmap(_klass: Value) -> Value {
        to_value(&zeo_rt::builtins::string::str_value_in_enc(
            zeo_rt::encoding::UTF_8,
            zeo_rt::encoding::UTF_8.name(),
        ))
    }

    /// `rb_to_encoding(v)`: an `Encoding` object or a name, as the encoding
    /// itself. `rb_to_encoding_index` is the same question by index.
    fn rb_to_encoding(v: Value) -> Encoding {
        Ok(token(unsafe { enc_arg(v)? }))
    }

    fn rb_to_encoding_index(v: Value) -> c_int {
        Ok(c_int::from(unsafe { enc_arg(v)? }.0))
    }

    fn rb_obj_encoding(v: Value) -> Value {
        let id = unsafe { enc_of_value(v) };
        to_value(&zeo_rt::builtins::encoding::encoding_value(id))
    }

    // ---- re-encoding -----------------------------------------------------

    /// `rb_str_conv_enc(str, from, to)`: re-encode, and answer the RECEIVER
    /// unchanged when it cannot. MRI does the same -- the entry is
    /// best-effort by contract, which is why it takes no options and cannot
    /// raise.
    fn rb_str_conv_enc(v: Value, from: Encoding, to: Encoding) -> Value {
        Ok(reencode(v, from, to).unwrap_or(v))
    }

    fn rb_str_conv_enc_opts(
        v: Value,
        from: Encoding,
        to: Encoding,
        _ecflags: c_int,
        _ecopts: Value,
    ) -> Value {
        Ok(reencode(v, from, to).unwrap_or(v))
    }

    fn rb_str_export_to_enc(v: Value, to: Encoding) -> Value {
        let from = token(unsafe { enc_of_value(v) });
        Ok(reencode(v, from, to).unwrap_or(v))
    }

    /// `rb_str_encode(str, to, ecflags, ecopts)`: `String#encode`, which
    /// RAISES on an undefined character where `rb_str_conv_enc` gives up.
    fn rb_str_encode(v: Value, to: Value, _ecflags: c_int, opts: Value) -> Value {
        let s = unsafe { value_of(v) };
        let target = unsafe { value_of(to) };
        let mut args = vec![target];
        if !matches!(unsafe { value_of(opts) }, RubyValue::Nil) {
            args.push(unsafe { value_of(opts) });
        }
        to_value(&send(&s, "encode", &args)?)
    }

    /// `rb_str_coderange_scan_restartable(p, e, enc, &cr)`: scan as far as a
    /// whole character boundary allows, reporting the coderange found. A
    /// streaming decoder calls it per chunk, so answering the whole length
    /// on a truncated tail would lose the partial character.
    fn rb_str_coderange_scan_restartable(
        p: *const c_char,
        e: *const c_char,
        enc: Encoding,
        cr: *mut c_int,
    ) -> c_long {
        let bytes = unsafe { span(p, e) };
        let id = encoding_of(enc);
        let buf = zeo_rt::encoding::StrBuf::from_bytes(bytes.to_vec(), id);
        let ranges = buf.char_ranges();
        // The scan stops at the last COMPLETE character.
        let end = ranges.last().map_or(0, |r| r.end);
        if !cr.is_null() {
            let head = zeo_rt::encoding::StrBuf::from_bytes(bytes[..end].to_vec(), id);
            let code = if head.ascii_only() {
                ENC_CODERANGE_7BIT
            } else if head.valid_encoding() {
                ENC_CODERANGE_VALID
            } else {
                ENC_CODERANGE_BROKEN
            };
            // SAFETY: the caller's own `int`.
            unsafe { cr.write(code) };
        }
        Ok(end as c_long)
    }

    /// `rb_enc_raise(enc, exc, fmt, ...)`'s worker: the message was
    /// formatted in `csrc/cext_err.c`. The encoding tags the message string,
    /// which zeo's exceptions carry as UTF-8 either way.
    fn zeo_cext_enc_raise(_enc: Encoding, exc: Value, msg: *const c_char) -> () {
        unsafe { super::call::zeo_cext_raise_str(exc, msg) };
        Ok(())
    }
}

/// Re-encode a String, or `None` when it cannot be done. The best-effort
/// entries hand back the receiver on a `None`, which is MRI's own answer.
fn reencode(v: Value, from: Encoding, to: Encoding) -> Option<Value> {
    let (from, to) = (encoding_of(from), encoding_of(to));
    if from == to {
        return Some(v);
    }
    let RubyValue::Str(s) = (unsafe { value_of(v) }) else {
        return None;
    };
    let bytes = s.lock().bytes().to_vec();
    let out = zeo_rt::encoding::transcode(
        &bytes,
        from,
        to,
        &zeo_rt::encoding::TranscodeOptions::default(),
        None,
    )
    .ok()?;
    to_value(&RubyValue::Str(zeo_rt::string_from_bytes(out, to))).ok()
}

/// The encoding a `VALUE` argument names: an `Encoding` object, or a String
/// naming one.
///
/// # Safety
///
/// `v` must be a live `VALUE`.
unsafe fn enc_arg(v: Value) -> Result<EncodingId, Signal> {
    let val = unsafe { value_of(v) };
    if let RubyValue::Str(s) = &val {
        let name = s.lock().to_utf8_lossy().into_owned();
        return zeo_rt::encoding::find(&name)
            .ok_or_else(|| zeo_rt::builtins::arg_error!("unknown encoding name - {name}"));
    }
    let name = send(&val, "name", &[])?.to_display_string();
    zeo_rt::encoding::find(&name)
        .ok_or_else(|| zeo_rt::builtins::arg_error!("unknown encoding name - {name}"))
}

/// # Safety
///
/// Both must be live `VALUE`s.
unsafe fn compatible(a: Value, b: Value) -> Option<EncodingId> {
    let (x, y) = unsafe { (enc_of_value(a), enc_of_value(b)) };
    if x == y {
        return Some(x);
    }
    // Two different encodings combine when one side is 7-bit, which is the
    // rule `String#+` follows too.
    let (av, bv) = unsafe { (value_of(a), value_of(b)) };
    match (&av, &bv) {
        (RubyValue::Str(l), RubyValue::Str(r)) => l.lock().concat_enc_with(&r.lock()),
        _ if x.ascii_compatible() && y.ascii_compatible() => Some(x),
        _ => None,
    }
}

fn incompatible(a: Value, b: Value) -> Signal {
    let (x, y) = unsafe { (enc_of_value(a), enc_of_value(b)) };
    zeo_rt::dispatch::raise_error(
        "Encoding::CompatibilityError",
        format!(
            "incompatible character encodings: {} and {}",
            x.name(),
            y.name()
        ),
    )
}

/// How many bytes a lead byte promises, for the NEEDMORE arithmetic.
fn wanted_len(lead: u8, enc: EncodingId) -> usize {
    if !matches!(enc.kind(), zeo_rt::encoding::EncKind::Utf8) {
        return 1;
    }
    match lead {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        // A continuation byte in lead position is not a start at all.
        _ => 1,
    }
}

/// One codepoint's bytes in `enc`, or `None` when the encoding cannot hold
/// it.
fn codepoint_bytes(code: c_int, enc: EncodingId) -> Option<Vec<u8>> {
    let cp = u32::try_from(code).ok()?;
    if enc == zeo_rt::encoding::ASCII_8BIT {
        return (cp <= 0xff).then(|| vec![cp as u8]);
    }
    let ch = char::from_u32(cp)?;
    let mut buf = [0u8; 4];
    let utf8 = ch.encode_utf8(&mut buf).as_bytes().to_vec();
    if enc == zeo_rt::encoding::UTF_8 {
        return Some(utf8);
    }
    if enc.ascii_compatible() && cp <= 0x7f {
        return Some(vec![cp as u8]);
    }
    // Anything else goes through the transcoder, which is the same path
    // `Integer#chr(enc)` takes.
    zeo_rt::encoding::transcode(
        &utf8,
        zeo_rt::encoding::UTF_8,
        enc,
        &zeo_rt::encoding::TranscodeOptions::default(),
        None,
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A truncated final character must answer the bytes that ARE there, not
    /// the length its lead byte promised -- a caller's `p += n` would
    /// otherwise walk past `e`, which is the loop `fast_blank` runs.
    #[test]
    fn a_truncated_character_answers_its_available_length() {
        // The first two bytes of a three-byte UTF-8 character.
        let bytes = [0xe2u8, 0x82];
        let n = first_char_len(&bytes, zeo_rt::encoding::UTF_8);
        assert!(
            n.is_some_and(|n| n <= bytes.len()),
            "{n:?} runs past the end"
        );
    }

    /// The values an extension compares against come from `coderange.h`'s
    /// enum, which is built from the flags-word bits -- checked against the
    /// bindgen mirror so a header bump that moves them fails by name. json's
    /// generator switches on these; small ordinals read as garbage there.
    #[test]
    fn coderange_answers_are_the_headers_enum_values() {
        use super::super::layout;
        let bit8 = layout::FL_USER8 as c_int;
        let bit9 = layout::FL_USER9 as c_int;
        assert_eq!(ENC_CODERANGE_7BIT, bit8);
        assert_eq!(ENC_CODERANGE_VALID, bit9);
        assert_eq!(ENC_CODERANGE_BROKEN, bit8 | bit9);
    }

    #[test]
    fn a_codepoint_comes_back_with_its_byte_length() {
        for (bytes, want_cp, want_n) in [
            (&b"a"[..], 0x61u32, 1),
            ("\u{20ac}".as_bytes(), 0x20ac, 3),
            ("\u{1f600}".as_bytes(), 0x1f600, 4),
        ] {
            let got = first_codepoint(bytes, zeo_rt::encoding::UTF_8);
            assert_eq!(got, Some((want_cp, want_n)), "{bytes:?}");
        }
    }

    /// `rb_enc_precise_mbclen`'s negatives are two DIFFERENT answers, and a
    /// streaming decoder loops forever if they collapse.
    #[test]
    fn needmore_and_invalid_are_different_codes() {
        let utf8 = zeo_rt::encoding::UTF_8;
        // A three-byte lead with one byte present: needs two more, so -1-2.
        assert_eq!(wanted_len(0xe2, utf8), 3);
        // A continuation byte in lead position is not a start at all.
        assert_eq!(wanted_len(0x82, utf8), 1);
        // A single-byte encoding always wants one.
        assert_eq!(wanted_len(0xe2, zeo_rt::encoding::ASCII_8BIT), 1);
    }

    /// The token is never null for a real encoding, so a caller testing
    /// `enc == NULL` never mistakes binary for absent -- and it is never a
    /// valid pointer, so a dereference faults rather than reading a
    /// plausible wrong number.
    #[test]
    fn an_encoding_token_is_neither_null_nor_dereferenceable() {
        for id in [
            zeo_rt::encoding::ASCII_8BIT,
            zeo_rt::encoding::UTF_8,
            zeo_rt::encoding::US_ASCII,
        ] {
            let t = token(id);
            assert!(!t.is_null());
            assert!((t as usize) < 4096, "{t:?} looks like a real address");
            assert_eq!(encoding_of(t), id);
        }
    }

    #[test]
    fn a_codepoint_encodes_into_its_own_encoding() {
        assert_eq!(
            codepoint_bytes(0x41, zeo_rt::encoding::UTF_8),
            Some(vec![b'A'])
        );
        assert_eq!(
            codepoint_bytes(0x20ac, zeo_rt::encoding::UTF_8),
            Some("\u{20ac}".as_bytes().to_vec())
        );
        // Binary holds one byte and nothing wider.
        assert_eq!(
            codepoint_bytes(0xff, zeo_rt::encoding::ASCII_8BIT),
            Some(vec![0xff])
        );
        assert_eq!(codepoint_bytes(0x100, zeo_rt::encoding::ASCII_8BIT), None);
    }
}

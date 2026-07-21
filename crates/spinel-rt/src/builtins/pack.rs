//! `Array#pack` / `String#unpack` / `String#unpack1` (CRuby pack.c) -- the
//! template-directed byte (de)serializer. A template is a run of directive
//! letters, each optionally carrying an endian modifier (`<`/`>`), a native-
//! size modifier (`!`, accepted and ignored for the fixed-width directives),
//! and a count (a number, or `*` for "all the rest").
//!
//! Supported directives: `C c` (8-bit), `S s n v` (16-bit), `L l N V`
//! (32-bit), `Q q` (64-bit); `a A Z` (byte strings, null/space/null-term
//! padding); `m` (Base64); `H h` (hex, high/low nibble first); `w` (BER
//! compressed integer); `U` (UTF-8 character). Endianness: `N n` are always
//! big-endian, `V v` little-endian; the rest are native unless `<`/`>` says
//! otherwise.

use crate::{RubyValue, Signal};

/// One parsed template directive.
struct Directive {
    kind: char,
    count: Count,
    big_endian: bool,
    /// The native-size modifier (`!` or its synonym `_`) was present -- widens
    /// `l`/`L` to a native `long` (8 bytes here).
    native: bool,
}

#[derive(Clone, Copy)]
enum Count {
    /// An explicit repeat/width.
    Fixed(usize),
    /// `*` -- consume everything remaining.
    Star,
    /// No count given (defaults to 1, or per-directive).
    One,
}

fn parse_template(tmpl: &str) -> Result<Vec<Directive>, Signal> {
    let chars: Vec<char> = tmpl.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let kind = chars[i];
        i += 1;
        if kind.is_whitespace() {
            continue;
        }
        // Endian and native-size modifiers, in any order. `N n` and the
        // big-endian float directives `G g` default to big-endian.
        let mut big_endian = matches!(kind, 'N' | 'n' | 'G' | 'g');
        let mut native = false;
        while i < chars.len() && matches!(chars[i], '<' | '>' | '!' | '_') {
            match chars[i] {
                '<' => big_endian = false,
                '>' => big_endian = true,
                // `!` or `_`, the only other chars the loop guard admits.
                _ => native = true,
            }
            i += 1;
        }
        let count = if i < chars.len() && chars[i] == '*' {
            i += 1;
            Count::Star
        } else if i < chars.len() && chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            Count::Fixed(chars[start..i].iter().collect::<String>().parse().unwrap_or(0))
        } else {
            Count::One
        };
        if matches!(kind, 'v' | 'V') {
            big_endian = false;
        }
        out.push(Directive { kind, count, big_endian, native });
    }
    Ok(out)
}

fn err(msg: impl Into<String>) -> Signal {
    crate::dispatch::raise_error("ArgumentError", msg.into())
}

/// `Array#pack`'s result encoding: UTF-8 when EVERY directive is `U`,
/// otherwise ASCII-8BIT (CRuby's rule).
pub fn result_encoding(template: &str) -> crate::encoding::EncodingId {
    let letters: Vec<char> = template.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    if !letters.is_empty() && letters.iter().all(|c| *c == 'U') {
        crate::encoding::UTF_8
    } else {
        crate::encoding::ASCII_8BIT
    }
}

// ---------------------------------------------------------------------------
// pack
// ---------------------------------------------------------------------------

pub fn pack(elems: &[RubyValue], template: &str) -> Result<Vec<u8>, Signal> {
    let mut out: Vec<u8> = Vec::new();
    let mut idx = 0usize;
    for d in parse_template(template)? {
        match d.kind {
            'C' | 'c' | 'S' | 's' | 'L' | 'l' | 'Q' | 'q' | 'n' | 'N' | 'v' | 'V' | 'i' | 'I'
            | 'j' | 'J' => {
                let size = int_size(d.kind, d.native);
                let n = numeric_count(d.count, elems.len().saturating_sub(idx));
                for _ in 0..n {
                    let bits = next_int_bits(elems, &mut idx)?;
                    emit_int(&mut out, bits, size, d.big_endian);
                }
            }
            // IEEE-754 floats: `D d E G` are doubles, `F f e g` singles;
            // endianness comes from the letter/modifier (`d`/`f` native LE).
            'D' | 'd' | 'E' | 'G' => {
                let n = numeric_count(d.count, elems.len().saturating_sub(idx));
                for _ in 0..n {
                    let v = next_float(elems, &mut idx)?;
                    emit_int(&mut out, v.to_bits(), 8, d.big_endian);
                }
            }
            'F' | 'f' | 'e' | 'g' => {
                let n = numeric_count(d.count, elems.len().saturating_sub(idx));
                for _ in 0..n {
                    let v = next_float(elems, &mut idx)?;
                    emit_int(&mut out, (v as f32).to_bits() as u64, 4, d.big_endian);
                }
            }
            'a' | 'A' | 'Z' => {
                let s = next_str(elems, &mut idx)?;
                pack_str(&mut out, &s, d.kind, d.count);
            }
            'H' | 'h' => {
                let s = next_str(elems, &mut idx)?;
                pack_hex(&mut out, &s, d.kind == 'H', d.count);
            }
            'm' => {
                let s = next_str(elems, &mut idx)?;
                out.extend_from_slice(base64_encode(&s).as_bytes());
            }
            // `M` -- quoted-printable, `len` chars per line (default 72).
            'M' => {
                let s = next_str(elems, &mut idx)?;
                let len = match d.count {
                    Count::Fixed(n) if n > 1 => n,
                    _ => 72,
                };
                qpencode(&mut out, &s, len);
            }
            // `u` -- uuencode, `len` bytes per line (default 45, max 63).
            'u' => {
                let s = next_str(elems, &mut idx)?;
                let line = match d.count {
                    Count::Fixed(n) if n > 2 => (if n > 63 { 63 } else { n }) / 3 * 3,
                    _ => 45,
                };
                let line = line.max(3);
                let mut p = 0;
                while p < s.len() {
                    let todo = (s.len() - p).min(line);
                    uu_encode_line(&mut out, &s[p..p + todo]);
                    p += todo;
                }
            }
            'w' => {
                let n = numeric_count(d.count, elems.len().saturating_sub(idx));
                for _ in 0..n {
                    let v = next_int(elems, &mut idx)?;
                    if v < 0 {
                        return Err(err("can't compress negative numbers"));
                    }
                    emit_ber(&mut out, v as u64);
                }
            }
            'U' => {
                let n = numeric_count(d.count, elems.len().saturating_sub(idx));
                for _ in 0..n {
                    let v = next_int(elems, &mut idx)?;
                    let c = char::from_u32(v as u32).ok_or_else(|| err("invalid codepoint"))?;
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
            }
            other => return Err(err(format!("unsupported pack directive: {other}"))),
        }
    }
    Ok(out)
}

/// The byte width of an integer directive. `i`/`I` (C `int`) are 4; `j`/`J`
/// (`intptr_t`) are 8; a native `l!`/`L!` widens from 4 to a native `long`
/// (8 bytes on the LP64 targets spinel supports).
fn int_size(kind: char, native: bool) -> usize {
    match kind {
        'C' | 'c' => 1,
        'S' | 's' | 'n' | 'v' => 2,
        'i' | 'I' => 4,
        'L' | 'l' | 'N' | 'V' => if native { 8 } else { 4 },
        'Q' | 'q' | 'j' | 'J' => 8,
        _ => unreachable!(),
    }
}

fn emit_int(out: &mut Vec<u8>, val: u64, size: usize, big: bool) {
    for i in 0..size {
        let shift = if big { (size - 1 - i) * 8 } else { i * 8 };
        out.push((val >> shift) as u8);
    }
}

/// BER compressed integer: base-128, big-endian, high bit set on all but the
/// last byte.
fn emit_ber(out: &mut Vec<u8>, mut val: u64) {
    let mut groups = vec![(val & 0x7f) as u8];
    val >>= 7;
    while val > 0 {
        groups.push((val & 0x7f) as u8 | 0x80);
        val >>= 7;
    }
    groups.reverse();
    out.extend_from_slice(&groups);
}

fn pack_str(out: &mut Vec<u8>, s: &[u8], kind: char, count: Count) {
    let width = match count {
        Count::Star => {
            out.extend_from_slice(s);
            if kind == 'Z' {
                out.push(0); // `Z*` always appends a trailing NUL.
            }
            return;
        }
        Count::One => 1,
        Count::Fixed(n) => n,
    };
    let pad = if kind == 'A' { b' ' } else { 0 };
    for i in 0..width {
        out.push(s.get(i).copied().unwrap_or(pad));
    }
}

fn pack_hex(out: &mut Vec<u8>, s: &[u8], high_first: bool, count: Count) {
    let nibbles: Vec<u8> = s
        .iter()
        .map(|c| (*c as char).to_digit(16).unwrap_or(0) as u8)
        .collect();
    let take = match count {
        Count::Star => nibbles.len(),
        Count::One => 1,
        Count::Fixed(n) => n,
    };
    let mut i = 0;
    while i < take {
        let hi = nibbles.get(i).copied().unwrap_or(0);
        let lo = nibbles.get(i + 1).copied().unwrap_or(0);
        let byte = if high_first { (hi << 4) | lo } else { (lo << 4) | hi };
        out.push(byte);
        i += 2;
    }
}

// ---------------------------------------------------------------------------
// unpack
// ---------------------------------------------------------------------------

pub fn unpack(bytes: &[u8], template: &str) -> Result<Vec<RubyValue>, Signal> {
    let mut out: Vec<RubyValue> = Vec::new();
    let mut pos = 0usize;
    for d in parse_template(template)? {
        match d.kind {
            'C' | 'c' | 'S' | 's' | 'L' | 'l' | 'Q' | 'q' | 'n' | 'N' | 'v' | 'V' | 'i' | 'I'
            | 'j' | 'J' => {
                let size = int_size(d.kind, d.native);
                let signed = d.kind.is_ascii_lowercase() && !matches!(d.kind, 'n' | 'v');
                let avail = (bytes.len().saturating_sub(pos)) / size;
                let n = numeric_count(d.count, avail);
                for _ in 0..n {
                    if pos + size > bytes.len() {
                        out.push(RubyValue::Nil);
                    } else {
                        let v = read_int(&bytes[pos..pos + size], d.big_endian, signed);
                        out.push(RubyValue::Int(v));
                        pos += size;
                    }
                }
            }
            'D' | 'd' | 'E' | 'G' | 'F' | 'f' | 'e' | 'g' => {
                let size = if matches!(d.kind, 'D' | 'd' | 'E' | 'G') { 8 } else { 4 };
                let avail = (bytes.len().saturating_sub(pos)) / size;
                let n = numeric_count(d.count, avail);
                for _ in 0..n {
                    if pos + size > bytes.len() {
                        out.push(RubyValue::Nil);
                    } else {
                        let bits = read_bits(&bytes[pos..pos + size], d.big_endian);
                        let f = if size == 8 {
                            f64::from_bits(bits)
                        } else {
                            f32::from_bits(bits as u32) as f64
                        };
                        out.push(RubyValue::Float(f));
                        pos += size;
                    }
                }
            }
            'a' | 'A' | 'Z' => {
                let width = match d.count {
                    Count::Star => bytes.len() - pos,
                    Count::One => 1,
                    Count::Fixed(n) => n.min(bytes.len() - pos),
                };
                let mut field = bytes[pos..pos + width].to_vec();
                pos += width;
                match d.kind {
                    // `A` strips trailing spaces and NULs.
                    'A' => while matches!(field.last(), Some(b' ') | Some(0)) {
                        field.pop();
                    },
                    // `Z*` reads up to the first NUL; `Z` with a width keeps
                    // the fixed field but truncates at the first NUL.
                    'Z' => {
                        if let Some(nul) = field.iter().position(|b| *b == 0) {
                            field.truncate(nul);
                        }
                    }
                    _ => {}
                }
                out.push(RubyValue::Str(crate::string_from_bytes(field, crate::encoding::ASCII_8BIT)));
            }
            'H' | 'h' => {
                let take = match d.count {
                    Count::Star => (bytes.len() - pos) * 2,
                    Count::One => 1,
                    Count::Fixed(n) => n,
                };
                out.push(RubyValue::Str(crate::string_new(unpack_hex(&bytes[pos..], take, d.kind == 'H'))));
                pos = bytes.len().min(pos + take.div_ceil(2));
            }
            'm' => {
                let decoded = base64_decode(&bytes[pos..]);
                pos = bytes.len();
                out.push(RubyValue::Str(crate::string_from_bytes(decoded, crate::encoding::ASCII_8BIT)));
            }
            'M' => {
                let decoded = qpdecode(&bytes[pos..]);
                pos = bytes.len();
                out.push(RubyValue::Str(crate::string_from_bytes(decoded, crate::encoding::ASCII_8BIT)));
            }
            'u' => {
                let decoded = uu_decode(&bytes[pos..]);
                pos = bytes.len();
                out.push(RubyValue::Str(crate::string_from_bytes(decoded, crate::encoding::ASCII_8BIT)));
            }
            'w' => {
                let n = numeric_count(d.count, usize::MAX);
                for _ in 0..n {
                    if pos >= bytes.len() {
                        break;
                    }
                    let (val, used) = read_ber(&bytes[pos..]);
                    pos += used;
                    out.push(crate::builtins::integer::int_value(num_bigint::BigInt::from(val)));
                }
            }
            'U' => {
                let text = String::from_utf8_lossy(&bytes[pos..]);
                let n = numeric_count(d.count, text.chars().count());
                for c in text.chars().take(n) {
                    out.push(RubyValue::Int(c as i64));
                }
                pos = bytes.len();
            }
            other => return Err(err(format!("unsupported unpack directive: {other}"))),
        }
    }
    Ok(out)
}

fn read_int(bytes: &[u8], big: bool, signed: bool) -> i64 {
    let size = bytes.len();
    let mut val: u64 = 0;
    for i in 0..size {
        let b = bytes[i] as u64;
        let shift = if big { (size - 1 - i) * 8 } else { i * 8 };
        val |= b << shift;
    }
    if signed && size < 8 {
        let sign_bit = 1u64 << (size * 8 - 1);
        if val & sign_bit != 0 {
            // Sign-extend.
            return (val | !((1u64 << (size * 8)) - 1)) as i64;
        }
    }
    val as i64
}

fn read_ber(bytes: &[u8]) -> (u64, usize) {
    let mut val: u64 = 0;
    let mut used = 0;
    for &b in bytes {
        val = (val << 7) | (b & 0x7f) as u64;
        used += 1;
        if b & 0x80 == 0 {
            break;
        }
    }
    (val, used)
}

fn unpack_hex(bytes: &[u8], nibbles: usize, high_first: bool) -> String {
    let mut out = String::new();
    for i in 0..nibbles {
        let byte = match bytes.get(i / 2) {
            Some(b) => *b,
            None => break,
        };
        let nib = if (i % 2 == 0) == high_first { byte >> 4 } else { byte & 0x0f };
        out.push(std::char::from_digit(nib as u32, 16).unwrap());
    }
    out
}

// ---------------------------------------------------------------------------
// element access + counts
// ---------------------------------------------------------------------------

fn numeric_count(count: Count, remaining: usize) -> usize {
    match count {
        Count::One => 1,
        Count::Fixed(n) => n,
        Count::Star => remaining,
    }
}

fn next_int(elems: &[RubyValue], idx: &mut usize) -> Result<i64, Signal> {
    let v = elems.get(*idx).ok_or_else(|| err("too few arguments"))?;
    *idx += 1;
    match v {
        RubyValue::Int(n) => Ok(*n),
        RubyValue::BigInt(b) => Ok(num_traits::ToPrimitive::to_i64(&**b).unwrap_or(0)),
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!("no implicit conversion of {} into Integer", crate::builtins::class_name_of(other)),
        )),
    }
}

/// The low 64 bits of the next element coerced to an exact integer, for the
/// integer pack directives. A Float truncates toward zero and CRuby coerces it
/// *through an exact Integer*, so a value beyond the i64 range wraps modulo
/// 2**64 rather than saturating (a plain C cast would be undefined behaviour);
/// NaN/Infinity raise FloatDomainError. `emit_int` then slices off the low
/// `size` bytes.
fn next_int_bits(elems: &[RubyValue], idx: &mut usize) -> Result<u64, Signal> {
    let v = elems.get(*idx).ok_or_else(|| err("too few arguments"))?;
    *idx += 1;
    match v {
        RubyValue::Int(n) => Ok(*n as u64),
        RubyValue::BigInt(b) => Ok(low_u64(b)),
        RubyValue::Float(f) => match crate::builtins::float::float_to_integer(*f)? {
            RubyValue::Int(n) => Ok(n as u64),
            RubyValue::BigInt(b) => Ok(low_u64(&b)),
            _ => unreachable!("float_to_integer yields only Int/BigInt"),
        },
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!("no implicit conversion of {} into Integer", crate::builtins::class_name_of(other)),
        )),
    }
}

/// A BigInt's low 64 bits, two's-complement (num-bigint's `BitAnd` uses Ruby's
/// infinite-two's-complement semantics, so a negative value masks correctly).
fn low_u64(b: &num_bigint::BigInt) -> u64 {
    let mask = num_bigint::BigInt::from(u64::MAX);
    num_traits::ToPrimitive::to_u64(&(b & &mask)).unwrap_or(0)
}

fn next_float(elems: &[RubyValue], idx: &mut usize) -> Result<f64, Signal> {
    let v = elems.get(*idx).ok_or_else(|| err("too few arguments"))?;
    *idx += 1;
    match v {
        RubyValue::Float(f) => Ok(*f),
        RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Rational(_) => {
            Ok(crate::builtins::numeric::num_to_f64_unchecked(v))
        }
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!("no implicit conversion to float from {}", crate::builtins::class_name_of(other)),
        )),
    }
}

/// Read `bytes` (1..=8 of them) as an unsigned integer of the given byte order
/// -- the raw bit pattern behind a float directive's `from_bits`.
fn read_bits(bytes: &[u8], big: bool) -> u64 {
    let mut val: u64 = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let shift = if big { (bytes.len() - 1 - i) * 8 } else { i * 8 };
        val |= (b as u64) << shift;
    }
    val
}

fn next_str(elems: &[RubyValue], idx: &mut usize) -> Result<Vec<u8>, Signal> {
    let v = elems.get(*idx).ok_or_else(|| err("too few arguments"))?;
    *idx += 1;
    match v {
        RubyValue::Str(s) => Ok(s.lock().bytes().to_vec()),
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!("no implicit conversion of {} into String", crate::builtins::class_name_of(other)),
        )),
    }
}

// ---------------------------------------------------------------------------
// Base64 (the `m` directive) -- MIME base64, 60-char lines, `=` padding.
// ---------------------------------------------------------------------------

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    // CRuby's `m` (default) breaks lines at 60 chars and ends with a newline.
    let mut wrapped = String::new();
    for line in out.as_bytes().chunks(60) {
        wrapped.push_str(std::str::from_utf8(line).unwrap());
        wrapped.push('\n');
    }
    if wrapped.is_empty() {
        wrapped.push('\n');
    }
    wrapped
}

fn base64_decode(data: &[u8]) -> Vec<u8> {
    let val = |c: u8| B64.iter().position(|&b| b == c).map(|p| p as u32);
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0;
    for &c in data {
        if c == b'=' {
            break;
        }
        let Some(v) = val(c) else { continue };
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Quoted-printable (`M`) and uuencode (`u`) -- ports of pack.c's `qpencode`
// and `encodes`/uudecode.
// ---------------------------------------------------------------------------

const HEX: &[u8; 16] = b"0123456789ABCDEF";
/// The 6-bit -> character table CRuby's uuencode uses (value 0 is a backtick,
/// not a space).
const UU: &[u8; 64] =
    b"`!\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_";

fn qpencode(out: &mut Vec<u8>, s: &[u8], len: usize) {
    let mut n = 0usize;
    let mut prev: i32 = -1;
    for &c in s {
        if c > 126 || (c < 32 && c != b'\n' && c != b'\t') || c == b'=' {
            out.push(b'=');
            out.push(HEX[(c >> 4) as usize]);
            out.push(HEX[(c & 0x0f) as usize]);
            n += 3;
            prev = -1;
        } else if c == b'\n' {
            if prev == b' ' as i32 || prev == b'\t' as i32 {
                out.push(b'=');
                out.push(c);
            }
            out.push(c);
            n = 0;
            prev = c as i32;
        } else {
            out.push(c);
            n += 1;
            prev = c as i32;
        }
        if n > len {
            out.push(b'=');
            out.push(b'\n');
            n = 0;
            prev = b'\n' as i32;
        }
    }
    if n > 0 {
        out.push(b'=');
        out.push(b'\n');
    }
}

fn qpdecode(src: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut i = 0;
    while i < src.len() {
        if src[i] == b'=' {
            i += 1;
            if i >= src.len() {
                break;
            }
            // A `=\r\n` soft break: skip the CR so the `\n` case runs.
            if i + 1 < src.len() && src[i] == b'\r' && src[i + 1] == b'\n' {
                i += 1;
            }
            if src[i] != b'\n' {
                let Some(c1) = hex2num(src[i]) else { break };
                i += 1;
                if i >= src.len() {
                    break;
                }
                let Some(c2) = hex2num(src[i]) else { break };
                buf.push((c1 << 4) | c2);
            }
        } else {
            buf.push(src[i]);
        }
        i += 1;
    }
    buf
}

fn hex2num(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|d| d as u8)
}

fn uu_encode_line(out: &mut Vec<u8>, s: &[u8]) {
    out.push(s.len() as u8 + b' ');
    let mut i = 0;
    while i + 3 <= s.len() {
        let (a, b, c) = (s[i], s[i + 1], s[i + 2]);
        out.push(UU[(a >> 2) as usize & 0o77]);
        out.push(UU[(((a << 4) & 0o60) | ((b >> 4) & 0o17)) as usize & 0o77]);
        out.push(UU[(((b << 2) & 0o74) | ((c >> 6) & 0o3)) as usize & 0o77]);
        out.push(UU[(c & 0o77) as usize]);
        i += 3;
    }
    match s.len() - i {
        2 => {
            let (a, b) = (s[i], s[i + 1]);
            out.push(UU[(a >> 2) as usize & 0o77]);
            out.push(UU[(((a << 4) & 0o60) | ((b >> 4) & 0o17)) as usize & 0o77]);
            out.push(UU[((b << 2) & 0o74) as usize & 0o77]);
            out.push(b'`');
        }
        1 => {
            let a = s[i];
            out.push(UU[(a >> 2) as usize & 0o77]);
            out.push(UU[((a << 4) & 0o60) as usize & 0o77]);
            out.push(b'`');
            out.push(b'`');
        }
        _ => {}
    }
    out.push(b'\n');
}

fn uu_decode(src: &[u8]) -> Vec<u8> {
    // A uuencode sextet: a printable char in `' '..'a'` carries `(c - ' ') & 077`.
    fn sextet(src: &[u8], i: &mut usize) -> u8 {
        if *i < src.len() && src[*i] >= b' ' && src[*i] < b'a' {
            let v = (src[*i].wrapping_sub(b' ')) & 0o77;
            *i += 1;
            v
        } else {
            0
        }
    }
    let mut buf = Vec::new();
    let mut i = 0;
    while i < src.len() && src[i] > b' ' && src[i] < b'a' {
        let line_len = ((src[i].wrapping_sub(b' ')) & 0o77) as usize;
        i += 1;
        let mut produced = 0;
        while produced < line_len {
            let (a, b, c, d) = (
                sextet(src, &mut i),
                sextet(src, &mut i),
                sextet(src, &mut i),
                sextet(src, &mut i),
            );
            let hunk = [a << 2 | b >> 4, b << 4 | c >> 2, c << 6 | d];
            let mlen = (line_len - produced).min(3);
            buf.extend_from_slice(&hunk[..mlen]);
            produced += mlen;
        }
        // Skip an optional checksum byte then the line terminator.
        if i < src.len() && src[i] != b'\r' && src[i] != b'\n' {
            i += 1;
        }
        if i < src.len() && src[i] == b'\r' {
            i += 1;
        }
        if i < src.len() && src[i] == b'\n' {
            i += 1;
        }
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ints(vs: &[i64]) -> Vec<RubyValue> {
        vs.iter().map(|v| RubyValue::Int(*v)).collect()
    }
    fn str_val(s: &str) -> RubyValue {
        RubyValue::Str(crate::string_new(s.to_string()))
    }
    /// `RubyValue` isn't `PartialEq`; project unpack results to a comparable
    /// `(i64, bytes)`-ish form for assertions.
    fn i64s(vs: &[RubyValue]) -> Vec<i64> {
        vs.iter().map(|v| match v {
            RubyValue::Int(n) => *n,
            _ => panic!("expected Int"),
        }).collect()
    }
    fn strs(vs: &[RubyValue]) -> Vec<String> {
        vs.iter().map(|v| match v {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            _ => panic!("expected Str"),
        }).collect()
    }

    #[test]
    fn pack_integers_with_endianness() {
        assert_eq!(pack(&ints(&[65, 66, 67]), "C*").unwrap(), b"ABC");
        assert_eq!(pack(&ints(&[1, 2]), "nN").unwrap(), vec![0, 1, 0, 0, 0, 2]);
        assert_eq!(pack(&ints(&[1, 2]), "vV").unwrap(), vec![1, 0, 2, 0, 0, 0]);
        assert_eq!(pack(&ints(&[-1]), "c").unwrap(), vec![255]);
        assert_eq!(pack(&ints(&[258]), "S>").unwrap(), vec![1, 2]);
    }

    #[test]
    fn unpack_integers() {
        assert_eq!(i64s(&unpack(b"hello", "C*").unwrap()), [104, 101, 108, 108, 111]);
        assert_eq!(i64s(&unpack(&[1, 2, 3, 4], "N").unwrap()), [16909060]);
        assert_eq!(i64s(&unpack(&[255], "c").unwrap()), [-1]);
    }

    #[test]
    fn pack_and_unpack_strings() {
        assert_eq!(pack(&[str_val("abc")], "a5").unwrap(), vec![97, 98, 99, 0, 0]);
        assert_eq!(pack(&[str_val("abc")], "A5").unwrap(), vec![97, 98, 99, 32, 32]);
        assert_eq!(pack(&[str_val("abc")], "Z*").unwrap(), vec![97, 98, 99, 0]);
        assert_eq!(strs(&unpack(b"abc\0\0", "A5").unwrap()), ["abc"]);
        assert_eq!(strs(&unpack(b"abc\0de", "Z*").unwrap()), ["abc"]);
    }

    #[test]
    fn hex_and_ber_and_base64() {
        assert_eq!(pack(&[str_val("ff01")], "H*").unwrap(), vec![255, 1]);
        assert_eq!(strs(&unpack(&[255, 1], "H*").unwrap()), ["ff01"]);
        assert_eq!(pack(&ints(&[300]), "w").unwrap(), vec![130, 44]);
        assert_eq!(i64s(&unpack(&[130, 44], "w").unwrap()), [300]);
        assert_eq!(pack(&[str_val("hello world")], "m").unwrap(), b"aGVsbG8gd29ybGQ=\n");
        assert_eq!(strs(&unpack(b"aGVsbG8=\n", "m").unwrap()), ["hello"]);
    }

    #[test]
    fn utf8_codepoints() {
        assert_eq!(pack(&ints(&[0x3042]), "U").unwrap(), "\u{3042}".as_bytes());
        assert_eq!(i64s(&unpack("\u{3042}".as_bytes(), "U*").unwrap()), [0x3042]);
    }

    fn floats(vs: &[f64]) -> Vec<RubyValue> {
        vs.iter().map(|v| RubyValue::Float(*v)).collect()
    }
    fn f64s(vs: &[RubyValue]) -> Vec<f64> {
        vs.iter()
            .map(|v| match v {
                RubyValue::Float(f) => *f,
                _ => panic!("expected Float"),
            })
            .collect()
    }

    #[test]
    fn float_directives_endianness_and_width() {
        // `1.5` little-endian double vs `G` big-endian double.
        assert_eq!(pack(&floats(&[1.5]), "D").unwrap(), vec![0, 0, 0, 0, 0, 0, 248, 63]);
        assert_eq!(pack(&floats(&[1.5]), "G").unwrap(), vec![63, 248, 0, 0, 0, 0, 0, 0]);
        assert_eq!(pack(&floats(&[1.5]), "F").unwrap(), vec![0, 0, 192, 63]);
        assert_eq!(f64s(&unpack(&pack(&floats(&[3.5]), "d").unwrap(), "d").unwrap()), [3.5]);
        assert_eq!(f64s(&unpack(&pack(&floats(&[2.0, 3.0]), "E*").unwrap(), "E*").unwrap()), [2.0, 3.0]);
    }

    #[test]
    fn integer_directive_truncates_and_wraps_a_float() {
        // Truncate toward zero for the small case.
        assert_eq!(pack(&floats(&[1.5]), "C*").unwrap(), vec![1]);
        assert_eq!(pack(&floats(&[-3.75]), "c").unwrap(), vec![253]); // -3 as u8
        // A value past i64 wraps modulo 2**64 (coerced through an exact Integer).
        assert_eq!(
            pack(&floats(&[2.0e19]), "Q").unwrap(),
            1553255926290448384u64.to_le_bytes()
        );
        // 1e300 is a multiple of 2**64, so its low 64 bits are zero.
        assert_eq!(pack(&floats(&[1.0e300]), "Q").unwrap(), vec![0; 8]);
        // (NaN/Infinity raising FloatDomainError needs the runtime exception
        // machinery, so that path is exercised by the e2e test, not here.)
    }

    #[test]
    fn native_size_modifiers() {
        assert_eq!(pack(&ints(&[1]), "l!").unwrap().len(), 8);
        assert_eq!(pack(&ints(&[1]), "l_").unwrap().len(), 8);
        assert_eq!(pack(&ints(&[1]), "i").unwrap().len(), 4);
        assert_eq!(pack(&ints(&[1]), "j").unwrap().len(), 8);
        assert_eq!(pack(&ints(&[1]), "s!").unwrap().len(), 2);
    }

    #[test]
    fn quoted_printable_and_uuencode() {
        assert_eq!(pack(&[str_val("hello world")], "M").unwrap(), b"hello world=\n");
        assert_eq!(pack(&[str_val("a=b c")], "M").unwrap(), b"a=3Db c=\n");
        let decoded = unpack(b"caf=C3=A9=\n", "M").unwrap();
        let RubyValue::Str(s) = &decoded[0] else { panic!("expected Str") };
        assert_eq!(s.lock().bytes(), &[99, 97, 102, 0xC3, 0xA9]);
        // uuencode round-trips over arbitrary bytes, including a partial line.
        for s in ["", "x", "ab", "abc", "hello world foo bar"] {
            let packed = pack(&[str_val(s)], "u").unwrap();
            assert_eq!(strs(&unpack(&packed, "u").unwrap())[0], s);
        }
    }
}

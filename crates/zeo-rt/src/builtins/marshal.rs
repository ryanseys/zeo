//! `Marshal` (#2483) -- object serialization in CRuby's wire format v4.8. A
//! `Writer`/`Reader` pair over a shared symbol table (`:`/`;`) and object-link
//! table (`@`), so shared references and cycles round-trip by identity. Values
//! whose bytes the conformance corpus compares (symbols, floats, arrays) match
//! CRuby exactly; the user-object (`o`), user-marshal (`U`: Rational/Complex),
//! and bignum (`l`) forms are self-consistent (dump/load are the only readers).

use crate::builtins::{arity, builtin_methods};
use crate::collections::{array_get, array_len, array_new, array_push, hash_new, hash_pairs, hash_set};
use crate::dispatch::raise_error;
use crate::signal::Signal;
use crate::value::RubyValue;
use num_bigint::{BigInt, Sign};
use std::collections::HashMap;
use std::sync::Arc;

const MAJOR: u8 = 4;
const MINOR: u8 = 8;

builtin_methods! {
    pub(crate) fn lookup_class;

    // `Marshal.dump(obj[, io])` -> a BINARY String of the serialized object.
    "dump" => fn dump(_recv, args, _block) {
        arity!(args, 1..=2);
        let mut w = Writer::default();
        w.out.push(MAJOR);
        w.out.push(MINOR);
        w.write(&args[0])?;
        let s = crate::string_from_bytes(w.out, crate::encoding::ASCII_8BIT);
        Ok(RubyValue::Str(s))
    }

    // `Marshal.load(str)` -> the deserialized object.
    "load" => fn load(_recv, args, _block) {
        arity!(args, 1..=2);
        let RubyValue::Str(s) = &args[0] else {
            return Err(raise_error("TypeError", "instance of IO needed".to_string()));
        };
        let bytes = s.lock().bytes().to_vec();
        let mut r = Reader { bytes: &bytes, pos: 0, symbols: Vec::new(), objects: Vec::new() };
        if r.byte()? != MAJOR || r.byte()? != MINOR {
            return Err(raise_error("TypeError", "incompatible marshal file format".to_string()));
        }
        r.read()
    }
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Writer {
    out: Vec<u8>,
    /// Symbol name -> its `:`-write index, for `;` symlinks.
    symbols: HashMap<String, usize>,
    /// Linkable-object Arc pointer -> its `@`-link index. Only Array/Hash/
    /// Object are deduped by identity; every linkable object still advances
    /// `next_link` so indices align with the reader.
    links: HashMap<usize, usize>,
    next_link: usize,
}

impl Writer {
    fn write(&mut self, v: &RubyValue) -> Result<(), Signal> {
        match v {
            RubyValue::Nil => self.out.push(b'0'),
            RubyValue::Bool(true) => self.out.push(b'T'),
            RubyValue::Bool(false) => self.out.push(b'F'),
            RubyValue::Int(n) if fits_i32(*n) => {
                self.out.push(b'i');
                self.write_long(*n);
            }
            RubyValue::Int(n) => self.write_bignum(&BigInt::from(*n)),
            RubyValue::BigInt(b) => self.write_bignum(b),
            RubyValue::Symbol(s) => {
                let name = s.name();
                self.write_symbol(&name);
            }
            RubyValue::Float(f) => {
                self.next_link += 1;
                self.out.push(b'f');
                let s = float_to_marshal(*f);
                self.write_bytes(s.as_bytes());
            }
            RubyValue::Str(rs) => {
                if self.link(ptr_of(v)) {
                    return Ok(());
                }
                self.out.push(b'"');
                let bytes = rs.lock().bytes().to_vec();
                self.write_bytes(&bytes);
            }
            RubyValue::Array(a) => {
                if self.link(ptr_of(v)) {
                    return Ok(());
                }
                self.out.push(b'[');
                let len = array_len(a);
                self.write_long(len);
                for i in 0..len {
                    let e = array_get(a, i);
                    self.write(&e)?;
                }
            }
            RubyValue::Hash(h) => {
                if self.link(ptr_of(v)) {
                    return Ok(());
                }
                self.out.push(b'{');
                let pairs = hash_pairs(h);
                self.write_long(pairs.len() as i64);
                for (k, val) in pairs {
                    self.write(&k)?;
                    self.write(&val)?;
                }
            }
            RubyValue::Rational(r) => {
                self.next_link += 1;
                self.out.push(b'U');
                self.write_symbol("Rational");
                self.write_inline_array(&[bigint_to_value(&r.num), bigint_to_value(&r.den)])?;
            }
            RubyValue::Complex(c) => {
                self.next_link += 1;
                self.out.push(b'U');
                self.write_symbol("Complex");
                self.write_inline_array(&[c.real.clone(), c.imag.clone()])?;
            }
            RubyValue::Object(o) => {
                if self.link(ptr_of(v)) {
                    return Ok(());
                }
                let cid = crate::dispatch::RubyObject::class_id(o.as_ref());
                let name = crate::dispatch::class_name(cid).unwrap_or_default();
                self.out.push(b'o');
                self.write_symbol(&name);
                let ivars = o.ivar_pairs();
                self.write_long(ivars.len() as i64);
                for (iname, ival) in ivars {
                    self.write_symbol(&iname);
                    self.write(&ival)?;
                }
            }
            other => {
                return Err(raise_error(
                    "TypeError",
                    format!("no _dump_data is defined for class {}", crate::builtins::class_name_of(other)),
                ))
            }
        }
        Ok(())
    }

    /// If this Arc pointer was already written, emit an `@`-link and return
    /// true. Otherwise assign it the next link index and return false (the
    /// caller writes the body).
    fn link(&mut self, ptr: usize) -> bool {
        if let Some(&idx) = self.links.get(&ptr) {
            self.out.push(b'@');
            self.write_long(idx as i64);
            return true;
        }
        let idx = self.next_link;
        self.next_link += 1;
        self.links.insert(ptr, idx);
        false
    }

    fn write_symbol(&mut self, name: &str) {
        if let Some(&idx) = self.symbols.get(name) {
            self.out.push(b';');
            self.write_long(idx as i64);
            return;
        }
        let idx = self.symbols.len();
        self.symbols.insert(name.to_string(), idx);
        self.out.push(b':');
        self.write_bytes(name.as_bytes());
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        self.write_long(bytes.len() as i64);
        self.out.extend_from_slice(bytes);
    }

    /// Write a SYNTHETIC array (a Rational/Complex `marshal_dump` payload) that
    /// exists only for the dump: it still consumes a link index (so the reader's
    /// indices stay aligned) but is NOT entered in the pointer-dedup map -- its
    /// storage is freed and reused between calls, which would otherwise
    /// false-match the next temporary.
    fn write_inline_array(&mut self, elems: &[RubyValue]) -> Result<(), Signal> {
        self.next_link += 1;
        self.out.push(b'[');
        self.write_long(elems.len() as i64);
        for e in elems {
            self.write(e)?;
        }
        Ok(())
    }

    /// CRuby's `w_long` packed encoding: 0 -> 0x00; 1..122 -> n+5; -123..-1 ->
    /// (n-5)&0xff; else a leading signed byte count and little-endian bytes.
    fn write_long(&mut self, mut x: i64) {
        if x == 0 {
            self.out.push(0);
            return;
        }
        if (1..123).contains(&x) {
            self.out.push((x + 5) as u8);
            return;
        }
        if (-123..0).contains(&x) {
            self.out.push((x - 5) as u8);
            return;
        }
        let mut buf = [0u8; 9];
        let mut len = 0usize;
        for i in 1..=8 {
            buf[i] = (x & 0xff) as u8;
            x >>= 8;
            if x == 0 {
                buf[0] = i as u8;
                len = i;
                break;
            }
            if x == -1 {
                buf[0] = (-(i as i64)) as u8;
                len = i;
                break;
            }
        }
        self.out.extend_from_slice(&buf[..=len]);
    }

    fn write_bignum(&mut self, b: &BigInt) {
        self.next_link += 1;
        self.out.push(b'l');
        let (sign, mut bytes) = b.to_bytes_le();
        self.out.push(if sign == Sign::Minus { b'-' } else { b'+' });
        if bytes.len() % 2 == 1 {
            bytes.push(0);
        }
        self.write_long((bytes.len() / 2) as i64);
        self.out.extend_from_slice(&bytes);
    }
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    symbols: Vec<String>,
    objects: Vec<RubyValue>,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, Signal> {
        let b = *self.bytes.get(self.pos).ok_or_else(|| {
            raise_error("ArgumentError", "marshal data too short".to_string())
        })?;
        self.pos += 1;
        Ok(b)
    }

    fn read(&mut self) -> Result<RubyValue, Signal> {
        match self.byte()? {
            b'0' => Ok(RubyValue::Nil),
            b'T' => Ok(RubyValue::Bool(true)),
            b'F' => Ok(RubyValue::Bool(false)),
            b'i' => Ok(int_to_value(self.read_long()?)),
            b':' => self.read_symbol_new(),
            b';' => {
                let idx = self.read_long()? as usize;
                let name = self.symbols.get(idx).cloned().ok_or_else(|| {
                    raise_error("ArgumentError", "bad symbol link".to_string())
                })?;
                Ok(RubyValue::Symbol(crate::Symbol::intern(&name)))
            }
            b'@' => {
                let idx = self.read_long()? as usize;
                self.objects.get(idx).cloned().ok_or_else(|| {
                    raise_error("ArgumentError", "bad object link".to_string())
                })
            }
            b'f' => {
                let bytes = self.read_bytes()?;
                let s = String::from_utf8_lossy(&bytes);
                let f = parse_marshal_float(&s);
                let v = RubyValue::Float(f);
                self.objects.push(v.clone());
                Ok(v)
            }
            b'"' => {
                let bytes = self.read_bytes()?;
                let v = RubyValue::Str(crate::string_from_bytes(bytes, crate::encoding::UTF_8));
                self.objects.push(v.clone());
                Ok(v)
            }
            b'[' => {
                let arr = array_new(Vec::new());
                let v = RubyValue::Array(arr.clone());
                self.objects.push(v.clone());
                let len = self.read_long()?;
                for _ in 0..len {
                    let e = self.read()?;
                    array_push(&arr, e);
                }
                Ok(v)
            }
            b'{' => {
                let h = hash_new(Vec::new());
                let v = RubyValue::Hash(h.clone());
                self.objects.push(v.clone());
                let n = self.read_long()?;
                for _ in 0..n {
                    let k = self.read()?;
                    let val = self.read()?;
                    hash_set(&h, k, val);
                }
                Ok(v)
            }
            b'l' => self.read_bignum(),
            b'U' => self.read_user_marshal(),
            b'o' => self.read_object(),
            other => Err(raise_error(
                "ArgumentError",
                format!("dump format error (0x{other:x})"),
            )),
        }
    }

    fn read_symbol_new(&mut self) -> Result<RubyValue, Signal> {
        let bytes = self.read_bytes()?;
        let name = String::from_utf8_lossy(&bytes).into_owned();
        self.symbols.push(name.clone());
        Ok(RubyValue::Symbol(crate::Symbol::intern(&name)))
    }

    /// Read a symbol NAME (for a class/ivar position): a fresh `:` symbol or a
    /// `;` link, returning the string.
    fn read_symbol_name(&mut self) -> Result<String, Signal> {
        match self.byte()? {
            b':' => {
                let bytes = self.read_bytes()?;
                let name = String::from_utf8_lossy(&bytes).into_owned();
                self.symbols.push(name.clone());
                Ok(name)
            }
            b';' => {
                let idx = self.read_long()? as usize;
                self.symbols.get(idx).cloned().ok_or_else(|| {
                    raise_error("ArgumentError", "bad symbol link".to_string())
                })
            }
            other => Err(raise_error(
                "ArgumentError",
                format!("expected a symbol (0x{other:x})"),
            )),
        }
    }

    fn read_user_marshal(&mut self) -> Result<RubyValue, Signal> {
        let idx = self.objects.len();
        self.objects.push(RubyValue::Nil);
        let cls = self.read_symbol_name()?;
        let inner = self.read()?;
        let RubyValue::Array(a) = &inner else {
            return Err(raise_error("ArgumentError", "malformed user marshal".to_string()));
        };
        let get = |i| array_get(a, i);
        let v = match cls.as_str() {
            "Rational" => crate::builtins::rational::rational_new(
                to_bigint(&get(0))?,
                to_bigint(&get(1))?,
            )?,
            "Complex" => crate::builtins::complex::complex_new(get(0), get(1))?,
            other => {
                return Err(raise_error(
                    "ArgumentError",
                    format!("undefined class/module {other}"),
                ))
            }
        };
        self.objects[idx] = v.clone();
        Ok(v)
    }

    fn read_object(&mut self) -> Result<RubyValue, Signal> {
        let idx = self.objects.len();
        self.objects.push(RubyValue::Nil);
        let cls = self.read_symbol_name()?;
        let cid = crate::dispatch::class_id_by_name(&cls).ok_or_else(|| {
            raise_error("ArgumentError", format!("undefined class/module {cls}"))
        })?;
        let obj = crate::dispatch::allocate_of(cid).ok_or_else(|| {
            raise_error("TypeError", format!("allocator undefined for {cls}"))
        })?;
        self.objects[idx] = obj.clone();
        let count = self.read_long()?;
        if let RubyValue::Object(o) = &obj {
            for _ in 0..count {
                let iname = self.read_symbol_name()?;
                let value = self.read()?;
                // ivar names arrive `@`-prefixed; the by-name setter keys on the
                // bare name.
                o.ivar_set_named(iname.trim_start_matches('@'), value);
            }
        }
        Ok(obj)
    }

    fn read_bignum(&mut self) -> Result<RubyValue, Signal> {
        let sign = if self.byte()? == b'-' { Sign::Minus } else { Sign::Plus };
        let shorts = self.read_long()? as usize;
        let mut bytes = Vec::with_capacity(shorts * 2);
        for _ in 0..shorts * 2 {
            bytes.push(self.byte()?);
        }
        let b = BigInt::from_bytes_le(sign, &bytes);
        let v = bigint_to_value(&b);
        self.objects.push(v.clone());
        Ok(v)
    }

    fn read_bytes(&mut self) -> Result<Vec<u8>, Signal> {
        let len = self.read_long()? as usize;
        let end = self.pos + len;
        if end > self.bytes.len() {
            return Err(raise_error("ArgumentError", "marshal data too short".to_string()));
        }
        let out = self.bytes[self.pos..end].to_vec();
        self.pos = end;
        Ok(out)
    }

    /// The inverse of `Writer::write_long`.
    fn read_long(&mut self) -> Result<i64, Signal> {
        let c = self.byte()? as i8;
        if c == 0 {
            return Ok(0);
        }
        if c > 0 {
            if (6..=127).contains(&c) {
                return Ok(c as i64 - 5);
            }
            let mut x: i64 = 0;
            for i in 0..c as usize {
                x |= (self.byte()? as i64) << (8 * i);
            }
            Ok(x)
        } else {
            if (-128..=-6).contains(&c) {
                return Ok(c as i64 + 5);
            }
            let n = (-c) as usize;
            let mut x: i64 = -1;
            for i in 0..n {
                x &= !(0xffi64 << (8 * i));
                x |= (self.byte()? as i64) << (8 * i);
            }
            Ok(x)
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn fits_i32(n: i64) -> bool {
    (i32::MIN as i64..=i32::MAX as i64).contains(&n)
}

/// The identity of a linkable ref value, as its Arc data-pointer address.
fn ptr_of(v: &RubyValue) -> usize {
    match v {
        RubyValue::Str(s) => Arc::as_ptr(s) as *const () as usize,
        RubyValue::Array(a) => Arc::as_ptr(a) as *const () as usize,
        RubyValue::Hash(h) => Arc::as_ptr(h) as *const () as usize,
        RubyValue::Object(o) => Arc::as_ptr(o) as *const () as usize,
        _ => 0,
    }
}

fn int_to_value(n: i64) -> RubyValue {
    RubyValue::Int(n)
}

fn bigint_to_value(b: &BigInt) -> RubyValue {
    match i64::try_from(b) {
        Ok(n) => RubyValue::Int(n),
        Err(_) => RubyValue::BigInt(Arc::new(b.clone())),
    }
}

fn to_bigint(v: &RubyValue) -> Result<BigInt, Signal> {
    match v {
        RubyValue::Int(n) => Ok(BigInt::from(*n)),
        RubyValue::BigInt(b) => Ok((**b).clone()),
        other => Err(raise_error(
            "TypeError",
            format!("can't convert {} into Integer", crate::builtins::convert_name_of(other)),
        )),
    }
}

/// CRuby's Marshal float text: the shortest round-tripping decimal, using
/// exponent form only when the fixed form would need trailing zeros
/// (`decpt > ndigits`) or more than three leading zeros (`decpt <= -4`).
/// Rust's `{:e}` supplies the shortest mantissa/exponent.
fn float_to_marshal(d: f64) -> String {
    if d == 0.0 {
        return if d.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    if d.is_infinite() {
        return if d < 0.0 { "-inf".into() } else { "inf".into() };
    }
    if d.is_nan() {
        return "nan".into();
    }
    let sign = if d < 0.0 { "-" } else { "" };
    let e = format!("{:e}", d.abs());
    let (mant, exp) = e.split_once('e').expect("{:e} always has an 'e'");
    let exp: i32 = exp.parse().expect("{:e} exponent parses");
    let digs: String = mant.chars().filter(|c| *c != '.').collect();
    let decpt = exp + 1;
    let ndigits = digs.len() as i32;
    let body = if decpt <= 0 {
        if decpt > -4 {
            format!("0.{}{}", "0".repeat((-decpt) as usize), digs)
        } else {
            exp_form(&digs, decpt)
        }
    } else if decpt >= ndigits {
        if decpt == ndigits {
            digs.clone()
        } else {
            exp_form(&digs, decpt)
        }
    } else {
        format!("{}.{}", &digs[..decpt as usize], &digs[decpt as usize..])
    };
    format!("{sign}{body}")
}

fn exp_form(digs: &str, decpt: i32) -> String {
    let mant = if digs.len() > 1 {
        format!("{}.{}", &digs[..1], &digs[1..])
    } else {
        digs.to_string()
    };
    format!("{mant}e{}", decpt - 1)
}

fn parse_marshal_float(s: &str) -> f64 {
    match s {
        "inf" => f64::INFINITY,
        "-inf" => f64::NEG_INFINITY,
        "nan" => f64::NAN,
        _ => s.parse().unwrap_or(0.0),
    }
}

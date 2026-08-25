//! `Marshal` -- object serialization in CRuby's wire format v4.8. A
//! `Writer`/`Reader` pair over a shared symbol table (`:`/`;`) and object-link
//! table (`@`), so shared references and cycles round-trip by identity.
//!
//! Supported tags mirror CRuby's `marshal.c`: `0`/`T`/`F` (nil/bool), `i`
//! (fixnum), `l` (bignum), `f` (float), `:`/`;` (symbol + link), `"` (string),
//! `[`/`{` (array/hash), `/` (regexp), `c`/`m` (class/module reference), `o`
//! (plain object + ivars), `S` (Struct), `C` (value-builtin subclass), `U`
//! (`marshal_dump`/`marshal_load`), `u` (`_dump`/`_load`), and the `I`
//! ivar-wrapper that carries a string's or regexp's encoding (`E => true`
//! UTF-8, `E => false` US-ASCII, `encoding => "<name>"` otherwise; ASCII-8BIT
//! strings carry no wrapper) plus any user ivars on a `C` body.
//! Rational/Complex keep their own `U` fast paths so they load without a
//! Ruby-level `marshal_load`. Not yet handled: `e` (a singleton-extended
//! object), which needs a runtime `extend` on the loaded instance.

use crate::builtins::{arg_error, type_error};
use crate::collections::{
    array_get, array_len, array_new, array_push, hash_new, hash_pairs, hash_set, string_from_bytes,
};
use crate::dispatch::{
    RObj, allocate_of, class_id_by_name, class_is_module, class_name, responds_to, send_value,
};
use crate::encoding::{ASCII_8BIT, EncodingId, US_ASCII, UTF_8};
use crate::signal::Signal;
use crate::symbol::Symbol;
use crate::value::RubyValue;
use num_bigint::{BigInt, Sign};
use std::collections::HashMap;
use std::sync::Arc;

use zeo_macros::ruby_module;

/// The blank instance a loaded object is filled into -- CRuby's `rb_obj_alloc`,
/// which asks the class's own allocator and never dispatches `allocate` (a user
/// override of that method is not part of the Marshal protocol).
///
/// `dispatch::allocate_of` alone is not that answer: it is documented to say
/// `None` for a BUILTIN, whose blank instance lives with `Class#allocate`. A
/// plain `Object` carrying ivars is exactly that case, so dumping one and
/// loading it back raised `allocator undefined for Object`.
fn marshal_allocate(cid: crate::ClassId) -> Option<RubyValue> {
    crate::builtins::rclass::builtin_allocate(cid).or_else(|| allocate_of(cid))
}

const MAJOR: u8 = 4;
const MINOR: u8 = 8;

ruby_module! {
    Marshal = zeo_abi::MARSHAL_MODULE;

    // The format version every dump carries in its first two bytes. Ruby
    // exposes both, and a reader that checks a stream by hand reads them from
    // here rather than hardcoding (rubygems' `SafeMarshal::Reader` does).
    const MAJOR_VERSION = RubyValue::Int(MAJOR as i64);
    const MINOR_VERSION = RubyValue::Int(MINOR as i64);

    // `Marshal.dump(obj[, io])` -> a BINARY String of the serialized object.
    // `module_function`, not `def self.`: CRuby gives `dump` alone a private
    // instance copy (`Marshal.private_instance_methods` is `[:dump]`), while
    // `load`/`restore` are singleton-only -- the pair is asymmetric.
    module_function def "dump" cfunc (_recv, arg1, _arg2?) {
        let mut w = Writer::default();
        w.out.push(MAJOR);
        w.out.push(MINOR);
        w.write(arg1)?;
        let s = crate::string_from_bytes(w.out, crate::encoding::ASCII_8BIT);
        Ok(RubyValue::Str(s))
    }

    // `Marshal.load(str)` -> the deserialized object.
    // `Marshal.restore` is CRuby's own alias of `.load`.
    def self."load" params "source, proc = nil, freeze: nil" | "restore" params "source, proc = nil, freeze: nil" (_recv, arg1, _arg2?) {
        let RubyValue::Str(s) = arg1 else {
            return Err(type_error!("instance of IO needed"));
        };
        let bytes = s.lock().bytes().to_vec();
        let mut r = Reader { bytes: &bytes, pos: 0, symbols: Vec::new(), objects: Vec::new() };
        if r.byte()? != MAJOR || r.byte()? != MINOR {
            return Err(type_error!("incompatible marshal file format"));
        }
        r.read()
    }
}

/// An instance variable carried by an `I`-wrapper: a string/regexp encoding, or
/// (for a `C` builtin-subclass body) a plain user instance variable.
enum Iv {
    /// `E => true` (UTF-8) or `E => false` (US-ASCII).
    EncBool(bool),
    /// `encoding => "<name>"` for any other encoding.
    EncName(&'static str),
    /// A user instance variable (`@x => value`), name `@`-prefixed as stored.
    Named(String, RubyValue),
}

/// Marshal's variable-length LONG, appended to `out` -- shared with
/// `Time#_dump`'s year-extension tail, which embeds one inside `u` data.
pub(crate) fn marshal_long_into(mut x: i64, out: &mut Vec<u8>) {
    if x == 0 {
        out.push(0);
        return;
    }
    if (1..123).contains(&x) {
        out.push((x + 5) as u8);
        return;
    }
    if (-123..0).contains(&x) {
        out.push((x - 5) as u8);
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
    out.extend_from_slice(&buf[..=len]);
}

/// The `I`-wrapper ivar for a string/regexp of encoding `enc`, or `None` for
/// ASCII-8BIT (which CRuby writes bare, with no wrapper).
fn encoding_ivar(enc: EncodingId) -> Option<Iv> {
    if enc == UTF_8 {
        Some(Iv::EncBool(true))
    } else if enc == US_ASCII {
        Some(Iv::EncBool(false))
    } else if enc == ASCII_8BIT {
        None
    } else {
        Some(Iv::EncName(enc.name()))
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
    /// Linkable ref/object Arc pointer (or class id) -> its `@`-link index.
    links: HashMap<usize, usize>,
    /// Encoding name -> the `@`-link index of the string that first named it,
    /// so a repeated non-standard encoding dedups like CRuby's shared name.
    enc_links: HashMap<String, usize>,
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
                if self.check_link(ptr_of(v)) {
                    return Ok(());
                }
                let ivars: Vec<Iv> = encoding_ivar(rs.lock().encoding()).into_iter().collect();
                if !ivars.is_empty() {
                    self.out.push(b'I');
                }
                self.register_link(ptr_of(v));
                self.out.push(b'"');
                let bytes = rs.lock().bytes().to_vec();
                self.write_bytes(&bytes);
                self.write_ivars(&ivars)?;
            }
            RubyValue::Regexp(re) => {
                if self.check_link(ptr_of(v)) {
                    return Ok(());
                }
                // Regexp source encoding isn't tracked separately; derive it the
                // way a literal does -- US-ASCII when the source is 7-bit,
                // UTF-8 otherwise.
                let enc = if re.source.is_ascii() {
                    US_ASCII
                } else {
                    UTF_8
                };
                let ivars: Vec<Iv> = encoding_ivar(enc).into_iter().collect();
                if !ivars.is_empty() {
                    self.out.push(b'I');
                }
                self.register_link(ptr_of(v));
                self.out.push(b'/');
                self.write_bytes(re.source.as_bytes());
                self.out.push(regexp_options(re));
                self.write_ivars(&ivars)?;
            }
            RubyValue::Array(a) => {
                if self.check_link(ptr_of(v)) {
                    return Ok(());
                }
                self.register_link(ptr_of(v));
                self.out.push(b'[');
                let len = array_len(a);
                self.write_long(len);
                for i in 0..len {
                    let e = array_get(a, i);
                    self.write(&e)?;
                }
            }
            RubyValue::Hash(h) => {
                if self.check_link(ptr_of(v)) {
                    return Ok(());
                }
                self.register_link(ptr_of(v));
                // A hash's per-instance default travels WITH it, under its own
                // tag and written after the entries (`marshal.c`'s
                // `TYPE_HASH_DEF`). Dropping it is the worse of the two
                // failures Marshal can have here: `Hash.new(0)` loads back
                // bare and the first `h[k] += 1` raises, somewhere else and
                // later. A proc default cannot cross at all.
                let (default, has_proc) = {
                    let g = h.lock();
                    (g.default.clone(), g.default_proc.is_some())
                };
                if has_proc {
                    return Err(type_error!("can't dump hash with default proc"));
                }
                let defaulted = !default.is_nil();
                self.out.push(if defaulted { b'}' } else { b'{' });
                let pairs = hash_pairs(h);
                self.write_long(pairs.len() as i64);
                for (k, val) in pairs {
                    self.write(&k)?;
                    self.write(&val)?;
                }
                if defaulted {
                    self.write(&default)?;
                }
            }
            // Ruby marshals a Range as an ordinary object carrying three
            // ivars, through the generic `marshal_compat` hook `range.c`
            // registers -- there is no Range arm in `w_object` at all. Without
            // it a Range, and anything CONTAINING one, fails with a TypeError
            // about a C-extension hook the caller never wrote.
            RubyValue::Range(__rg) => {
                let (start, end, excl) = __rg.parts();
                if self.check_link(ptr_of(v)) {
                    return Ok(());
                }
                self.register_link(ptr_of(v));
                self.out.push(b'o');
                self.write_symbol("Range");
                self.write_long(3);
                // The write ORDER is ruby's: excl, begin, end.
                self.write_symbol("excl");
                self.write(&RubyValue::Bool(excl))?;
                self.write_symbol("begin");
                self.write(start.unwrap_or(&RubyValue::Nil))?;
                self.write_symbol("end");
                self.write(end.unwrap_or(&RubyValue::Nil))?;
            }
            RubyValue::Class(cid) => {
                if self.check_link(cid.0 as usize) {
                    return Ok(());
                }
                self.register_link(cid.0 as usize);
                let is_mod = class_is_module(*cid).unwrap_or(false);
                self.out.push(if is_mod { b'm' } else { b'c' });
                let name = nameable(*cid)?;
                self.write_bytes(name.as_bytes());
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
            RubyValue::Object(o) => self.write_object(v, o)?,
            other => {
                return Err(type_error!(
                    "no _dump_data is defined for class {}",
                    crate::builtins::class_name_of(other)
                ));
            }
        }
        Ok(())
    }

    /// Serialize a heap object, choosing CRuby's tag by protocol: `marshal_dump`
    /// -> `U`, else `_dump` -> `u`, else a Struct -> `S`, else a value-builtin
    /// subclass -> `C`, else a plain `o` with inline ivars.
    fn write_object(&mut self, v: &RubyValue, o: &RObj) -> Result<(), Signal> {
        let cid = o.class_id();
        let name = nameable(cid)?;

        // `Time` -> `Iu` with the zone/offset/nano ivars CRuby hangs on the
        // dump string -- native, because the `_dump` row's answer (a zeo
        // string) has nowhere to carry them.
        if let Some(t) = o.as_any().downcast_ref::<crate::builtins::time::RTime>() {
            if self.check_link(ptr_of(v)) {
                return Ok(());
            }
            self.register_link(ptr_of(v));
            let (bytes, named) = crate::builtins::time::time_mdump(t);
            let ivars: Vec<Iv> = named.into_iter().map(|(n, iv)| Iv::Named(n, iv)).collect();
            if !ivars.is_empty() {
                self.out.push(b'I');
            }
            self.out.push(b'u');
            self.write_symbol(&name);
            self.write_bytes(&bytes);
            self.write_ivars(&ivars)?;
            return Ok(());
        }

        // A Set IS its backing Hash, and CRuby dumps exactly that: `o:Set`
        // with one `@hash` ivar. Native, because `RSet` carries no ivar bag
        // for the generic object path to find.
        if let Some(hash) = crate::builtins::set::backing_hash(v) {
            if self.check_link(ptr_of(v)) {
                return Ok(());
            }
            self.register_link(ptr_of(v));
            self.out.push(b'o');
            self.write_symbol(&name);
            self.write_ivars(&[Iv::Named("@hash".to_string(), RubyValue::Hash(hash))])?;
            return Ok(());
        }

        if responds_to(cid, Symbol::intern("marshal_dump"), true) {
            if self.check_link(ptr_of(v)) {
                return Ok(());
            }
            self.register_link(ptr_of(v));
            self.out.push(b'U');
            self.write_symbol(&name);
            let dumped = send_value(v, Symbol::intern("marshal_dump"), &[], None)?;
            return self.write(&dumped);
        }

        if responds_to(cid, Symbol::intern("_dump"), true) {
            if self.check_link(ptr_of(v)) {
                return Ok(());
            }
            self.register_link(ptr_of(v));
            let result = send_value(v, Symbol::intern("_dump"), &[RubyValue::Int(-1)], None)?;
            let RubyValue::Str(rs) = &result else {
                return Err(type_error!("_dump() must return string"));
            };
            let ivars: Vec<Iv> = encoding_ivar(rs.lock().encoding()).into_iter().collect();
            if !ivars.is_empty() {
                self.out.push(b'I');
            }
            self.out.push(b'u');
            self.write_symbol(&name);
            let bytes = rs.lock().bytes().to_vec();
            self.write_bytes(&bytes);
            self.write_ivars(&ivars)?;
            return Ok(());
        }

        if let Some(members) = crate::builtins::rstruct::marshal_members(v) {
            if self.check_link(ptr_of(v)) {
                return Ok(());
            }
            self.register_link(ptr_of(v));
            self.out.push(b'S');
            self.write_symbol(&name);
            self.write_long(members.len() as i64);
            for (m, val) in members {
                self.write_symbol(&m.name());
                self.write(&val)?;
            }
            return Ok(());
        }

        // A value-builtin subclass (`class Tag < String`) -> `C`: the class
        // symbol then the inherited builtin's body inline, `I`-wrapped for the
        // string encoding and any user ivars.
        if let Some(payload) = o.builtin_payload() {
            if self.check_link(ptr_of(v)) {
                return Ok(());
            }
            let mut ivars: Vec<Iv> = Vec::new();
            if let RubyValue::Str(rs) = &payload {
                ivars.extend(encoding_ivar(rs.lock().encoding()));
            }
            for (iname, ival) in o.ivar_pairs() {
                ivars.push(Iv::Named(iname, ival));
            }
            if !ivars.is_empty() {
                self.out.push(b'I');
            }
            self.register_link(ptr_of(v));
            self.out.push(b'C');
            self.write_symbol(&name);
            self.write_builtin_body(&payload)?;
            self.write_ivars(&ivars)?;
            return Ok(());
        }

        if self.check_link(ptr_of(v)) {
            return Ok(());
        }
        self.register_link(ptr_of(v));
        self.out.push(b'o');
        self.write_symbol(&name);
        let ivars = o.ivar_pairs();
        self.write_long(ivars.len() as i64);
        for (iname, ival) in ivars {
            self.write_symbol(&iname);
            self.write(&ival)?;
        }
        Ok(())
    }

    /// Write a value-builtin subclass's inherited body INLINE (no `I`-wrapper,
    /// no separate link -- the `C` object owns the link): the string bytes,
    /// array elements, or hash pairs of the wrapped payload.
    fn write_builtin_body(&mut self, payload: &RubyValue) -> Result<(), Signal> {
        match payload {
            RubyValue::Str(rs) => {
                self.out.push(b'"');
                let bytes = rs.lock().bytes().to_vec();
                self.write_bytes(&bytes);
            }
            RubyValue::Array(a) => {
                self.out.push(b'[');
                let len = array_len(a);
                self.write_long(len);
                for i in 0..len {
                    let e = array_get(a, i);
                    self.write(&e)?;
                }
            }
            RubyValue::Hash(h) => {
                self.out.push(b'{');
                let pairs = hash_pairs(h);
                self.write_long(pairs.len() as i64);
                for (k, val) in pairs {
                    self.write(&k)?;
                    self.write(&val)?;
                }
            }
            // A Regexp subclass carries its source and flags exactly as a bare
            // one does -- the `C` wrapper above already named the class, so
            // the body is the plain `/` form with no `I`-wrapper of its own.
            RubyValue::Regexp(re) => {
                self.out.push(b'/');
                self.write_bytes(re.source.as_bytes());
                self.out.push(regexp_options(re));
            }
            other => {
                return Err(type_error!(
                    "can't dump {} subclass payload",
                    crate::builtins::class_name_of(other)
                ));
            }
        }
        Ok(())
    }

    /// If this key (Arc pointer or class id) was already written, emit an
    /// `@`-link and return true.
    fn check_link(&mut self, key: usize) -> bool {
        if let Some(&idx) = self.links.get(&key) {
            self.out.push(b'@');
            self.write_long(idx as i64);
            return true;
        }
        false
    }

    /// Assign this key the next link index (call once, on the write path, after
    /// `check_link` misses).
    fn register_link(&mut self, key: usize) {
        let idx = self.next_link;
        self.next_link += 1;
        self.links.insert(key, idx);
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

    /// The ivar list of an `I`-wrapper: a length then symbol/value pairs. `E`
    /// takes a bare bool; `encoding` takes a linkable name string; a user ivar
    /// takes a recursively-marshaled value.
    fn write_ivars(&mut self, ivars: &[Iv]) -> Result<(), Signal> {
        if ivars.is_empty() {
            return Ok(());
        }
        self.write_long(ivars.len() as i64);
        for iv in ivars {
            match iv {
                Iv::EncBool(b) => {
                    self.write_symbol("E");
                    self.out.push(if *b { b'T' } else { b'F' });
                }
                Iv::EncName(n) => {
                    self.write_symbol("encoding");
                    self.write_encoding_name(n);
                }
                Iv::Named(name, val) => {
                    self.write_symbol(name);
                    self.write(val)?;
                }
            }
        }
        Ok(())
    }

    /// The `encoding => "<name>"` value: a bare (`"`) string that joins the
    /// object-link table, deduped by name (CRuby shares the one frozen name).
    fn write_encoding_name(&mut self, name: &str) {
        if let Some(&idx) = self.enc_links.get(name) {
            self.out.push(b'@');
            self.write_long(idx as i64);
            return;
        }
        let idx = self.next_link;
        self.next_link += 1;
        self.enc_links.insert(name.to_string(), idx);
        self.out.push(b'"');
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
    fn write_long(&mut self, x: i64) {
        marshal_long_into(x, &mut self.out);
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

/// CRuby's regexp option byte: `IGNORECASE=1 | EXTEND=2 | MULTILINE=4`.
fn regexp_options(re: &crate::regexp::RegexpData) -> u8 {
    let mut o = 0u8;
    if re.ignore_case {
        o |= 1;
    }
    if re.extended {
        o |= 2;
    }
    if re.multiline {
        o |= 4;
    }
    o
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
        let b = *self
            .bytes
            .get(self.pos)
            .ok_or_else(|| arg_error!("marshal data too short"))?;
        self.pos += 1;
        Ok(b)
    }

    fn read(&mut self) -> Result<RubyValue, Signal> {
        let tag = self.byte()?;
        self.read_tag(tag)
    }

    fn read_tag(&mut self, tag: u8) -> Result<RubyValue, Signal> {
        match tag {
            b'0' => Ok(RubyValue::Nil),
            b'T' => Ok(RubyValue::Bool(true)),
            b'F' => Ok(RubyValue::Bool(false)),
            b'i' => Ok(int_to_value(self.read_long()?)),
            b':' => self.read_symbol_new(),
            b';' => {
                let idx = self.read_long()? as usize;
                let name = self
                    .symbols
                    .get(idx)
                    .cloned()
                    .ok_or_else(|| arg_error!("bad symbol link"))?;
                Ok(RubyValue::Symbol(crate::Symbol::intern(&name)))
            }
            b'@' => {
                let idx = self.read_long()? as usize;
                self.objects
                    .get(idx)
                    .cloned()
                    .ok_or_else(|| arg_error!("bad object link"))
            }
            b'f' => {
                let bytes = self.read_bytes()?;
                let s = String::from_utf8_lossy(&bytes);
                let f = parse_marshal_float(&s);
                let v = RubyValue::Float(f);
                self.objects.push(v.clone());
                Ok(v)
            }
            // A bare string carries no encoding wrapper: CRuby only writes it
            // for ASCII-8BIT (BINARY) data.
            b'"' => self.read_string(ASCII_8BIT),
            b'/' => self.read_regexp(),
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
            // `}` is `{` plus a trailing default value.
            tag @ (b'{' | b'}') => {
                let h = hash_new(Vec::new());
                let v = RubyValue::Hash(h.clone());
                self.objects.push(v.clone());
                let n = self.read_long()?;
                for _ in 0..n {
                    let k = self.read()?;
                    let val = self.read()?;
                    hash_set(&h, k, val);
                }
                if tag == b'}' {
                    let default = self.read()?;
                    h.lock().default = default;
                }
                Ok(v)
            }
            b'l' => self.read_bignum(),
            b'c' => self.read_class_ref(),
            b'm' => self.read_class_ref(),
            b'C' => self.read_subclass(),
            b'S' => self.read_struct(),
            b'U' => self.read_user_marshal(),
            b'u' => self.read_userdef(false),
            b'o' => self.read_object(),
            b'I' => self.read_ivar(),
            other => Err(arg_error!("dump format error (0x{other:x})")),
        }
    }

    /// An `I` ivar-wrapper: the wrapped object, then its ivars. For `u`
    /// (user-def) the ivars retag the `_dump` data string BEFORE `_load`, so it
    /// is threaded in; every other type takes the ivars after its body.
    fn read_ivar(&mut self) -> Result<RubyValue, Signal> {
        let tag = self.byte()?;
        if tag == b'u' {
            return self.read_userdef(true);
        }
        let v = self.read_tag(tag)?;
        self.apply_ivars(&v)?;
        Ok(v)
    }

    /// Read an ivar list and apply it to `v`: `E`/`encoding` retag a string's
    /// encoding; any other name is a real instance variable.
    fn apply_ivars(&mut self, v: &RubyValue) -> Result<(), Signal> {
        let n = self.read_long()?;
        for _ in 0..n {
            let name = self.read_symbol_name()?;
            let val = self.read()?;
            match name.as_str() {
                "E" => set_string_encoding(v, if truthy(&val) { UTF_8 } else { US_ASCII }),
                "encoding" => {
                    if let RubyValue::Str(s) = &val {
                        let nm = s.lock().to_utf8_lossy().into_owned();
                        if let Some(e) = crate::encoding::find(&nm) {
                            set_string_encoding(v, e);
                        }
                    }
                }
                other => {
                    if let RubyValue::Object(o) = v {
                        o.ivar_set_named(other.trim_start_matches('@'), val);
                    }
                }
            }
        }
        Ok(())
    }

    fn read_string(&mut self, enc: EncodingId) -> Result<RubyValue, Signal> {
        let bytes = self.read_bytes()?;
        let v = RubyValue::Str(string_from_bytes(bytes, enc));
        self.objects.push(v.clone());
        Ok(v)
    }

    fn read_regexp(&mut self) -> Result<RubyValue, Signal> {
        let idx = self.objects.len();
        self.objects.push(RubyValue::Nil);
        let src = self.read_bytes()?;
        let opts = self.byte()?;
        let source = String::from_utf8_lossy(&src).into_owned();
        let re = crate::regexp::regexp_new(&source, opts & 1 != 0, opts & 2 != 0, opts & 4 != 0)
            .map_err(|e| arg_error!("{e}"))?;
        let v = RubyValue::Regexp(re);
        self.objects[idx] = v.clone();
        Ok(v)
    }

    fn read_class_ref(&mut self) -> Result<RubyValue, Signal> {
        let bytes = self.read_bytes()?;
        let name = String::from_utf8_lossy(&bytes).into_owned();
        let cid =
            class_id_by_name(&name).ok_or_else(|| arg_error!("undefined class/module {name}"))?;
        let v = RubyValue::Class(cid);
        self.objects.push(v.clone());
        Ok(v)
    }

    fn read_struct(&mut self) -> Result<RubyValue, Signal> {
        let idx = self.objects.len();
        self.objects.push(RubyValue::Nil);
        let cls = self.read_symbol_name()?;
        let cid =
            class_id_by_name(&cls).ok_or_else(|| arg_error!("undefined class/module {cls}"))?;
        let meta = crate::builtins::rstruct::meta_of(cid)
            .ok_or_else(|| type_error!("{cls} is not a Struct"))?;
        let count = self.read_long()? as usize;
        let mut values = vec![RubyValue::Nil; meta.members.len()];
        for _ in 0..count {
            let mname = self.read_symbol_name()?;
            let val = self.read()?;
            let msym = Symbol::intern(&mname);
            if let Some(pos) = meta.members.iter().position(|m| *m == msym) {
                values[pos] = val;
            }
        }
        let v = crate::builtins::rstruct::struct_construct(cid, &values, None)?;
        self.objects[idx] = v.clone();
        Ok(v)
    }

    /// `C`: a value-builtin subclass. The class symbol then its inherited
    /// builtin body INLINE (a `"`/`[`/`{` tag), read into a fresh payload of the
    /// subclass's root kind and seated -- the payload shares the `C` object's
    /// link, so the body is NOT read as a separate object.
    fn read_subclass(&mut self) -> Result<RubyValue, Signal> {
        let idx = self.objects.len();
        self.objects.push(RubyValue::Nil);
        let cls = self.read_symbol_name()?;
        let cid =
            class_id_by_name(&cls).ok_or_else(|| arg_error!("undefined class/module {cls}"))?;
        let body_tag = self.byte()?;
        let payload = match body_tag {
            b'"' => RubyValue::Str(string_from_bytes(self.read_bytes()?, ASCII_8BIT)),
            b'[' => RubyValue::Array(array_new(Vec::new())),
            b'{' => RubyValue::Hash(hash_new(Vec::new())),
            // The Regexp body is complete right here (source + flags), unlike
            // the three collection bodies, which are filled in below.
            b'/' => {
                let src = self.read_bytes()?;
                let opts = self.byte()?;
                let source = String::from_utf8_lossy(&src).into_owned();
                let re =
                    crate::regexp::regexp_new(&source, opts & 1 != 0, opts & 2 != 0, opts & 4 != 0)
                        .map_err(|e| arg_error!("{e}"))?;
                RubyValue::Regexp(re)
            }
            other => return Err(arg_error!("bad subclass body (0x{other:x})")),
        };
        let obj = crate::builtins::value_subclass::marshal_alloc(cid, payload.clone())
            .ok_or_else(|| type_error!("{cls} is not a subclass of a marshalable builtin"))?;
        let v = RubyValue::Object(obj);
        self.objects[idx] = v.clone();
        // Fill the collection payloads now that the subclass is linkable (so a
        // self-referential element resolves to `@idx`).
        match (body_tag, &payload) {
            (b'[', RubyValue::Array(arr)) => {
                let len = self.read_long()?;
                for _ in 0..len {
                    let e = self.read()?;
                    array_push(arr, e);
                }
            }
            (b'{', RubyValue::Hash(h)) => {
                let n = self.read_long()?;
                for _ in 0..n {
                    let k = self.read()?;
                    let val = self.read()?;
                    hash_set(h, k, val);
                }
            }
            _ => {}
        }
        Ok(v)
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
                self.symbols
                    .get(idx)
                    .cloned()
                    .ok_or_else(|| arg_error!("bad symbol link"))
            }
            other => Err(arg_error!("expected a symbol (0x{other:x})")),
        }
    }

    /// `U`: `marshal_dump`/`marshal_load`. Rational/Complex keep native fast
    /// paths; every other class allocates then dispatches `marshal_load`.
    fn read_user_marshal(&mut self) -> Result<RubyValue, Signal> {
        let idx = self.objects.len();
        self.objects.push(RubyValue::Nil);
        let cls = self.read_symbol_name()?;
        let v = match cls.as_str() {
            "Rational" | "Complex" => {
                let inner = self.read()?;
                let RubyValue::Array(a) = &inner else {
                    return Err(arg_error!("malformed user marshal"));
                };
                let get = |i| array_get(a, i);
                if cls == "Rational" {
                    crate::builtins::rational::rational_new(
                        to_bigint(&get(0))?,
                        to_bigint(&get(1))?,
                    )?
                } else {
                    crate::builtins::complex::complex_new(get(0), get(1))?
                }
            }
            _ => {
                let cid = class_id_by_name(&cls)
                    .ok_or_else(|| arg_error!("undefined class/module {cls}"))?;
                let obj = marshal_allocate(cid)
                    .ok_or_else(|| type_error!("allocator undefined for {cls}"))?;
                self.objects[idx] = obj.clone();
                let inner = self.read()?;
                send_value(&obj, Symbol::intern("marshal_load"), &[inner], None)?;
                obj
            }
        };
        self.objects[idx] = v.clone();
        Ok(v)
    }

    /// `u`: `_dump`/`_load`. With `has_ivars`, the trailing encoding ivars
    /// retag the data string before it reaches `Class._load`.
    fn read_userdef(&mut self, has_ivars: bool) -> Result<RubyValue, Signal> {
        let idx = self.objects.len();
        self.objects.push(RubyValue::Nil);
        let cls = self.read_symbol_name()?;
        let bytes = self.read_bytes()?;
        let mut enc = ASCII_8BIT;
        let mut named: Vec<(String, RubyValue)> = Vec::new();
        if has_ivars {
            let n = self.read_long()?;
            for _ in 0..n {
                let name = self.read_symbol_name()?;
                let val = self.read()?;
                if let Some(e) = enc_from_ivar(&name, &val) {
                    enc = e;
                } else {
                    named.push((name, val));
                }
            }
        }
        // `Time` rebuilds natively: its wire ivars (zone/offset/nano) belong
        // to the VALUE, not to the byte string `_load` would receive.
        if cls == "Time" {
            let v = crate::builtins::time::time_mload(&bytes, &named)?;
            self.objects[idx] = v.clone();
            return Ok(v);
        }
        let data = RubyValue::Str(string_from_bytes(bytes, enc));
        let cid =
            class_id_by_name(&cls).ok_or_else(|| arg_error!("undefined class/module {cls}"))?;
        let v = send_value(
            &RubyValue::Class(cid),
            Symbol::intern("_load"),
            &[data],
            None,
        )?;
        self.objects[idx] = v.clone();
        Ok(v)
    }

    fn read_object(&mut self) -> Result<RubyValue, Signal> {
        let idx = self.objects.len();
        self.objects.push(RubyValue::Nil);
        let cls = self.read_symbol_name()?;
        // Range is not an ordinary object on the way back either: ruby's
        // `range_loader` reads the three ivars off the placeholder and
        // rebuilds a real Range, which is what the loaded value has to be.
        if cls == "Range" {
            return self.read_range(idx);
        }
        if cls == "Set" {
            return self.read_set(idx);
        }
        let cid =
            class_id_by_name(&cls).ok_or_else(|| arg_error!("undefined class/module {cls}"))?;
        let obj =
            marshal_allocate(cid).ok_or_else(|| type_error!("allocator undefined for {cls}"))?;
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

    /// The `o:Set` body: one `@hash` ivar whose KEYS are the members -- the
    /// shape CRuby's Set dumps, so a stream crosses between the two.
    fn read_set(&mut self, idx: usize) -> Result<RubyValue, Signal> {
        let mut members: Vec<RubyValue> = Vec::new();
        let count = self.read_long()?;
        for _ in 0..count {
            let iname = self.read_symbol_name()?;
            let value = self.read()?;
            if iname.trim_start_matches('@') == "hash"
                && let RubyValue::Hash(h) = value
            {
                members = crate::collections::hash_pairs_snapshot(&h)
                    .into_iter()
                    .map(|(k, _)| k)
                    .collect();
            }
        }
        let v = crate::builtins::set::set_from(members);
        self.objects[idx] = v.clone();
        Ok(v)
    }

    /// The `o:Range` body: three ivars in any order, reassembled into a real
    /// `RubyValue::Range`. `range_endpoint` normalizes a nil bound to the
    /// beginless/endless form, so `nil..5` loads back as `..5`.
    fn read_range(&mut self, idx: usize) -> Result<RubyValue, Signal> {
        let (mut start, mut end, mut excl) = (RubyValue::Nil, RubyValue::Nil, false);
        let count = self.read_long()?;
        for _ in 0..count {
            let iname = self.read_symbol_name()?;
            let value = self.read()?;
            match iname.trim_start_matches('@') {
                "begin" => start = value,
                "end" => end = value,
                "excl" => excl = value.truthy(),
                _ => {}
            }
        }
        let v = crate::builtins::range::range_value(
            crate::value::range_endpoint(start),
            crate::value::range_endpoint(end),
            excl,
        );
        self.objects[idx] = v.clone();
        Ok(v)
    }

    fn read_bignum(&mut self) -> Result<RubyValue, Signal> {
        let sign = if self.byte()? == b'-' {
            Sign::Minus
        } else {
            Sign::Plus
        };
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
            return Err(arg_error!("marshal data too short"));
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

fn truthy(v: &RubyValue) -> bool {
    !matches!(v, RubyValue::Nil | RubyValue::Bool(false))
}

/// Retag a string's encoding in place -- for a bare string or a string-rooted
/// value-subclass payload (`C`). A no-op for any other value (a regexp's
/// encoding isn't stored, and other types carry no `E` ivar).
fn set_string_encoding(v: &RubyValue, enc: EncodingId) {
    match v {
        RubyValue::Str(s) => s.lock().set_encoding(enc),
        RubyValue::Object(o) => {
            if let Some(RubyValue::Str(s)) = o.builtin_payload() {
                s.lock().set_encoding(enc);
            }
        }
        _ => {}
    }
}

/// The encoding named by an `E`/`encoding` ivar pair, if this is one.
fn enc_from_ivar(name: &str, val: &RubyValue) -> Option<EncodingId> {
    match name {
        "E" => Some(if truthy(val) { UTF_8 } else { US_ASCII }),
        "encoding" => {
            let RubyValue::Str(s) = val else { return None };
            let nm = s.lock().to_utf8_lossy().into_owned();
            crate::encoding::find(&nm)
        }
        _ => None,
    }
}

/// The identity of a linkable ref value, as its Arc data-pointer address.
/// A class's name for the stream, or a TypeError. A dump names the class so
/// `Marshal.load` can find it again -- which an ANONYMOUS class (an unassigned
/// `Class.new`/`Struct.new`, a singleton class) has no way to be, so CRuby
/// refuses to write one rather than emit a reference nothing can resolve.
fn nameable(cid: crate::ClassId) -> Result<String, Signal> {
    if let Some(name) = crate::dispatch::class_real_name(cid) {
        return Ok(name);
    }
    let rendered = class_name(cid).unwrap_or_default();
    // A SINGLETON class gets its own wording, and no rendering at all -- CRuby
    // has nothing to name it by either.
    if rendered.starts_with("#<Class:") && !rendered.starts_with("#<Class:0x") {
        return Err(type_error!("singleton class can't be dumped"));
    }
    let kind = if class_is_module(cid).unwrap_or(false) {
        "module"
    } else {
        "class"
    };
    Err(type_error!("can't dump anonymous {kind} {rendered}"))
}

fn ptr_of(v: &RubyValue) -> usize {
    match v {
        RubyValue::Str(s) => Arc::as_ptr(s) as *const () as usize,
        RubyValue::Array(a) => Arc::as_ptr(a) as *const () as usize,
        RubyValue::Hash(h) => Arc::as_ptr(h) as *const () as usize,
        RubyValue::Regexp(re) => Arc::as_ptr(re) as *const () as usize,
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
        other => Err(type_error!(
            "can't convert {} into Integer",
            crate::builtins::convert_name_of(other)
        )),
    }
}

/// CRuby's Marshal float text: the shortest round-tripping decimal, using
/// exponent form only when the fixed form would need trailing zeros
/// (`decpt > ndigits`) or more than three leading zeros (`decpt <= -4`).
/// Rust's `{:e}` supplies the shortest mantissa/exponent.
fn float_to_marshal(d: f64) -> String {
    if d == 0.0 {
        return if d.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    // Each test runs in its own nextest process -- see the crate README for
    // the with_core() bootstrap pattern and why `cargo test` is unsupported.
    fn install_core() {
        crate::dispatch::install_class_registry(crate::dispatch::ClassRegistry::with_core());
    }

    fn dump(v: &RubyValue) -> Vec<u8> {
        let out = send_value(
            &RubyValue::Class(zeo_abi::MARSHAL_MODULE),
            Symbol::intern("dump"),
            std::slice::from_ref(v),
            None,
        )
        .expect("Marshal.dump succeeds");
        match out {
            RubyValue::Str(s) => s.lock().bytes().to_vec(),
            _ => panic!("Marshal.dump answers a String"),
        }
    }

    fn load_bytes(bytes: Vec<u8>) -> Result<RubyValue, Signal> {
        let s = RubyValue::Str(string_from_bytes(bytes, ASCII_8BIT));
        send_value(
            &RubyValue::Class(zeo_abi::MARSHAL_MODULE),
            Symbol::intern("load"),
            &[s],
            None,
        )
    }

    fn round_trip(v: &RubyValue) -> RubyValue {
        load_bytes(dump(v)).expect("Marshal.load succeeds")
    }

    fn raised_class(r: Result<RubyValue, Signal>) -> crate::ClassId {
        let Err(Signal::Raise(exc)) = r else {
            panic!("expected a raised exception");
        };
        exc.as_object_unchecked().class_id()
    }

    #[test]
    fn a_dump_starts_with_the_v48_header() {
        install_core();
        let bytes = dump(&RubyValue::Nil);
        assert_eq!(&bytes, &[MAJOR, MINOR, b'0']);
    }

    #[test]
    fn core_scalars_round_trip() {
        install_core();
        let cases = [
            RubyValue::Nil,
            RubyValue::Bool(true),
            RubyValue::Bool(false),
            RubyValue::Int(0),
            RubyValue::Int(42),
            RubyValue::Int(-1),
            RubyValue::Int(123_456_789),
            RubyValue::Int(i64::MIN + 1),
            RubyValue::Float(1.5),
            RubyValue::Str(crate::string_new("h\u{e9}llo".to_string())),
        ];
        for v in &cases {
            assert_eq!(round_trip(v).inspect_string(), v.inspect_string());
        }
    }

    #[test]
    fn a_symbol_round_trips_as_a_symbol() {
        install_core();
        let sym = Symbol::intern("zeo");
        let loaded = round_trip(&RubyValue::Symbol(sym));
        assert!(matches!(loaded, RubyValue::Symbol(s) if s == sym));
    }

    #[test]
    fn arrays_and_hashes_round_trip() {
        install_core();
        let arr = RubyValue::Array(array_new(vec![
            RubyValue::Int(1),
            RubyValue::Str(crate::string_new("two".to_string())),
            RubyValue::Nil,
        ]));
        assert_eq!(round_trip(&arr).inspect_string(), arr.inspect_string());

        let hash = RubyValue::Hash(hash_new(vec![
            (RubyValue::Symbol(Symbol::intern("a")), RubyValue::Int(1)),
            (
                RubyValue::Str(crate::string_new("b".to_string())),
                RubyValue::Int(2),
            ),
        ]));
        assert_eq!(round_trip(&hash).inspect_string(), hash.inspect_string());
    }

    #[test]
    fn a_shared_reference_loads_as_one_object() {
        install_core();
        // The object-link table (`@`): the second occurrence dumps as a link,
        // so the load answers ONE string reachable twice.
        let s = RubyValue::Str(crate::string_new("shared".to_string()));
        let arr = RubyValue::Array(array_new(vec![s.clone(), s]));
        let RubyValue::Array(loaded) = round_trip(&arr) else {
            panic!("expected an Array back");
        };
        let elems = loaded.lock().to_vec();
        let (RubyValue::Str(a), RubyValue::Str(b)) = (&elems[0], &elems[1]) else {
            panic!("expected two Strings back");
        };
        assert!(Arc::ptr_eq(a, b));
    }

    #[test]
    fn a_wrong_version_is_refused() {
        install_core();
        let err = load_bytes(vec![MAJOR - 1, MINOR, b'0']);
        assert_eq!(raised_class(err), zeo_abi::TYPE_ERROR_CLASS);
    }

    #[test]
    fn a_truncated_stream_is_refused() {
        install_core();
        // A string tag with no length byte behind it.
        let err = load_bytes(vec![MAJOR, MINOR, b'"']);
        assert_eq!(raised_class(err), zeo_abi::ARGUMENT_ERROR_CLASS);
    }
}

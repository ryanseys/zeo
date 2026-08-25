//! The rest of the C0 surface: TypedData constructors, encodings, coercions,
//! and the `st_*` string helpers.
//!
//! # `rb_encoding *` is an opaque token
//!
//! MRI's `rb_encoding` is an `OnigEncodingType`, a struct with function
//! pointers an extension could in principle read. In practice it does not:
//! it calls `rb_enc_get(str)`, hands the result to `rb_enc_str_new`, and
//! compares it against `rb_utf8_encoding()`. All three work on a token.
//!
//! So a `rb_encoding *` is `EncodingId + 1` cast to a pointer -- `+ 1` so
//! that `ASCII-8BIT`, which is id 0, is not NULL. An extension that
//! DEREFERENCES one gets a fault at the read rather than a wrong byte, which
//! is the loud direction. Recorded in `cext/README.md`.

use super::convert::{to_value, value_of};
use super::data::{CData, DataFunc, DataType};
use super::value::Value;
use crate::encoding::EncodingId;
use crate::{RubyValue, Signal};
use std::ffi::{c_char, c_int, c_long, c_void};

/// `rb_encoding *` as zeo spells it. See the module docs.
pub type Encoding = *const c_void;

fn enc_token(id: EncodingId) -> Encoding {
    (id.0 as usize + 1) as Encoding
}

pub(super) fn encoding_of(e: Encoding) -> EncodingId {
    enc_of(e)
}

fn enc_of(e: Encoding) -> EncodingId {
    match (e as usize).checked_sub(1) {
        Some(n) => EncodingId(n as u8),
        // NULL is not an encoding; binary is the answer that loses nothing.
        None => crate::encoding::ASCII_8BIT,
    }
}

fn out_of_range(what: &str) -> Signal {
    crate::builtins::range_error!("{what} out of range")
}

/// # Safety
///
/// `v` is an extension's own `VALUE`.
unsafe fn as_class(v: Value) -> Result<crate::dispatch::ClassId, Signal> {
    match unsafe { value_of(v) } {
        RubyValue::Class(cid) => Ok(cid),
        other => Err(crate::builtins::wrong_arg_type(&other, "Class")),
    }
}

crate::cext_fn! {
    /// `TypedData_Wrap_Struct`'s runtime half.
    fn rb_data_typed_object_wrap(klass: Value, data: *mut c_void, dtype: *const DataType) -> Value {
        let cid = unsafe { as_class(klass)? };
        to_value(&RubyValue::Object(CData::typed(cid, data, dtype)))
    }

    /// `TypedData_Make_Struct`'s runtime half: allocate `size` zeroed bytes
    /// and wrap them. The allocation is `ruby_xcalloc`'s, so `dfree` may be
    /// `RUBY_TYPED_DEFAULT_FREE`.
    fn rb_data_typed_object_zalloc(klass: Value, size: usize, dtype: *const DataType) -> Value {
        let cid = unsafe { as_class(klass)? };
        let data = unsafe { super::alloc::xcalloc(1, size) };
        to_value(&RubyValue::Object(CData::typed(cid, data, dtype)))
    }

    fn rb_data_object_wrap(
        klass: Value,
        data: *mut c_void,
        mark: DataFunc,
        free: DataFunc,
    ) -> Value {
        let cid = unsafe { as_class(klass)? };
        to_value(&RubyValue::Object(CData::untyped(cid, data, mark, free)))
    }

    fn rb_data_object_zalloc(
        klass: Value,
        size: usize,
        mark: DataFunc,
        free: DataFunc,
    ) -> Value {
        let cid = unsafe { as_class(klass)? };
        let data = unsafe { super::alloc::xcalloc(1, size) };
        to_value(&RubyValue::Object(CData::untyped(cid, data, mark, free)))
    }

    /// `rb_check_typeddata`. The type check walks the `parent` chain, which
    /// is how a subclass's descriptor passes its parent's check.
    fn rb_check_typeddata(obj: Value, expected: *const DataType) -> *mut c_void {
        let v = unsafe { value_of(obj) };
        let RubyValue::Object(o) = &v else {
            return Err(crate::builtins::wrong_arg_type(&v, "TypedData"));
        };
        let Some(d) = o.as_any().downcast_ref::<CData>() else {
            return Err(crate::builtins::wrong_arg_type(&v, "TypedData"));
        };
        let mut actual = d.data_type();
        while !actual.is_null() {
            if std::ptr::eq(actual, expected) {
                // SAFETY: the slot belongs to a `CData` this handle holds.
                return Ok(unsafe { d.slot().read() });
            }
            // SAFETY: a descriptor is a static in the extension's image.
            actual = unsafe { (*actual).parent };
        }
        Err(crate::builtins::wrong_arg_type(&v, "the expected TypedData"))
    }

    fn rb_utf8_encoding() -> Encoding {
        Ok(enc_token(crate::encoding::UTF_8))
    }

    fn rb_usascii_encoding() -> Encoding {
        Ok(enc_token(crate::encoding::US_ASCII))
    }

    fn rb_ascii8bit_encoding() -> Encoding {
        Ok(enc_token(crate::encoding::ASCII_8BIT))
    }

    fn rb_utf8_encindex() -> c_int {
        Ok(crate::encoding::UTF_8.0 as c_int)
    }

    fn rb_usascii_encindex() -> c_int {
        Ok(crate::encoding::US_ASCII.0 as c_int)
    }

    fn rb_ascii8bit_encindex() -> c_int {
        Ok(crate::encoding::ASCII_8BIT.0 as c_int)
    }

    fn rb_enc_from_index(idx: c_int) -> Encoding {
        Ok(enc_token(EncodingId(idx.max(0) as u8)))
    }

    fn rb_enc_to_index(e: Encoding) -> c_int {
        Ok(enc_of(e).0 as c_int)
    }

    /// `rb_enc_get(obj)`. A String answers its own encoding; anything else
    /// answers binary, as MRI does for an object with no encoding.
    fn rb_enc_get(obj: Value) -> Encoding {
        Ok(match unsafe { value_of(obj) } {
            RubyValue::Str(s) => enc_token(s.lock().encoding()),
            _ => enc_token(crate::encoding::ASCII_8BIT),
        })
    }

    fn rb_enc_get_index(obj: Value) -> c_int {
        Ok(match unsafe { value_of(obj) } {
            RubyValue::Str(s) => s.lock().encoding().0 as c_int,
            _ => crate::encoding::ASCII_8BIT.0 as c_int,
        })
    }

    fn rb_enc_str_new(p: *const c_char, len: c_long, e: Encoding) -> Value {
        let bytes = unsafe { super::string::borrow_bytes(p, len) };
        to_value(&RubyValue::Str(crate::string_from_bytes(bytes, enc_of(e))))
    }

    fn rb_enc_str_new_cstr(p: *const c_char, e: Encoding) -> Value {
        let bytes = unsafe { super::string::borrow_bytes(p, -1) };
        to_value(&RubyValue::Str(crate::string_from_bytes(bytes, enc_of(e))))
    }

    /// `rb_enc_interned_str`. zeo does not intern strings, so this builds an
    /// ordinary frozen one -- which is what an interned string IS from the
    /// caller's side, minus the sharing.
    fn rb_enc_interned_str(p: *const c_char, len: c_long, e: Encoding) -> Value {
        let bytes = unsafe { super::string::borrow_bytes(p, len) };
        let s = RubyValue::Str(crate::string_from_bytes(bytes, enc_of(e)));
        s.freeze_value()?;
        to_value(&s)
    }

    fn rb_enc_associate_index(obj: Value, idx: c_int) -> Value {
        if let RubyValue::Str(s) = unsafe { value_of(obj) } {
            let mut g = s.lock();
            let bytes = g.bytes().to_vec();
            g.replace_bytes(bytes, EncodingId(idx.max(0) as u8));
        }
        Ok(obj)
    }

    fn rb_enc_associate(obj: Value, e: Encoding) -> Value {
        unsafe { Ok(rb_enc_associate_index(obj, enc_of(e).0 as c_int)) }
    }

    /// `rb_String(v)` -- `Kernel#String`, which is `to_str` then `to_s`.
    fn rb_String(v: Value) -> Value {
        let v = unsafe { value_of(v) };
        to_value(&coerce(&v, "to_str").or_else(|_| coerce(&v, "to_s"))?)
    }

    fn rb_Array(v: Value) -> Value {
        let v = unsafe { value_of(v) };
        to_value(&coerce(&v, "to_a").unwrap_or(RubyValue::Array(
            crate::value::collections::array_new(vec![v.clone()]),
        )))
    }

    fn rb_Integer(v: Value) -> Value {
        let v = unsafe { value_of(v) };
        to_value(&coerce(&v, "to_i")?)
    }

    fn rb_Float(v: Value) -> Value {
        let v = unsafe { value_of(v) };
        to_value(&coerce(&v, "to_f")?)
    }

    /// `rb_check_string_type(v)`: `to_str` if it answers one, else nil. The
    /// CHECK forms never raise -- that is the whole difference from
    /// `rb_String`.
    fn rb_check_string_type(v: Value) -> Value {
        let v = unsafe { value_of(v) };
        to_value(&check(&v, "to_str"))
    }

    fn rb_check_array_type(v: Value) -> Value {
        let v = unsafe { value_of(v) };
        to_value(&check(&v, "to_ary"))
    }

    fn rb_check_hash_type(v: Value) -> Value {
        let v = unsafe { value_of(v) };
        to_value(&check(&v, "to_hash"))
    }

    /// `rb_obj_classname` -- the class's name as a `const char *`, which the
    /// symbol table's own C-string cache supplies.
    fn rb_obj_classname(v: Value) -> *const c_char {
        let v = unsafe { value_of(v) };
        let name = crate::dispatch::class_name(v.class_id()).unwrap_or("Object".into());
        Ok(super::symbol::cstr_for_owned(&name))
    }

    fn rb_class2name(klass: Value) -> *const c_char {
        let cid = unsafe { as_class(klass)? };
        let name = crate::dispatch::class_name(cid).unwrap_or("Class".into());
        Ok(super::symbol::cstr_for_owned(&name))
    }

    /// `rb_class_inherited_p(mod, arg)` -- `Module#<=`.
    fn rb_class_inherited_p(a: Value, b: Value) -> Value {
        let a = unsafe { value_of(a) };
        let b = unsafe { value_of(b) };
        to_value(&crate::dispatch::send_value(&a, crate::Symbol::intern("<="), &[b], None)?)
    }

    fn rb_include_module(target: Value, module: Value) -> Value {
        let t = unsafe { value_of(target) };
        let m = unsafe { value_of(module) };
        crate::runtime_meta::runtime_include(&t, &[m])?;
        Ok(target)
    }

    fn rb_extend_object(target: Value, module: Value) -> Value {
        let t = unsafe { value_of(target) };
        let m = unsafe { value_of(module) };
        crate::runtime_meta::runtime_extend(&t, &m)?;
        Ok(target)
    }

    /// `rb_str_substr(str, beg, len)` -- `String#[]` with two Integers.
    fn rb_str_substr(v: Value, beg: c_long, len: c_long) -> Value {
        let s = unsafe { value_of(v) };
        to_value(&crate::dispatch::send_value(
            &s,
            crate::Symbol::intern("[]"),
            &[RubyValue::Int(beg as i64), RubyValue::Int(len as i64)],
            None,
        )?)
    }

    /// `rb_str_resize(str, len)`. Growing pads with NUL bytes, as MRI does;
    /// shrinking truncates. Answers the receiver.
    fn rb_str_resize(v: Value, len: c_long) -> Value {
        let sv = unsafe { value_of(v) };
        let RubyValue::Str(s) = &sv else {
            return Err(crate::builtins::wrong_arg_type(&sv, "String"));
        };
        let want = len.max(0) as usize;
        let mut g = s.lock();
        let mut bytes = g.bytes().to_vec();
        bytes.resize(want, 0);
        let enc = g.encoding();
        g.replace_bytes(bytes, enc);
        Ok(v)
    }

    /// `rb_hash_foreach(hash, f, arg)`. `f` answers `ST_CONTINUE` (0) to keep
    /// going and `ST_STOP` (1) to stop; `ST_DELETE` (2) is not honoured,
    /// because deleting under an iteration is a shape zeo's hash does not
    /// support and answering "deleted" without deleting would be worse.
    fn rb_hash_foreach(
        h: Value,
        f: unsafe extern "C" fn(Value, Value, Value) -> c_int,
        arg: Value,
    ) -> Value {
        let hv = unsafe { value_of(h) };
        let RubyValue::Hash(hh) = &hv else {
            return Err(crate::builtins::wrong_arg_type(&hv, "Hash"));
        };
        // A snapshot, so the callback may touch the hash without the walk
        // reading a moved row.
        let pairs: Vec<(RubyValue, RubyValue)> =
            hh.lock().iter().map(|(_, (k, v))| (k.clone(), v.clone())).collect();
        for (k, v) in pairs {
            let (k, v) = (to_value(&k)?, to_value(&v)?);
            let go = super::jmp::protect(|| unsafe { f(k, v, arg) })?;
            if go != 0 {
                break;
            }
        }
        Ok(h)
    }

    /// `rb_big2ll` / `rb_big2ull`: the whole Integer, or a `RangeError`.
    fn rb_big2ll(v: Value) -> i64 {
        match unsafe { value_of(v) } {
            RubyValue::Int(n) => Ok(n),
            RubyValue::BigInt(b) => i64::try_from(&*b)
                .map_err(|_| out_of_range("bignum too big to convert into `long long'")),
            other => Err(crate::builtins::wrong_arg_type(&other, "Integer")),
        }
    }

    fn rb_big2ull(v: Value) -> u64 {
        match unsafe { value_of(v) } {
            RubyValue::Int(n) => u64::try_from(n)
                .map_err(|_| out_of_range("negative value into unsigned")),
            RubyValue::BigInt(b) => u64::try_from(&*b)
                .map_err(|_| out_of_range("bignum too big to convert into `unsigned long long'")),
            other => Err(crate::builtins::wrong_arg_type(&other, "Integer")),
        }
    }

    /// `rb_absint_size(v, &nlz_bits)` -- how many bytes the absolute value
    /// needs. An extension sizes a buffer with it before packing.
    fn rb_absint_size(v: Value, nlz_bits: *mut c_int) -> usize {
        let bits = match unsafe { value_of(v) } {
            RubyValue::Int(n) => 64 - n.unsigned_abs().leading_zeros(),
            RubyValue::BigInt(b) => b.magnitude().bits() as u32,
            other => return Err(crate::builtins::wrong_arg_type(&other, "Integer")),
        };
        let bytes = usize::try_from(bits.div_ceil(8)).unwrap_or(0).max(1);
        if !nlz_bits.is_null() {
            // SAFETY: the caller's own `int` slot.
            unsafe { nlz_bits.write((bytes as u32 * 8 - bits.max(1)) as c_int) };
        }
        Ok(bytes)
    }

    /// `rb_proc_call_with_block(proc, argc, argv, block)`.
    fn rb_proc_call_with_block(
        p: Value,
        argc: c_int,
        argv: *const Value,
        block: Value,
    ) -> Value {
        let recv = unsafe { value_of(p) };
        let args: Vec<RubyValue> = (0..argc.max(0))
            .map(|i| unsafe { value_of(argv.offset(i as isize).read()) })
            .collect();
        let block = match unsafe { value_of(block) } {
            RubyValue::Nil => None,
            b => Some(b),
        };
        to_value(&crate::dispatch::send_value(
            &recv,
            crate::Symbol::intern("call"),
            &args,
            block,
        )?)
    }

    /// `rb_struct_define`'s worker. `outer == 0` is the top-level form.
    fn zeo_cext_struct_define(
        outer: Value,
        name: *const c_char,
        members: *const *const c_char,
        n: c_int,
    ) -> Value {
        let mut args: Vec<RubyValue> = Vec::with_capacity(n.max(0) as usize + 1);
        for i in 0..n.max(0) {
            // SAFETY: `n` NUL-terminated names, collected by cext_va.c.
            let m = unsafe { super::string::borrow_bytes(*members.offset(i as isize), -1) };
            args.push(RubyValue::Symbol(crate::Symbol::intern(
                &String::from_utf8_lossy(&m),
            )));
        }
        let cls = crate::dispatch::send_value(
            &RubyValue::Class(zeo_abi::STRUCT_CLASS),
            crate::Symbol::intern("new"),
            &args,
            None,
        )?;
        // A named Struct is a constant under its namespace, exactly as
        // `Struct.new` assigned to a constant would be.
        let name = unsafe { super::string::borrow_bytes(name, -1) };
        if !name.is_empty()
            && let RubyValue::Class(cid) = &cls
        {
            let owner = if outer == 0 {
                zeo_abi::OBJECT_CLASS
            } else {
                unsafe { as_class(outer)? }
            };
            let leaf = String::from_utf8_lossy(&name).into_owned();
            crate::runtime_meta::name_runtime_class_if_anonymous(*cid, &leaf);
            crate::constants::const_set(owner.0, &leaf, cls.clone());
        }
        to_value(&cls)
    }

    /// How many members a Struct class has, so `csrc/cext_va.c` knows how
    /// many varargs to read.
    fn zeo_cext_struct_size(klass: Value) -> c_long {
        let cls = unsafe { value_of(klass) };
        let members = crate::dispatch::send_value(&cls, crate::Symbol::intern("members"), &[], None)?;
        Ok(match members {
            RubyValue::Array(a) => a.lock().len() as c_long,
            _ => 0,
        })
    }

    fn zeo_cext_struct_new(klass: Value, values: *const Value, n: c_int) -> Value {
        let cls = unsafe { value_of(klass) };
        let args: Vec<RubyValue> = (0..n.max(0))
            .map(|i| unsafe { value_of(values.offset(i as isize).read()) })
            .collect();
        to_value(&crate::dispatch::send_value(&cls, crate::Symbol::intern("new"), &args, None)?)
    }

    /// `rb_hash_lookup(hash, key)`. Unlike `rb_hash_aref` it does NOT consult
    /// the hash's default, which is the whole reason both exist.
    fn rb_hash_lookup(h: Value, key: Value) -> Value {
        let h = unsafe { value_of(h) };
        let key = unsafe { value_of(key) };
        let RubyValue::Hash(h) = &h else {
            return Err(crate::builtins::wrong_arg_type(&h, "Hash"));
        };
        to_value(&crate::value::collections::hash_lookup(h, &key).unwrap_or(RubyValue::Nil))
    }

    fn rb_hash_lookup2(h: Value, key: Value, def: Value) -> Value {
        let hv = unsafe { value_of(h) };
        let key = unsafe { value_of(key) };
        let RubyValue::Hash(hh) = &hv else {
            return Err(crate::builtins::wrong_arg_type(&hv, "Hash"));
        };
        match crate::value::collections::hash_lookup(hh, &key) {
            Some(v) => to_value(&v),
            None => Ok(def),
        }
    }
}

/// A conversion that RAISES when the receiver cannot do it -- `rb_String`
/// and its neighbours.
fn coerce(v: &RubyValue, meth: &str) -> Result<RubyValue, Signal> {
    crate::dispatch::send_value(v, crate::Symbol::intern(meth), &[], None)
}

/// A conversion that answers nil instead -- the `rb_check_*_type` family.
fn check(v: &RubyValue, meth: &str) -> RubyValue {
    let sym = crate::Symbol::intern(meth);
    if !crate::dispatch::responds_to_value(v, sym, false) {
        return RubyValue::Nil;
    }
    crate::dispatch::send_value(v, sym, &[], None).unwrap_or(RubyValue::Nil)
}

/// `st_strcasecmp` and `st_strncasecmp`.
///
/// These are the only `st_*` the census actually uses in bulk -- 8 of 13
/// uses across the 23 C-extension gems -- and neither touches the hash table
/// the rest of the family is. MRI's `st.c` is 3,224 lines and pulls in five
/// internal headers; vendoring it to serve the other five uses is not a
/// trade worth making, so the table itself stays stubbed.
///
/// # Safety
///
/// Both pointers must be NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn st_strcasecmp(a: *const c_char, b: *const c_char) -> c_int {
    unsafe { st_strncasecmp(a, b, usize::MAX) }
}

/// # Safety
///
/// Both pointers must be NUL-terminated, or name `n` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn st_strncasecmp(a: *const c_char, b: *const c_char, n: usize) -> c_int {
    let mut i = 0usize;
    while i < n {
        // SAFETY: the caller's contract -- both runs are readable to their
        // NUL, and the loop stops at the first one.
        let (x, y) = unsafe {
            (
                (a.add(i).read() as u8).to_ascii_lowercase(),
                (b.add(i).read() as u8).to_ascii_lowercase(),
            )
        };
        if x != y {
            return c_int::from(x) - c_int::from(y);
        }
        if x == 0 {
            return 0;
        }
        i += 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cext::scope::Scope;

    #[test]
    fn an_encoding_token_round_trips_and_is_never_null() {
        for id in [
            crate::encoding::ASCII_8BIT,
            crate::encoding::UTF_8,
            crate::encoding::US_ASCII,
        ] {
            let tok = enc_token(id);
            assert!(!tok.is_null(), "encoding {id:?} became NULL");
            assert_eq!(enc_of(tok), id);
        }
        // A NULL token is binary rather than a panic: MRI hands NULL around
        // for "no encoding" and an extension may pass it straight back.
        assert_eq!(enc_of(std::ptr::null()), crate::encoding::ASCII_8BIT);
    }

    #[test]
    fn a_string_built_with_an_encoding_keeps_it() {
        let _scope = Scope::enter();
        let e = unsafe { rb_utf8_encoding() };
        let s = unsafe { rb_enc_str_new(c"hi".as_ptr(), 2, e) };
        assert_eq!(unsafe { rb_enc_get(s) }, e);
        assert_eq!(
            unsafe { rb_enc_get_index(s) },
            crate::encoding::UTF_8.0 as c_int
        );
    }

    #[test]
    fn an_interned_string_comes_back_frozen() {
        let _scope = Scope::enter();
        let e = unsafe { rb_utf8_encoding() };
        let s = unsafe { rb_enc_interned_str(c"k".as_ptr(), 1, e) };
        assert!(unsafe { value_of(s) }.is_frozen());
    }

    #[test]
    fn st_strncasecmp_matches_c_semantics() {
        unsafe {
            assert_eq!(st_strcasecmp(c"Abc".as_ptr(), c"aBC".as_ptr()), 0);
            assert!(st_strcasecmp(c"abc".as_ptr(), c"abd".as_ptr()) < 0);
            assert!(st_strcasecmp(c"abd".as_ptr(), c"abc".as_ptr()) > 0);
            // The bound stops the comparison even where the strings differ.
            assert_eq!(st_strncasecmp(c"abcX".as_ptr(), c"ABCY".as_ptr(), 3), 0);
            assert!(st_strncasecmp(c"abcX".as_ptr(), c"ABCY".as_ptr(), 4) != 0);
            // A shorter string stops at its NUL rather than reading past it.
            assert!(st_strcasecmp(c"ab".as_ptr(), c"abc".as_ptr()) < 0);
        }
    }
}

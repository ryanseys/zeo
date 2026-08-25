//! `rb_ary_*` and `rb_hash_*`, and the array layout accessors.
//!
//! # `RARRAY_CONST_PTR` pins, for the same reason `RSTRING_PTR` does
//!
//! A zeo Array is a `Vec<RubyValue>` behind a lock, and a `RubyValue` is 24
//! bytes where a `VALUE` is 8. So there is no contiguous run of `VALUE`s to
//! hand out at all, not merely no stable one: the array has to be projected
//! into a fresh buffer of handles.
//!
//! That makes the pin cost real, and it is why the census matters:
//! `RARRAY_AREF` appears 111 times across the 23 C-extension gems and
//! `RARRAY_PTR` exactly once. So `RARRAY_AREF` is a call that reads one
//! element and pins nothing, and only the pointer forms project.
//!
//! A projected buffer is READ-ONLY in practice. `RARRAY_PTR` hands back a
//! `VALUE *` an extension could store through, and that store cannot reach
//! the Ruby array -- writing a raw `VALUE` back would need the handle to
//! still be pinned, which nothing guarantees. `RARRAY_ASET` is the supported
//! way, it is what `RARRAY_PTR_USE` expands to, and the one `RARRAY_PTR` in
//! the census only reads.

use super::convert::{to_value, value_of};
use super::value::Value;
use crate::RubyValue;
use crate::collections::{RArray, RHash};
use std::cell::RefCell;
use std::ffi::{c_int, c_long};

thread_local! {
    /// Projected `VALUE` buffers, one per array that asked. Keyed by payload
    /// address so two `RARRAY_CONST_PTR` calls answer one pointer.
    static PROJECTIONS: RefCell<Vec<(usize, Box<[Value]>)>> = const { RefCell::new(Vec::new()) };
}

/// Drop every projection. Called from the scope pop, with the string pins.
pub(super) fn flush_projections() {
    PROJECTIONS.with_borrow_mut(|p| p.clear());
}

/// # Safety
///
/// `v` is an extension's own `VALUE`.
unsafe fn as_ary(v: Value) -> Result<RArray, crate::Signal> {
    match unsafe { value_of(v) } {
        RubyValue::Array(a) => Ok(a),
        other => Err(crate::builtins::wrong_arg_type(&other, "Array")),
    }
}

/// # Safety
///
/// `v` is an extension's own `VALUE`.
unsafe fn as_hash(v: Value) -> Result<RHash, crate::Signal> {
    match unsafe { value_of(v) } {
        RubyValue::Hash(h) => Ok(h),
        other => Err(crate::builtins::wrong_arg_type(&other, "Hash")),
    }
}

crate::cext_fn! {
    fn rb_ary_new() -> Value {
        to_value(&RubyValue::Array(crate::value::collections::array_new(Vec::new())))
    }

    /// `rb_ary_new2` / `rb_ary_new_capa`: a capacity hint. zeo's arrays grow
    /// on demand, so the hint is accepted and dropped.
    fn rb_ary_new_capa(_capa: c_long) -> Value {
        to_value(&RubyValue::Array(crate::value::collections::array_new(Vec::new())))
    }

    /// `rb_ary_new_from_values(n, ptr)` -- `rb_ary_new4`'s real name.
    fn rb_ary_new_from_values(n: c_long, elts: *const Value) -> Value {
        let mut out = Vec::with_capacity(n.max(0) as usize);
        for i in 0..n.max(0) {
            // SAFETY: the caller promised `n` readable `VALUE`s.
            out.push(unsafe { value_of(elts.offset(i as isize).read()) });
        }
        to_value(&RubyValue::Array(crate::value::collections::array_new(out)))
    }

    fn rb_ary_push(v: Value, item: Value) -> Value {
        let a = unsafe { as_ary(v)? };
        let item = unsafe { value_of(item) };
        crate::builtins::array::array_push_checked(&a, item)?;
        Ok(v)
    }

    fn rb_ary_pop(v: Value) -> Value {
        let a = unsafe { as_ary(v)? };
        to_value(&crate::builtins::array::array_pop_checked(&a)?)
    }

    fn rb_ary_shift(v: Value) -> Value {
        let a = unsafe { as_ary(v)? };
        to_value(&crate::builtins::array::array_shift_checked(&a)?)
    }

    /// `rb_ary_entry`. A negative index counts from the end and an index past
    /// either end answers nil, both as `Array#[]` does.
    fn rb_ary_entry(v: Value, idx: c_long) -> Value {
        let a = unsafe { as_ary(v)? };
        let g = a.lock();
        let len = g.len() as c_long;
        let i = if idx < 0 { idx + len } else { idx };
        let out = if i < 0 || i >= len {
            RubyValue::Nil
        } else {
            g[i as usize].clone()
        };
        drop(g);
        to_value(&out)
    }

    fn rb_ary_clear(v: Value) -> Value {
        unsafe { as_ary(v)? }.lock().clear();
        Ok(v)
    }

    fn rb_hash_new() -> Value {
        to_value(&RubyValue::Hash(crate::value::collections::hash_new(Vec::new())))
    }

    /// `rb_hash_new_capa`. Same as `rb_ary_new_capa`: the hint is dropped.
    fn rb_hash_new_capa(_capa: c_long) -> Value {
        to_value(&RubyValue::Hash(crate::value::collections::hash_new(Vec::new())))
    }

    /// `rb_hash_aref`. Goes through `Hash#[]` rather than the storage, so a
    /// missing key reaches the hash's DEFAULT -- including a default PROC,
    /// which is the whole difference from `rb_hash_lookup`.
    fn rb_hash_aref(v: Value, key: Value) -> Value {
        let hv = unsafe { value_of(v) };
        let _ = unsafe { as_hash(v)? };
        let key = unsafe { value_of(key) };
        to_value(&crate::dispatch::send_value(
            &hv,
            crate::Symbol::intern("[]"),
            &[key],
            None,
        )?)
    }

    fn rb_hash_aset(v: Value, key: Value, val: Value) -> Value {
        let h = unsafe { as_hash(v)? };
        let key = unsafe { value_of(key) };
        let val = unsafe { value_of(val) };
        crate::value::collections::hash_set_checked(&h, key, val)?;
        Ok(v)
    }

    fn rb_hash_size(v: Value) -> Value {
        let h = unsafe { as_hash(v)? };
        let n = h.lock().len() as i64;
        to_value(&RubyValue::Int(n))
    }

    fn rb_hash_clear(v: Value) -> Value {
        unsafe { as_hash(v)? }.lock().clear();
        Ok(v)
    }
}

/// `RARRAY_LEN`'s runtime half.
///
/// # Safety
///
/// `v` must be a live Array `VALUE`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rbimpl_zeo_ary_len(v: Value) -> c_long {
    match unsafe { as_ary(v) } {
        Ok(a) => a.lock().len() as c_long,
        Err(sig) => super::jmp::raise(sig),
    }
}

/// `RARRAY_AREF`'s runtime half: one element, no projection.
///
/// # Safety
///
/// `v` must be a live Array `VALUE`, and `i` in range -- MRI's macro does not
/// bounds-check either, and an extension that reads past the end is as wrong
/// here as there. Out of range answers `Qnil` rather than reading memory,
/// which is the safe direction.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rbimpl_zeo_ary_aref(v: Value, i: c_long) -> Value {
    let out = (|| -> Result<Value, crate::Signal> {
        let a = unsafe { as_ary(v)? };
        let g = a.lock();
        let item = g.get(usize::try_from(i).unwrap_or(usize::MAX)).cloned();
        drop(g);
        to_value(&item.unwrap_or(RubyValue::Nil))
    })();
    match out {
        Ok(v) => v,
        Err(sig) => super::jmp::raise(sig),
    }
}

/// `RARRAY_ASET`'s runtime half.
///
/// # Safety
///
/// `v` must be a live Array `VALUE`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rbimpl_zeo_ary_aset(v: Value, i: c_long, item: Value) {
    let out = (|| -> Result<(), crate::Signal> {
        let a = unsafe { as_ary(v)? };
        let item = unsafe { value_of(item) };
        let mut g = a.lock();
        if let Some(slot) = g.vec().get_mut(usize::try_from(i).unwrap_or(usize::MAX)) {
            *slot = item;
        }
        Ok(())
    })();
    if let Err(sig) = out {
        super::jmp::raise(sig);
    }
}

/// `RARRAY_CONST_PTR`'s runtime half: project the array into a buffer of
/// `VALUE`s and pin it for the scope. See this module's docs for why this is
/// a projection rather than a borrow, and why it is read-only.
///
/// # Safety
///
/// `v` must be a live Array `VALUE`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rbimpl_zeo_ary_const_ptr(v: Value) -> *const Value {
    let out = (|| -> Result<*const Value, crate::Signal> {
        let a = unsafe { as_ary(v)? };
        let key = std::sync::Arc::as_ptr(&a) as *const () as usize;
        if let Some(p) = PROJECTIONS.with_borrow_mut(|p| {
            p.iter_mut()
                .find(|(k, _)| *k == key)
                .map(|(_, buf)| buf.as_ptr())
        }) {
            return Ok(p);
        }
        let items = a.lock().to_vec();
        let mut buf = Vec::with_capacity(items.len());
        for item in &items {
            buf.push(to_value(item)?);
        }
        let buf = buf.into_boxed_slice();
        Ok(PROJECTIONS.with_borrow_mut(|p| {
            p.push((key, buf));
            p.last().expect("just pushed").1.as_ptr()
        }))
    })();
    match out {
        Ok(p) => p,
        Err(sig) => super::jmp::raise(sig),
    }
}

/// `RARRAY_PTR`'s runtime half. The same projection, cast mutable because the
/// macro's type says so; a store through it does not reach the Ruby array.
///
/// # Safety
///
/// `v` must be a live Array `VALUE`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rbimpl_zeo_ary_ptr(v: Value) -> *mut Value {
    unsafe { rbimpl_zeo_ary_const_ptr(v).cast_mut() }
}

crate::cext_fn! {
    // ---- arrays ----------------------------------------------------------

    /// `rb_assoc_new(a, b)`: the two-element Array a Hash iteration yields.
    fn rb_assoc_new(a: Value, b: Value) -> Value {
        let pair = vec![unsafe { value_of(a) }, unsafe { value_of(b) }];
        to_value(&RubyValue::Array(crate::value::collections::array_new(pair)))
    }

    /// `rb_ary_hidden_new(capa)`: MRI's internal array, invisible to
    /// `ObjectSpace`. zeo has no hidden objects, so it is a plain one -- the
    /// difference is visible only to a heap walk, and an extension using
    /// this as scratch cannot see it.
    fn rb_ary_hidden_new(_capa: c_long) -> Value {
        to_value(&RubyValue::Array(crate::value::collections::array_new(Vec::new())))
    }

    /// `rb_ary_aref(argc, argv, ary)`: `Array#[]`, which takes an index, a
    /// pair or a Range depending on `argc`.
    fn rb_ary_aref(argc: c_int, argv: *const Value, ary: Value) -> Value {
        let a = unsafe { value_of(ary) };
        let args = unsafe { super::object::args_of(argc, argv) };
        to_value(&super::object::send(&a, "[]", &args)?)
    }

    fn rb_ary_store(v: Value, idx: c_long, item: Value) -> () {
        let a = unsafe { value_of(v) };
        let item = unsafe { value_of(item) };
        super::object::send(&a, "[]=", &[RubyValue::Int(idx as i64), item])?;
        Ok(())
    }

    fn rb_ary_delete_at(v: Value, idx: c_long) -> Value {
        let a = unsafe { value_of(v) };
        to_value(&super::object::send(&a, "delete_at", &[RubyValue::Int(idx as i64)])?)
    }

    fn rb_ary_rotate(v: Value, n: c_long) -> Value {
        let a = unsafe { value_of(v) };
        to_value(&super::object::send(&a, "rotate!", &[RubyValue::Int(n as i64)])?)
    }

    fn rb_ary_subseq(v: Value, beg: c_long, len: c_long) -> Value {
        let a = unsafe { value_of(v) };
        let args = [RubyValue::Int(beg as i64), RubyValue::Int(len as i64)];
        to_value(&super::object::send(&a, "[]", &args)?)
    }

    /// `rb_ary_resize(ary, len)`: grow with nils or truncate, in place.
    fn rb_ary_resize(v: Value, len: c_long) -> Value {
        let a = unsafe { as_ary(v)? };
        check_writable(&a)?;
        let mut g = a.lock();
        g.resize(len.max(0) as usize, RubyValue::Nil);
        drop(g);
        Ok(v)
    }

    /// `rb_ary_cat(ary, ptr, len)`: append `len` `VALUE`s at once.
    fn rb_ary_cat(v: Value, items: *const Value, len: c_long) -> Value {
        let a = unsafe { as_ary(v)? };
        check_writable(&a)?;
        for i in 0..len.max(0) {
            // SAFETY: the caller promised `len` readable `VALUE`s.
            let item = unsafe { value_of(items.offset(i as isize).read()) };
            crate::builtins::array::array_push_checked(&a, item)?;
        }
        Ok(v)
    }

    /// `rb_ary_resurrect(ary)`: a new Array with the same elements.
    fn rb_ary_resurrect(v: Value) -> Value {
        let a = unsafe { as_ary(v)? };
        let copy = a.lock().to_vec();
        to_value(&RubyValue::Array(crate::value::collections::array_new(copy)))
    }

    /// `rb_ary_modify(ary)`: MRI un-shares the buffer and checks the freeze.
    /// zeo has nothing to un-share, so the freeze check is the whole of it.
    fn rb_ary_modify(v: Value) -> () {
        let a = unsafe { as_ary(v)? };
        check_writable(&a)?;
        Ok(())
    }

    /// `rb_ary_free(ary)`: MRI releases the element buffer early. zeo's
    /// Array is refcounted, and freeing it here would leave the `VALUE` the
    /// extension still holds dangling.
    fn rb_ary_free(v: Value) -> () {
        unsafe { as_ary(v)? };
        Ok(())
    }

    /// `rb_ary_shared_with_p(a, b)`: do the two share one element buffer?
    /// zeo's arrays never share, so the honest answer is always false -- and
    /// false is the answer that makes every caller take the copying path.
    fn rb_ary_shared_with_p(_a: Value, _b: Value) -> Value {
        Ok(super::convert::boolean(false))
    }

    /// `rb_ary_each(ary)`: yield every element to the CURRENT block and
    /// answer the array. Not `Array#each`, which with no block answers an
    /// Enumerator -- that is why it cannot be a forwarded row.
    fn rb_ary_each(v: Value) -> Value {
        let a = unsafe { as_ary(v)? };
        let items = a.lock().to_vec();
        for item in items {
            super::call::yield_to_block(&[item])?;
        }
        Ok(v)
    }

    /// `rb_ary_ptr_use_start` / `_end`: the pair `RARRAY_PTR_USE` expands
    /// to. The projection is read-only and lives until the scope pops, so
    /// the `end` half has nothing to release -- see this module's docs for
    /// why a store through the pointer cannot reach the Array.
    fn rb_ary_ptr_use_start(v: Value) -> *mut Value {
        Ok(unsafe { rbimpl_zeo_ary_const_ptr(v) }.cast_mut())
    }

    fn rb_ary_ptr_use_end(v: Value) -> () {
        unsafe { as_ary(v)? };
        Ok(())
    }

    // ---- hashes ----------------------------------------------------------

    fn rb_hash(v: Value) -> Value {
        let recv = unsafe { value_of(v) };
        to_value(&super::object::send(&recv, "hash", &[])?)
    }

    fn rb_hash_size_num(v: Value) -> usize {
        let h = unsafe { as_hash(v)? };
        let n = h.lock().len();
        Ok(n)
    }

    /// `rb_hash_set_ifnone(hash, default)`: the value a missing key answers.
    fn rb_hash_set_ifnone(v: Value, default: Value) -> Value {
        let hv = unsafe { value_of(v) };
        let d = unsafe { value_of(default) };
        super::object::send(&hv, "default=", &[d])?;
        Ok(v)
    }

    /// `rb_hash_bulk_insert(n, pairs, hash)`: `n` alternating keys and
    /// values, which is how a literal Hash is built.
    fn rb_hash_bulk_insert(n: c_long, pairs: *const Value, hash: Value) -> () {
        let h = unsafe { as_hash(hash)? };
        let mut i = 0;
        while i + 1 < n.max(0) {
            // SAFETY: the caller promised `n` readable `VALUE`s.
            let k = unsafe { value_of(pairs.offset(i as isize).read()) };
            let val = unsafe { value_of(pairs.offset(i as isize + 1).read()) };
            crate::value::collections::hash_set(&h, k, val);
            i += 2;
        }
        Ok(())
    }

    /// `rb_hash_delete_if(hash)`: drop every pair the CURRENT block answers
    /// truthy for. Not `Hash#delete_if`, which with no block answers an
    /// Enumerator.
    fn rb_hash_delete_if(v: Value) -> Value {
        let h = unsafe { as_hash(v)? };
        check_hash_writable(&h)?;
        let pairs: Vec<(RubyValue, RubyValue)> =
            h.lock().iter().map(|(_, (k, val))| (k.clone(), val.clone())).collect();
        for (k, val) in pairs {
            let verdict = super::call::yield_to_block(&[k.clone(), val])?;
            if super::convert::truthy(to_value(&verdict)?) {
                crate::value::collections::hash_delete(&h, &k);
            }
        }
        Ok(v)
    }

    /// `rb_hash_update_by(h1, h2, func)`: merge `h2` into `h1`, asking
    /// `func(key, old, new)` whenever both have the key. A null `func` takes
    /// `h2`'s value, which is what `Hash#merge!` without a block does.
    fn rb_hash_update_by(
        dst: Value,
        src: Value,
        func: Option<unsafe extern "C" fn(Value, Value, Value) -> Value>,
    ) -> Value {
        let d = unsafe { as_hash(dst)? };
        let s = unsafe { as_hash(src)? };
        check_hash_writable(&d)?;
        let pairs: Vec<(RubyValue, RubyValue)> =
            s.lock().iter().map(|(_, (k, v))| (k.clone(), v.clone())).collect();
        for (k, new) in pairs {
            let old = crate::value::collections::hash_lookup(&d, &k);
            let value = match (old, func) {
                (Some(old), Some(f)) => {
                    let (rk, ro, rn) = (to_value(&k)?, to_value(&old)?, to_value(&new)?);
                    // SAFETY: the caller's own callback, on pinned handles.
                    let out = super::jmp::protect(|| unsafe { f(rk, ro, rn) })?;
                    unsafe { value_of(out) }
                }
                _ => new,
            };
            crate::value::collections::hash_set(&d, k, value);
        }
        Ok(dst)
    }

    // ---- Struct ----------------------------------------------------------

    /// `rb_struct_alloc(klass, values)`: build from an ARRAY of values,
    /// where `Struct#new` takes them splatted. That difference is why this
    /// cannot be a forwarded row.
    fn rb_struct_alloc(klass: Value, values: Value) -> Value {
        let cls = unsafe { value_of(klass) };
        let args = unsafe { splat(values) };
        to_value(&crate::dispatch::send_value(&cls, crate::Symbol::intern("new"), &args, None)?)
    }

    /// `rb_struct_alloc_noinit(klass)`: an instance with every member nil,
    /// without running `initialize`.
    fn rb_struct_alloc_noinit(klass: Value) -> Value {
        let cls = unsafe { value_of(klass) };
        to_value(&super::object::send(&cls, "allocate", &[])?)
    }

    /// `rb_struct_initialize(self, values)`: the same array-taking shape.
    fn rb_struct_initialize(recv: Value, values: Value) -> Value {
        let s = unsafe { value_of(recv) };
        let args = unsafe { splat(values) };
        crate::dispatch::send_value(&s, crate::Symbol::intern("initialize"), &args, None)?;
        Ok(recv)
    }

    fn rb_struct_getmember(recv: Value, id: super::symbol::Id) -> Value {
        let s = unsafe { value_of(recv) };
        let name = RubyValue::Symbol(super::symbol::symbol_of(id));
        to_value(&super::object::send(&s, "[]", &[name])?)
    }

    fn rb_struct_s_members(klass: Value) -> Value {
        let cls = unsafe { value_of(klass) };
        to_value(&super::object::send(&cls, "members", &[])?)
    }
}

/// A frozen Array cannot be written, and MRI's own message names it.
fn check_writable(a: &RArray) -> Result<(), crate::Signal> {
    let v = RubyValue::Array(a.clone());
    if v.is_frozen() {
        return Err(crate::dispatch::raise_error(
            "FrozenError",
            format!("can't modify frozen Array: {}", v.to_display_string()),
        ));
    }
    Ok(())
}

fn check_hash_writable(h: &RHash) -> Result<(), crate::Signal> {
    let v = RubyValue::Hash(h.clone());
    if v.is_frozen() {
        return Err(crate::dispatch::raise_error(
            "FrozenError",
            format!("can't modify frozen Hash: {}", v.to_display_string()),
        ));
    }
    Ok(())
}

/// The `values` argument the three `rb_struct_*` entries take: an Array, and
/// nil for none.
///
/// # Safety
///
/// `v` must be a live `VALUE`.
unsafe fn splat(v: Value) -> Vec<RubyValue> {
    match unsafe { value_of(v) } {
        RubyValue::Array(a) => a.lock().to_vec(),
        RubyValue::Nil => Vec::new(),
        other => vec![other],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cext::scope::Scope;

    fn items(v: &Value) -> Vec<RubyValue> {
        match unsafe { value_of(*v) } {
            RubyValue::Array(a) => a.lock().to_vec(),
            other => panic!("not an Array: {other:?}"),
        }
    }

    fn int(v: Value) -> i64 {
        match unsafe { value_of(v) } {
            RubyValue::Int(n) => n,
            other => panic!("not an Int: {other:?}"),
        }
    }

    #[test]
    fn push_pop_and_shift_answer_what_ruby_would() {
        let _scope = Scope::enter();
        let a = unsafe { rb_ary_new() };
        for n in 1..=3 {
            unsafe { rb_ary_push(a, super::super::value::fixnum(n)) };
        }
        assert_eq!(items(&a).len(), 3);
        assert_eq!(int(unsafe { rb_ary_pop(a) }), 3);
        assert_eq!(int(unsafe { rb_ary_shift(a) }), 1);
        assert_eq!(items(&a).len(), 1);
        unsafe { rb_ary_clear(a) };
        assert!(items(&a).is_empty());
    }

    /// A negative index counts from the end and either overrun answers nil,
    /// as `Array#[]` does. Getting this wrong reads someone else's memory in
    /// MRI, so it is worth pinning.
    #[test]
    fn entry_handles_both_ends_the_way_array_index_does() {
        let _scope = Scope::enter();
        let a = unsafe { rb_ary_new() };
        for n in 10..13 {
            unsafe { rb_ary_push(a, super::super::value::fixnum(n)) };
        }
        assert_eq!(int(unsafe { rb_ary_entry(a, 0) }), 10);
        assert_eq!(int(unsafe { rb_ary_entry(a, 2) }), 12);
        assert_eq!(int(unsafe { rb_ary_entry(a, -1) }), 12);
        assert_eq!(int(unsafe { rb_ary_entry(a, -3) }), 10);
        assert_eq!(unsafe { rb_ary_entry(a, 3) }, super::super::value::Q_NIL);
        assert_eq!(unsafe { rb_ary_entry(a, -4) }, super::super::value::Q_NIL);
    }

    #[test]
    fn a_projection_is_stable_and_reads_the_elements() {
        let _scope = Scope::enter();
        let a = unsafe { rb_ary_new() };
        for n in 1..=3 {
            unsafe { rb_ary_push(a, super::super::value::fixnum(n)) };
        }
        let p = unsafe { rbimpl_zeo_ary_const_ptr(a) };
        assert_eq!(p, unsafe { rbimpl_zeo_ary_const_ptr(a) }, "two projections");
        assert_eq!(unsafe { rbimpl_zeo_ary_len(a) }, 3);
        for i in 0..3i64 {
            assert_eq!(int(unsafe { p.offset(i as isize).read() }), i + 1);
            assert_eq!(int(unsafe { rbimpl_zeo_ary_aref(a, i) }), i + 1);
        }
    }

    #[test]
    fn aset_reaches_the_ruby_array_where_a_projection_store_does_not() {
        let _scope = Scope::enter();
        let a = unsafe {
            rb_ary_new_from_values(
                2,
                [
                    super::super::value::fixnum(1),
                    super::super::value::fixnum(2),
                ]
                .as_ptr(),
            )
        };
        unsafe { rbimpl_zeo_ary_aset(a, 0, super::super::value::fixnum(9)) };
        assert_eq!(int(unsafe { rbimpl_zeo_ary_aref(a, 0) }), 9);
        // Out of range is a no-op rather than a write past the end.
        unsafe { rbimpl_zeo_ary_aset(a, 5, super::super::value::fixnum(7)) };
        assert_eq!(items(&a).len(), 2);
    }

    #[test]
    fn a_hash_round_trips_a_key_and_reports_its_size() {
        let _scope = Scope::enter();
        let h = unsafe { rb_hash_new() };
        let k = unsafe { rb_utf8_str_new(c"k".as_ptr(), 1) };
        unsafe { rb_hash_aset(h, k, super::super::value::fixnum(5)) };
        assert_eq!(int(unsafe { rb_hash_aref(h, k) }), 5);
        assert_eq!(int(unsafe { rb_hash_size(h) }), 1);
        unsafe { rb_hash_clear(h) };
        assert_eq!(int(unsafe { rb_hash_size(h) }), 0);
    }

    use super::super::string::rb_utf8_str_new;

    #[test]
    #[should_panic(expected = "TypeError: wrong argument type")]
    fn a_non_array_receiver_refuses() {
        let _scope = Scope::enter();
        let _ = unsafe { as_ary(super::super::value::fixnum(1)) };
    }
}

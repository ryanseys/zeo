//! The GC surface: marking, roots, and the entries that ask about a
//! collection.
//!
//! Most of MRI's GC API is advice zeo does not need. A `VALUE` here is
//! refcounted and pinned by the enclosing [`super::scope`], so
//! `RB_GC_GUARD`, the write barriers and `rb_gc_register_mark_object` have
//! nothing to protect against -- and saying so once here is better than 14
//! separate refusals.
//!
//! What IS real:
//!
//! * `rb_gc_mark` and its neighbours, which report an edge during a
//!   TypedData walk. Those live in [`super::call`] beside the walk.
//! * `rb_gc_count` and `rb_gc_stat`, which read the cycle collector's own
//!   counters -- so an extension asking "did a collection happen" gets the
//!   truth rather than a zero.
//! * The `rb_alloc_tmp_buffer` family, which is a real allocation with a
//!   real lifetime: MRI hangs a scratch buffer off a `VALUE` so a raise
//!   frees it. Here the scope frees it, which is the same guarantee.
//!
//! # `rb_gc_mark_maybe` takes anything
//!
//! It is called on a word that MIGHT be a `VALUE` -- MRI's conservative
//! scanner uses it. A garbage word must not be dereferenced, so it is
//! checked against the live handle table first rather than trusted.

use super::convert::{to_value, value_of};
use super::object::send;
use super::value::{self, Value};
use crate::{RubyValue, Signal};
use std::ffi::{c_int, c_void};

/// `GC`, or a `NameError` if the module is somehow absent.
fn gc_module() -> Result<RubyValue, Signal> {
    crate::constants::const_get(zeo_abi::OBJECT_CLASS.0, "GC").ok_or_else(|| {
        crate::dispatch::raise_error("NameError", "uninitialized constant GC".into())
    })
}

crate::cext_fn! {
    // ---- running a collection --------------------------------------------

    fn rb_gc() -> () {
        send(&gc_module()?, "start", &[])?;
        Ok(())
    }

    fn rb_gc_start() -> Value {
        to_value(&send(&gc_module()?, "start", &[])?)
    }

    fn rb_gc_enable() -> Value {
        to_value(&send(&gc_module()?, "enable", &[])?)
    }

    fn rb_gc_disable() -> Value {
        to_value(&send(&gc_module()?, "disable", &[])?)
    }

    fn rb_gc_count() -> usize {
        match send(&gc_module()?, "count", &[])? {
            RubyValue::Int(n) => Ok(n.max(0) as usize),
            _ => Ok(0),
        }
    }

    /// `rb_gc_stat(key)`: one row of `GC.stat`. A Symbol asks for that row;
    /// a Hash asks it to be filled, which zeo answers as 0 because the
    /// caller already has the Hash and `GC.stat` is the way to fill it.
    fn rb_gc_stat(key: Value) -> usize {
        let k = unsafe { value_of(key) };
        if !matches!(k, RubyValue::Symbol(_)) {
            return Ok(0);
        }
        match send(&gc_module()?, "stat", &[k])? {
            RubyValue::Int(n) => Ok(n.max(0) as usize),
            _ => Ok(0),
        }
    }

    fn rb_gc_latest_gc_info(arg: Value) -> Value {
        let a = unsafe { value_of(arg) };
        to_value(&send(&gc_module()?, "latest_gc_info", &[a])?)
    }

    /// `rb_during_gc()`: is a collection running right now? True only
    /// inside a `dmark`, which is the one place an extension can observe
    /// one.
    fn rb_during_gc() -> c_int {
        Ok(c_int::from(super::data::marking()))
    }

    /// `rb_gc_adjust_memory_usage(diff)`: tell MRI about memory the
    /// extension allocated itself, so its trigger accounts for it. zeo's
    /// collector triggers on the allocation registry rather than on bytes,
    /// so there is no counter this could move.
    fn rb_gc_adjust_memory_usage(_diff: isize) -> () {
        Ok(())
    }

    // ---- marking ---------------------------------------------------------

    /// `rb_gc_mark_maybe(v)`: mark `v` only if it really is a live object.
    /// The caller found the word by scanning memory, so it may be anything
    /// -- dereferencing it unchecked is the bug this entry exists to avoid.
    fn rb_gc_mark_maybe(v: Value) -> () {
        if !super::data::marking() {
            return Ok(());
        }
        if value::is_special_const(v) {
            // An immediate has no edge to report and cannot be a cycle.
            return Ok(());
        }
        if super::handles::is_live(v) {
            super::data::mark_edge(unsafe { value_of(v) });
        }
        Ok(())
    }

    /// `rb_gc_register_mark_object(v)`: keep `v` alive for the whole
    /// process. A pin in the permanent scope is exactly that, and it is what
    /// [`super::globals`] already uses for `rb_cObject` and its 91
    /// neighbours.
    fn rb_gc_register_mark_object(v: Value) -> () {
        if !value::is_special_const(v) {
            super::handles::pin_forever(v);
        }
        Ok(())
    }

    /// The write barriers. zeo's references are `Arc`s written under a lock,
    /// so there is no generational invariant an extension could break by
    /// storing without telling anyone.
    fn rb_gc_writebarrier(_a: Value, _b: Value) -> () {
        Ok(())
    }

    fn rb_gc_writebarrier_unprotect(_v: Value) -> () {
        Ok(())
    }

    /// `rb_mark_tbl` / `rb_mark_hash` / `rb_mark_set`: mark every `VALUE` in
    /// an `st_table`. Which HALF is a `VALUE` differs, and getting it wrong
    /// would hand the walk a `char *` as an object:
    ///
    /// | Entry | Marks |
    /// |---|---|
    /// | `rb_mark_set` | the keys |
    /// | `rb_mark_hash` | keys AND values |
    /// | `rb_mark_tbl` | the values |
    fn rb_mark_tbl(tbl: *mut super::st::StTable) -> () {
        mark_table(tbl, false, true)
    }

    fn rb_mark_tbl_no_pin(tbl: *mut super::st::StTable) -> () {
        mark_table(tbl, false, true)
    }

    fn rb_mark_set(tbl: *mut super::st::StTable) -> () {
        mark_table(tbl, true, false)
    }

    fn rb_mark_hash(tbl: *mut super::st::StTable) -> () {
        mark_table(tbl, true, true)
    }

    /// `rb_gc_update_tbl_refs`: rewrite an `st_table`'s values after a
    /// compaction moved them. zeo's objects never move -- a handle is
    /// canonical per object for the object's whole life -- so there is
    /// nothing to rewrite.
    fn rb_gc_update_tbl_refs(_tbl: *mut super::st::StTable) -> () {
        Ok(())
    }

    /// `rb_gc_copy_finalizer(dst, src)`: `dup` carries no finalizer in Ruby
    /// either, and MRI's own entry copies only when one exists. zeo attaches
    /// finalizers through `ObjectSpace.define_finalizer`, which the two
    /// objects do not share.
    fn rb_gc_copy_finalizer(_dst: Value, _src: Value) -> () {
        Ok(())
    }

    // ---- the scratch buffer ----------------------------------------------

    /// `rb_alloc_tmp_buffer(&store, len)`: a scratch buffer that is freed if
    /// the C code raises before it can free it itself. MRI hangs it off the
    /// `VALUE` in `*store`; zeo hangs it off the current scope, which
    /// unwinds on the raising path too.
    fn rb_alloc_tmp_buffer(store: *mut Value, len: isize) -> *mut c_void {
        tmp_buffer(store, len.max(0) as usize)
    }

    fn rb_alloc_tmp_buffer_with_count(store: *mut Value, size: usize, count: usize) -> *mut c_void {
        let total = size.checked_mul(count).ok_or_else(|| {
            crate::dispatch::raise_error("NoMemoryError", "buffer size overflow".into())
        })?;
        tmp_buffer(store, total)
    }

    /// `rb_free_tmp_buffer(&store)`: free it early. The scope would free it
    /// anyway, so this only shortens the life -- and clearing the slot is
    /// what stops a second free.
    fn rb_free_tmp_buffer(store: *mut Value) -> () {
        if store.is_null() {
            return Ok(());
        }
        // SAFETY: the caller's own `VALUE` slot, which `rb_alloc_tmp_buffer`
        // wrote.
        let held = unsafe { store.read() };
        release_tmp(held);
        unsafe { store.write(value::Q_NIL) };
        Ok(())
    }

    // ---- generic ivars ---------------------------------------------------

    /// `rb_copy_generic_ivar(dst, src)`: MRI keeps the ivars of a non-object
    /// `VALUE` in a side table, and `dup` has to copy that row. zeo's
    /// side table is `value_ivars`, and `initialize_copy` is what fills it.
    fn rb_copy_generic_ivar(dst: Value, src: Value) -> () {
        let (d, s) = (unsafe { value_of(dst) }, unsafe { value_of(src) });
        let names = match send(&s, "instance_variables", &[])? {
            RubyValue::Array(a) => a.lock().to_vec(),
            _ => Vec::new(),
        };
        for name in names {
            let v = crate::dispatch::instance_variable_get(&s, &name)?;
            crate::dispatch::instance_variable_set(&d, &name, v)?;
        }
        Ok(())
    }

    /// `rb_free_generic_ivar(v)`: drop that row. zeo's side table evicts
    /// with the object, so calling this early would only lose ivars the
    /// caller can still reach.
    fn rb_free_generic_ivar(_v: Value) -> () {
        Ok(())
    }
}

/// Report every `VALUE` in an `st_table` as an edge, during a walk.
fn mark_table(tbl: *mut super::st::StTable, keys: bool, values: bool) -> Result<(), Signal> {
    if !super::data::marking() {
        return Ok(());
    }
    for (k, v) in super::st::rows_of(tbl) {
        for (want, raw) in [(keys, k), (values, v)] {
            if want && !value::is_special_const(raw) && super::handles::is_live(raw) {
                super::data::mark_edge(unsafe { value_of(raw) });
            }
        }
    }
    Ok(())
}

thread_local! {
    /// Scratch buffers, by the `VALUE` handed back in `*store`. Freed with
    /// the scope, so a raise cannot strand one.
    static TMP: std::cell::RefCell<Vec<(Value, Vec<u8>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Drop every scratch buffer. Called from the scope pop with the pins.
pub(super) fn flush_tmp_buffers() {
    TMP.with_borrow_mut(Vec::clear);
}

fn tmp_buffer(store: *mut Value, len: usize) -> Result<*mut c_void, Signal> {
    // The `VALUE` is a real String, so `*store` is a live object an
    // extension may pass to `rb_gc_mark` or print -- as MRI's is.
    let holder = to_value(&crate::builtins::string::str_value_in_enc(
        crate::encoding::ASCII_8BIT,
        "",
    ))?;
    let mut bytes = vec![0u8; len];
    let p = bytes.as_mut_ptr().cast::<c_void>();
    TMP.with_borrow_mut(|t| t.push((holder, bytes)));
    if !store.is_null() {
        // SAFETY: the caller's own `VALUE` slot.
        unsafe { store.write(holder) };
    }
    Ok(p)
}

fn release_tmp(holder: Value) {
    TMP.with_borrow_mut(|t| t.retain(|(v, _)| *v != holder));
}

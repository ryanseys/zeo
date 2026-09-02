//! The views every payload-struct cast macro answers.
//!
//! # The rule
//!
//! A payload struct carries UPSTREAM'S LAYOUT. `X(obj)` is a call, not a
//! cast, and answers a view the runtime owns. The view ALIASES real storage
//! wherever zeo owns that storage as C-shaped memory, and is REFILLED from
//! the object on every reach wherever it does not. A field zeo has no answer
//! for is zero, and the accessor named for it raises.
//!
//! That is one rule for all eight structs, and it is why upstream's own
//! `RSTRING_PTR`, `RARRAY_AREF`, `ROBJECT_FIELDS`, `RREGEXP_SRC`,
//! `RMATCH_REGS`, `DATA_PTR` and `RTYPEDDATA_DATA` are unpatched code again:
//! each reads a field, and the field is there.
//!
//! # Aliased, and refilled
//!
//! [`rb_zeo_rdata`] and [`rb_zeo_rtypeddata`] alias. The cell they answer
//! lives inside the [`super::data::CData`], so `RTYPEDDATA(o)->data = p`
//! writes the object's ONE slot and cannot go stale. That is what date's
//! `d_lite_marshal_load` does after a `ruby_xrealloc`, and it is the reason
//! the cell moved into the object rather than being copied beside it -- two
//! slots would be two answers.
//!
//! Every other view is refilled, and lives until the scope pops. A pointer
//! held across a call back into Ruby therefore reads what the object looked
//! like at the reach. MRI gives the same warning about its own `RSTRING_PTR`,
//! whose buffer a `rb_str_cat` can move.
//!
//! [`super::io`] keeps its `struct rb_io` for longer, and that is deliberate
//! rather than inconsistent: MRI's `rb_io_t` IS the IO's own struct and lives
//! as long as the IO, so an extension holding an `fptr` across calls is doing
//! something MRI supports. Holding a `struct RString *` is not.

use super::layout as mri;
use super::value::Value;
use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_long, c_void};
use std::sync::atomic::{AtomicUsize, Ordering};
use zeo_rt::{RubyValue, Signal};

/// One view: the struct's type, the object's address, the boxed block.
type View = (TypeId, usize, Box<dyn Any>);

thread_local! {
    /// One refilled view per object per struct, alive until the scope pops.
    ///
    /// Keyed by [`TypeId`] as well as by the object, because an address a
    /// collected object freed can come back as a different kind. The blocks
    /// are boxed, so pushing another entry never moves one C already holds.
    static VIEWS: RefCell<Vec<View>> = const {
        RefCell::new(Vec::new())
    };
}

/// Drop every view. Called from the scope pop, with the string pins.
pub(super) fn flush_views() {
    VIEWS.with_borrow_mut(Vec::clear);
}

/// Store `value` as the view of `key`, replacing what the last reach left,
/// and answer the address C reads it at.
fn mint<T: Any>(key: usize, value: T) -> *mut T {
    VIEWS.with_borrow_mut(|views| {
        let id = TypeId::of::<T>();
        if let Some((_, _, slot)) = views.iter_mut().find(|(i, k, _)| *i == id && *k == key) {
            let held: &mut T = slot.downcast_mut().expect("one type per key");
            *held = value;
            return std::ptr::from_mut(held);
        }
        views.push((id, key, Box::new(value)));
        let (_, _, slot) = views.last_mut().expect("just pushed");
        std::ptr::from_mut(slot.downcast_mut::<T>().expect("just boxed"))
    })
}

/// The object's own `struct RBasic`, which is the first two words of its
/// handle. Copying it rather than zeroing it keeps a `RSTRING(v)->basic.flags`
/// read answering the truth: the tag, the freeze bit and the shape flags are
/// all really there.
pub(super) fn basic_of(v: Value) -> mri::RBasic {
    // SAFETY: `v` is a live heap `VALUE`, so it addresses a `Handle` whose
    // first two words ARE a `struct RBasic`. They are `AtomicUsize` there
    // because C writes `flags` with a plain read-modify-write; reading them
    // atomically is the Rust-legal half of that arrangement.
    let words = v as *const AtomicUsize;
    unsafe {
        mri::RBasic {
            flags: (*words).load(Ordering::Relaxed) as mri::VALUE,
            klass: (*words.add(1)).load(Ordering::Relaxed) as mri::VALUE,
        }
    }
}

/// The identity a view is keyed by: the payload address, which is what makes
/// two reaches for one object answer one pointer.
fn key_of(v: Value) -> Result<usize, Signal> {
    let val = unsafe { super::convert::value_of(v) };
    zeo_rt::value::weak_owner(&val)
        .map(|w| w.as_ptr() as *const () as usize)
        .ok_or_else(|| {
            zeo_rt::builtins::not_impl_error!(
                "this object has no stable address for a C view to key on"
            )
        })
}

crate::cext_fn! {
    /// `RSTRING(obj)`. `len` and `as.heap.ptr` come from the same pin
    /// `RSTRING_PTR` has always handed out, so a write through the bytes
    /// still reaches the String and is still written back at scope pop.
    ///
    /// zeo sets ::RSTRING_NOEMBED on a String handle, so upstream's own
    /// `RSTRING_PTR` and `RSTRING_END` take the `as.heap` arm this fills.
    fn rb_zeo_rstring(obj: Value) -> *mut mri::RString {
        let (ptr, len) = super::string::str_ptr_len(obj)?;
        Ok(mint(key_of(obj)?, mri::RString {
            basic: basic_of(obj),
            len,
            as_: mri::RStringPayload {
                heap: mri::RStringHeap {
                    ptr,
                    aux: mri::RStringAux { capa: len },
                },
            },
        }))
    }

    /// `RARRAY(obj)`. `as.heap.ptr` is the projection of tagged `VALUE`s
    /// `RARRAY_CONST_PTR` has always built -- a zeo Array element is a Rust
    /// enum, not a word, so there is no run of `VALUE`s inside the object to
    /// point at. A store through it stays in the projection, which is why
    /// `RARRAY_ASET` calls [`rb_zeo_ary_aset`] instead.
    ///
    /// zeo never sets ::RARRAY_EMBED_FLAG, so upstream's arms take
    /// `as.heap`.
    fn rb_zeo_rarray(obj: Value) -> *mut mri::RArray {
        let (ptr, len) = super::collection::ary_ptr_len(obj)?;
        Ok(mint(key_of(obj)?, mri::RArray {
            basic: basic_of(obj),
            as_: mri::RArrayPayload {
                heap: mri::RArrayHeap {
                    len,
                    aux: mri::RArrayAux { capa: len },
                    // zeo's `Value` and MRI's `VALUE` are both pointer-width
                    // words; the spellings differ (`usize` against
                    // `c_ulong`) and the cast is only that.
                    ptr: ptr.cast(),
                },
            },
        }))
    }

    /// `ROBJECT(obj)`. `as.heap.fields` names a materialized run of the
    /// object's instance variables, in `instance_variables` order, so
    /// upstream's `ROBJECT_FIELDS` reads them. zeo sets ::ROBJECT_HEAP so
    /// that arm is the one taken.
    ///
    /// A store through the run does not reach the object. Nothing in the
    /// 23-gem census calls `ROBJECT_FIELDS` at all.
    fn rb_zeo_robject(obj: Value) -> *mut mri::RObject {
        let fields = ivar_projection(obj)?;
        let key = key_of(obj)?;
        // The run outlives the view because it rides in the same block: the
        // C-visible struct comes first and the `Vec` behind it is Rust's.
        let block = mint(key, ObjectBlock {
            o: mri::RObject {
                basic: basic_of(obj),
                as_: mri::RObjectPayload {
                    heap: mri::RObjectHeap { fields: std::ptr::null_mut() },
                },
            },
            fields,
        });
        // SAFETY: `block` is the boxed view just minted, and nothing else
        // holds a reference to it.
        unsafe {
            (*block).o.as_.heap.fields = (*block).fields.as_mut_ptr();
            Ok(std::ptr::from_mut(&mut (*block).o))
        }
    }

    /// `RREGEXP(obj)`. `src` is the pattern String and `usecnt` is a real
    /// word in the view. `ptr` stays zero: ::RREGEXP_PTR raises rather than
    /// hand out the compiled pattern, which zeo's own engine owns and may
    /// recompile.
    fn rb_zeo_rregexp(obj: Value) -> *mut mri::RRegexp {
        let re = unsafe { super::convert::value_of(obj) };
        let src = super::convert::to_value(&super::object::send(&re, "source", &[])?)?;
        let key = key_of(obj)?;
        // `usecnt` is the one field an extension increments, so it must
        // survive the refill rather than be reset by it.
        let usecnt = VIEWS.with_borrow(|views| {
            let id = TypeId::of::<mri::RRegexp>();
            views
                .iter()
                .find(|(i, k, _)| *i == id && *k == key)
                .and_then(|(_, _, s)| s.downcast_ref::<mri::RRegexp>())
                .map_or(0, |r| r.usecnt)
        });
        Ok(mint(key, mri::RRegexp {
            basic: basic_of(obj),
            ptr: std::ptr::null_mut(),
            src: src as mri::VALUE,
            usecnt,
        }))
    }

    /// `RMATCH(obj)`. The block carries the `struct RMatch` and, right
    /// behind it, the `rb_matchext_t` that ::RMATCH_EXT walks onto -- so
    /// upstream's `RMATCH_REGS` reads real registers, filled from the
    /// MatchData's own group offsets.
    fn rb_zeo_rmatch(obj: Value) -> *mut mri::RMatch {
        let m = unsafe { super::convert::value_of(obj) };
        let str_v = super::convert::to_value(&super::object::send(&m, "string", &[])?)?;
        let re_v = super::convert::to_value(&super::object::send(&m, "regexp", &[])?)?;
        let (beg, end) = group_offsets(&m)?;
        let key = key_of(obj)?;
        let block = mint(key, MatchBlock {
            m: mri::RMatch {
                basic: basic_of(obj),
                str_: str_v as mri::VALUE,
                regexp: re_v as mri::VALUE,
            },
            // SAFETY: `rb_matchext_t` is plain data, and the members this
            // does not go on to set -- `char_offset`, and the capture
            // history root where onigmo carries one -- are pointers and
            // counts a zero leaves correctly unset.
            ext: unsafe { std::mem::zeroed() },
            beg,
            end,
        });
        // The registers point INTO the block, so they can only be filled
        // once the block sits at its final address.
        //
        // SAFETY: `block` is the boxed view just minted, and nothing else
        // holds a reference to it.
        unsafe {
            let n = (*block).beg.len() as c_int;
            (*block).ext.regs.allocated = n;
            (*block).ext.regs.num_regs = n;
            (*block).ext.regs.beg = (*block).beg.as_mut_ptr();
            (*block).ext.regs.end = (*block).end.as_mut_ptr();
            Ok(std::ptr::from_mut(&mut (*block).m))
        }
    }

    /// `RDATA(obj)`: the object's own cell, read as the untyped shape. MRI
    /// puts `data` at the same offset in both structs, which is why one cell
    /// serves both.
    fn rb_zeo_rdata(obj: Value) -> *mut mri::RData {
        Ok(data_cell(obj)?.cast())
    }

    /// `RTYPEDDATA(obj)`: the object's own cell. `data` is the object's ONE
    /// slot, so a store through it lands in the object.
    fn rb_zeo_rtypeddata(obj: Value) -> *mut mri::RTypedData {
        data_cell(obj)
    }

    /// `RARRAY_ASET`, which has to reach the Array rather than the
    /// projection `RARRAY_PTR_USE` hands out.
    fn rb_zeo_ary_aset(ary: Value, i: c_long, v: Value) -> () {
        let a = unsafe { super::convert::value_of(ary) };
        let item = unsafe { super::convert::value_of(v) };
        super::object::send(&a, "[]=", &[RubyValue::Int(i as i64), item])?;
        Ok(())
    }

    /// The loud floor: a field zeo will not hand out. The return type only
    /// satisfies the caller, which never gets one.
    fn rb_zeo_no_field(what: *const c_char) -> *mut c_void {
        let name = unsafe { super::object::cstr(what) };
        Err(zeo_rt::builtins::not_impl_error!(
            "{name} reaches a field zeo does not hand out; use the object's own methods"
        ))
    }
}

/// `struct RObject` plus the run of `VALUE`s its `as.heap.fields` points at.
/// `#[repr(C)]` and this field order are load-bearing only for `o`, which is
/// what C sees; `fields` is Rust's and sits behind it.
#[repr(C)]
struct ObjectBlock {
    o: mri::RObject,
    fields: Vec<mri::VALUE>,
}

/// `struct RMatch` and, contiguously, the `rb_matchext_t` that
/// `RMATCH_EXT(m)` reaches by adding `sizeof(struct RMatch)`. The two must
/// stay adjacent and in this order.
#[repr(C)]
struct MatchBlock {
    m: mri::RMatch,
    ext: mri::rb_matchext_struct,
    beg: Vec<mri::OnigPosition>,
    end: Vec<mri::OnigPosition>,
}

/// The `CData` cell for a `T_DATA`, or the stand-in for one of the values
/// that crosses as `T_DATA` without being a `CData`.
fn data_cell(obj: Value) -> Result<*mut mri::RTypedData, Signal> {
    let val = unsafe { super::convert::value_of(obj) };
    let RubyValue::Object(o) = &val else {
        return Err(opaque_cell(obj));
    };
    let Some(d) = o.as_any().downcast_ref::<super::data::CData>() else {
        return Err(opaque_cell(obj));
    };
    Ok(d.refill_cell(basic_of(obj)))
}

/// `Proc`, `Thread`, `Mutex`, `Queue`, `Fiber` and `Enumerator` all cross as
/// `T_DATA`, and none of them is a `CData` with a C struct behind it.
///
/// The refusal uses MRI'S OWN mechanism rather than a bespoke one: the view
/// names a `rb_data_type_t` that matches nothing, so `TypedData_Get_Struct`
/// raises the `TypeError` a type mismatch always raises. An extension that
/// reads `->data` past that gets NULL, which is also what MRI hands an
/// extension reaching into an object whose internals it was never given.
fn opaque_cell(_obj: Value) -> Signal {
    zeo_rt::builtins::not_impl_error!(
        "this object crosses as T_DATA but carries no C struct; \
         TypedData_Get_Struct on it raises, as a type mismatch does on MRI"
    )
}

/// The object's instance variables as a run of `VALUE`s, in
/// `instance_variables` order.
fn ivar_projection(obj: Value) -> Result<Vec<mri::VALUE>, Signal> {
    let recv = unsafe { super::convert::value_of(obj) };
    let names = match super::object::send(&recv, "instance_variables", &[])? {
        RubyValue::Array(a) => a.lock().to_vec(),
        _ => Vec::new(),
    };
    let mut out = Vec::with_capacity(names.len());
    for name in &names {
        let got = super::object::send(&recv, "instance_variable_get", std::slice::from_ref(name))?;
        out.push(super::convert::to_value(&got)? as mri::VALUE);
    }
    Ok(out)
}

/// Group begin and end offsets, in bytes, group 0 first -- which is onig's
/// own register order. A group that did not participate is `-1` in both,
/// which is `ONIG_REGION_NOTPOS`.
fn group_offsets(
    m: &RubyValue,
) -> Result<(Vec<mri::OnigPosition>, Vec<mri::OnigPosition>), Signal> {
    let n = match super::object::send(m, "size", &[])? {
        RubyValue::Int(n) => n.max(0) as usize,
        _ => 1,
    };
    let mut beg = Vec::with_capacity(n);
    let mut end = Vec::with_capacity(n);
    for i in 0..n {
        let at = RubyValue::Int(i as i64);
        let b = super::object::send(m, "begin", std::slice::from_ref(&at))?;
        let e = super::object::send(m, "end", &[at])?;
        beg.push(offset_or_notpos(&b));
        end.push(offset_or_notpos(&e));
    }
    Ok((beg, end))
}

fn offset_or_notpos(v: &RubyValue) -> mri::OnigPosition {
    match v {
        RubyValue::Int(n) => *n as mri::OnigPosition,
        _ => -1,
    }
}

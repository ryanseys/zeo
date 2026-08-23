//! `T_DATA`: a C struct wearing a Ruby object.
//!
//! `TypedData_Make_Struct(klass, struct foo, &foo_type, p)` asks for a Ruby
//! object of class `klass` that carries a `void *` and a descriptor. In zeo
//! that object is a [`CData`], an ordinary [`RubyObject`] -- which is the
//! whole reason the cycle collector needs no new node kind for it. It is
//! already `Node::Object`, and its `gc_visit` is already the hook.
//!
//! # The `dmark` <-> `gc_visit` agreement
//!
//! A `dmark` is an edge-list callback: it is handed the C struct and calls
//! `rb_gc_mark` once per `VALUE` the struct holds. That is exactly the
//! outgoing-edge enumerator [`RubyObject::gc_visit`] already is, so the two
//! marry directly: [`CData::gc_visit`] installs a sink, calls `dmark`, and
//! every `rb_gc_mark` inside lands in it.
//!
//! They marry on the WALK only. `gc_visit(take = true)` is the sweep, and it
//! must MOVE each reference out, leaving the slot nil -- a `dmark` cannot do
//! that. It reads a C struct zeo has never seen and has no way to clear a
//! field in it.
//!
//! The asymmetry rule decides what to do about that, and it is the rule the
//! whole collector rests on: an omitted edge only leaks, while a reported one
//! that cannot be released over-subtracts and can clear a live object. So the
//! sweep arm is empty. A cycle closed through a C extension's own struct
//! leaks, exactly as a cycle through a `Proc`'s captures does, and for the
//! same reason.
//!
//! What the walk arm still buys: a cycle that merely PASSES THROUGH a
//! TypedData object -- Ruby object -> C struct -> Ruby object -> back -- is
//! seen, counted, and reclaimed at the Ruby links. Only the C link itself is
//! unclearable.

use crate::RubyValue;
use crate::dispatch::{ClassId, RObj, RubyObject};
use std::any::Any;
use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// `RUBY_DATA_FUNC`.
pub type DataFunc = Option<unsafe extern "C" fn(*mut c_void)>;

/// MRI's `struct rb_data_type_struct`. Field order is the vendored header's
/// and is read by C, so it is `#[repr(C)]` and must not be reordered.
#[repr(C)]
pub struct DataTypeFns {
    pub dmark: DataFunc,
    pub dfree: DataFunc,
    pub dsize: Option<unsafe extern "C" fn(*const c_void) -> usize>,
    pub dcompact: DataFunc,
    pub reserved: [*mut c_void; 1],
}

#[repr(C)]
pub struct DataType {
    pub wrap_struct_name: *const std::ffi::c_char,
    pub function: DataTypeFns,
    pub parent: *const DataType,
    pub data: *mut c_void,
    pub flags: usize,
}

/// A Ruby object whose state is a C struct.
///
/// The `data` pointer is an `AtomicUsize` rather than a plain field because
/// `DATA_PTR(obj) = p` is a real idiom: C assigns through the slot, and
/// [`Self::slot`] is what it assigns through.
pub struct CData {
    class: ClassId,
    data: AtomicUsize,
    /// Null for the untyped `Data_Wrap_Struct` form, which carries its own
    /// `dmark`/`dfree` instead.
    dtype: *const DataType,
    /// The untyped form's two functions. MRI deprecates that API and only one
    /// gem in the census still reaches for it, but a deprecated API that
    /// silently does nothing is worse than one that works.
    untyped: (DataFunc, DataFunc),
    frozen: AtomicBool,
}

// SAFETY: `dtype` points at a `static const rb_data_type_t` in the
// extension's own image, which outlives the process and is never written.
// `data` is reached only under the GVL, which loading an extension arms.
unsafe impl Send for CData {}
unsafe impl Sync for CData {}

impl CData {
    /// The `TypedData_Wrap_Struct` form.
    pub fn typed(class: ClassId, data: *mut c_void, dtype: *const DataType) -> RObj {
        Self::build(CData {
            class,
            data: AtomicUsize::new(data as usize),
            dtype,
            untyped: (None, None),
            frozen: AtomicBool::new(false),
        })
    }

    /// The untyped `Data_Wrap_Struct` form.
    pub fn untyped(class: ClassId, data: *mut c_void, mark: DataFunc, free: DataFunc) -> RObj {
        Self::build(CData {
            class,
            data: AtomicUsize::new(data as usize),
            dtype: std::ptr::null(),
            untyped: (mark, free),
            frozen: AtomicBool::new(false),
        })
    }

    fn build(d: CData) -> RObj {
        let o: RObj = Arc::new(d);
        // Same registration every other object constructor does. Without it
        // `ObjectSpace.each_object` would not see a C extension's objects and
        // the collector would read every one of them as live.
        crate::gc::record_object(&o);
        o
    }

    /// The slot `DATA_PTR` and `RTYPEDDATA_DATA` hand back. Valid while this
    /// object lives, which the handle holding it guarantees.
    pub fn slot(&self) -> *mut *mut c_void {
        std::ptr::from_ref(&self.data).cast_mut().cast()
    }

    pub fn data_type(&self) -> *const DataType {
        self.dtype
    }

    pub fn is_typed(&self) -> bool {
        !self.dtype.is_null()
    }

    fn mark_fn(&self) -> DataFunc {
        if self.dtype.is_null() {
            self.untyped.0
        } else {
            // SAFETY: see the `unsafe impl Send` note -- the descriptor is a
            // static in the extension's image.
            unsafe { (*self.dtype).function.dmark }
        }
    }

    fn free_fn(&self) -> DataFunc {
        if self.dtype.is_null() {
            self.untyped.1
        } else {
            unsafe { (*self.dtype).function.dfree }
        }
    }
}

impl Drop for CData {
    /// `dfree` owns the C struct, so this is where it is released.
    ///
    /// MRI's `RUBY_TYPED_FREE_IMMEDIATELY` asks for exactly this timing, and
    /// zeo gives it to every TypedData object: a refcount drop IS immediate,
    /// and there is no sweep phase to defer to.
    fn drop(&mut self) {
        let p = self.data.swap(0, Ordering::Relaxed) as *mut c_void;
        if p.is_null() {
            return;
        }
        if let Some(free) = self.free_fn() {
            // `RUBY_DEFAULT_FREE` is spelled as the function pointer -1, not
            // as a real function; it means "this was ruby_xmalloc'd".
            if free as usize == usize::MAX {
                unsafe { crate::cext::alloc::xfree(p) };
            } else {
                unsafe { free(p) };
            }
        }
    }
}

thread_local! {
    /// Where `rb_gc_mark` puts an edge while a walk is running.
    ///
    /// `None` means no walk is in progress, and `rb_gc_mark` is then a no-op
    /// -- which is what it is in MRI outside a GC too. An extension is free
    /// to call it whenever it likes.
    static MARK_SINK: RefCell<Option<Vec<RubyValue>>> = const { RefCell::new(None) };
}

/// `rb_gc_mark`'s Rust half: record one edge, if anything is listening.
/// Does `child`'s `parent` chain reach `parent`? A subclass's descriptor
/// names its parent's, and a plain pointer comparison would answer no.
pub fn type_inherits(child: *const DataType, parent: *const DataType) -> bool {
    let mut at = child;
    while !at.is_null() {
        if std::ptr::eq(at, parent) {
            return true;
        }
        // SAFETY: a descriptor is a static in the extension's own image.
        at = unsafe { (*at).parent };
    }
    false
}

/// Is `v` a TypedData whose descriptor inherits from `ty`?
///
/// # Safety
///
/// `v` must be a live `VALUE`.
pub unsafe fn is_kind_of(v: super::value::Value, ty: *const DataType) -> bool {
    let val = unsafe { super::convert::value_of(v) };
    let RubyValue::Object(o) = &val else {
        return false;
    };
    o.as_any()
        .downcast_ref::<CData>()
        .is_some_and(|d| type_inherits(d.data_type(), ty))
}

pub fn mark_edge(v: RubyValue) {
    MARK_SINK.with_borrow_mut(|s| {
        if let Some(sink) = s.as_mut() {
            sink.push(v);
        }
    });
}

/// Whether a walk is in progress. `rb_gc_mark` reads it to skip the
/// conversion work when nothing is listening.
pub fn marking() -> bool {
    MARK_SINK.with_borrow(Option::is_some)
}

impl RubyObject for CData {
    fn class_id(&self) -> ClassId {
        self.class
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_rc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }

    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }

    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Relaxed);
    }

    /// The walk runs `dmark`; the sweep does nothing. See this module's docs
    /// for why the asymmetry is the safe direction rather than a shortcut.
    fn gc_visit(&self, out: &mut Vec<RubyValue>, take: bool) {
        if take {
            return;
        }
        let Some(mark) = self.mark_fn() else {
            return;
        };
        let data = self.data.load(Ordering::Relaxed) as *mut c_void;
        if data.is_null() || mark as usize == usize::MAX {
            return;
        }
        // A nested walk would splice one object's edges into another's, and
        // over-reporting is the direction that clears a live object. A
        // `dmark` that re-enters is a bug in the extension; refusing to
        // recurse is how zeo declines to make it a memory-safety bug.
        if marking() {
            return;
        }
        MARK_SINK.with_borrow_mut(|s| *s = Some(Vec::new()));
        // SAFETY: `mark` came from the descriptor the extension registered
        // and `data` from the same object's slot. This is the one call the
        // extension named for this purpose.
        unsafe { mark(data) };
        let edges = MARK_SINK.with_borrow_mut(Option::take).unwrap_or_default();
        out.extend(edges);
    }

    /// A shallow copy would alias the C struct, and then `dfree` would run
    /// twice on one pointer. So the copy carries a NULL slot and no free
    /// function: `DATA_PTR` on it is null, which is what a TypedData object
    /// looks like between `rb_obj_alloc` and `initialize_copy`.
    ///
    /// CRuby differs: `rb_obj_dup` calls the class's own allocator, so the
    /// copy gets a fresh zeroed struct. zeo cannot yet, because the
    /// allocator table arrives with `rb_define_alloc_func`. Recorded under
    /// "Known divergences" in `crates/zeo-rt/cext/README.md`; it becomes a
    /// real gap file once a gem can load and the shape is runnable.
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        Arc::new(CData {
            class: self.class,
            data: AtomicUsize::new(0),
            dtype: self.dtype,
            untyped: (self.untyped.0, None),
            frozen: AtomicBool::new(copy_frozen && self.is_frozen()),
        })
    }

    fn ivar_values(&self) -> Vec<RubyValue> {
        // A C struct's fields are not ivars. `dmark` reports the references;
        // `instance_variables` on a TypedData object is empty in MRI too.
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    /// A `rb_data_type_t` lives in the EXTENSION's image, as a C static, so
    /// nothing in the runtime ever declares one. A test has to, and Rust
    /// wants a `Sync` for that -- the raw pointers inside are what block it.
    struct Descriptor(DataType);
    // SAFETY: read-only for the whole test, as the real ones are for the
    // whole process.
    unsafe impl Sync for Descriptor {}

    static mut MARKED: usize = 0;

    unsafe extern "C" fn count_marks(_p: *mut c_void) {
        // Two edges, so the test can tell "ran" from "ran once".
        super::mark_edge(RubyValue::Int(1));
        super::mark_edge(RubyValue::Int(2));
        unsafe { MARKED += 1 };
    }

    unsafe extern "C" fn no_free(_p: *mut c_void) {}

    static COUNTING: Descriptor = Descriptor(DataType {
        wrap_struct_name: c"zeo/test/counting".as_ptr(),
        function: DataTypeFns {
            dmark: Some(count_marks),
            dfree: Some(no_free),
            dsize: None,
            dcompact: None,
            reserved: [std::ptr::null_mut()],
        },
        parent: std::ptr::null(),
        data: std::ptr::null_mut(),
        flags: 0,
    });

    fn one() -> RObj {
        CData::typed(
            zeo_abi::OBJECT_CLASS,
            std::ptr::dangling_mut(),
            &raw const COUNTING.0,
        )
    }

    #[test]
    fn the_walk_runs_dmark_and_the_sweep_does_not() {
        let o = one();
        let mut out = Vec::new();
        o.gc_visit(&mut out, false);
        assert_eq!(out.len(), 2, "dmark's edges did not reach the walk");

        let mut swept = Vec::new();
        o.gc_visit(&mut swept, true);
        assert!(
            swept.is_empty(),
            "the sweep reported an edge it cannot release"
        );
    }

    #[test]
    fn a_reentrant_dmark_reports_nothing_rather_than_splicing() {
        let o = one();
        let mut out = Vec::new();
        MARK_SINK.with_borrow_mut(|s| *s = Some(Vec::new()));
        o.gc_visit(&mut out, false);
        let outer = MARK_SINK.with_borrow_mut(Option::take).unwrap_or_default();
        assert!(out.is_empty(), "a nested walk reported edges");
        assert!(outer.is_empty(), "a nested walk wrote into the outer sink");
    }

    #[test]
    fn a_mark_outside_a_walk_is_a_no_op() {
        assert!(!marking());
        mark_edge(RubyValue::Int(9));
        let mut out = Vec::new();
        one().gc_visit(&mut out, false);
        assert_eq!(out.len(), 2, "a stray mark leaked into the next walk");
    }

    #[test]
    fn dfree_runs_when_the_last_reference_goes() {
        static FREED: AtomicUsize = AtomicUsize::new(0);
        unsafe extern "C" fn counting_free(_p: *mut c_void) {
            FREED.fetch_add(1, Ordering::Relaxed);
        }
        static FREEING: Descriptor = Descriptor(DataType {
            wrap_struct_name: c"zeo/test/freeing".as_ptr(),
            function: DataTypeFns {
                dmark: None,
                dfree: Some(counting_free),
                dsize: None,
                dcompact: None,
                reserved: [std::ptr::null_mut()],
            },
            parent: std::ptr::null(),
            data: std::ptr::null_mut(),
            flags: 0,
        });
        let o = CData::typed(
            zeo_abi::OBJECT_CLASS,
            std::ptr::dangling_mut(),
            &raw const FREEING.0,
        );
        assert_eq!(FREED.load(Ordering::Relaxed), 0);
        drop(o);
        assert_eq!(FREED.load(Ordering::Relaxed), 1);
    }
}

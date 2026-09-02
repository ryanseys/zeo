//! The slots the C-API crate (`zeo-capi`) fills.
//!
//! The runtime reaches the C API at four points: loading an extension,
//! asking whether a class allocates through `rb_define_alloc_func`, running
//! that allocator, and the `ruby_vm_at_exit` callbacks at shutdown. None of
//! them can be a direct call, because `zeo-capi` depends on this crate and
//! a build may leave it out altogether. So they are function pointers in a
//! [`linkme`] distributed slice: the C-API crate contributes one element and
//! nothing has to call an installer, which is what lets an AOT binary that
//! links `libzeo.a` find it without a line of startup code.
//!
//! `.new` sits in front of every one of these questions, so the first thing
//! it asks is one relaxed load ([`any_alloc_func`]): a program that never
//! registered a C allocator never reads the slice.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::dispatch::ClassId;
use crate::{RubyValue, Signal};

/// What the C-API crate answers for the runtime.
pub struct CapiHooks {
    /// `dlopen` the extension at `path` and run its `Init_<init>`; `false`
    /// when that path was already loaded.
    pub load: fn(path: &str, init: &str) -> Result<bool, Signal>,
    /// Whether the class allocates through a registered C allocator (an
    /// `rb_undef_alloc_func` counts: its `.new` must reach the C road to
    /// raise).
    pub has_alloc_func: fn(ClassId) -> bool,
    /// Run the class's C allocator. `None` with a parked signal is a raise;
    /// `None` with nothing parked means the class has no C allocator.
    pub c_allocate: fn(ClassId) -> Option<RubyValue>,
    /// The `ruby_vm_at_exit` callbacks, once, at shutdown.
    pub run_vm_at_exit: fn(),
}

/// Zero or one element: the C-API crate's, when it is linked.
#[linkme::distributed_slice]
pub static CAPI_HOOKS: [CapiHooks];

/// The C API, if this binary carries one.
pub fn hooks() -> Option<&'static CapiHooks> {
    CAPI_HOOKS.first()
}

/// Set once, by the C-API crate, the first time any extension registers an
/// allocator. Never cleared: `rb_undef_alloc_func` is a registration too.
static ANY_ALLOC_FUNC: AtomicBool = AtomicBool::new(false);

/// The fast path's whole cost: one relaxed load.
#[inline]
pub fn any_alloc_func() -> bool {
    ANY_ALLOC_FUNC.load(Ordering::Relaxed)
}

/// An extension registered (or undefined) an allocator.
pub fn note_alloc_func() {
    ANY_ALLOC_FUNC.store(true, Ordering::Relaxed);
}

/// Whether `id` allocates through a C allocator, WITHOUT running it.
#[inline]
pub fn has_alloc_func(id: ClassId) -> bool {
    any_alloc_func() && hooks().is_some_and(|h| (h.has_alloc_func)(id))
}

/// Run `id`'s C allocator, if it has one -- see [`CapiHooks::c_allocate`]
/// for what the two `None`s mean.
#[inline]
pub fn c_allocate(id: ClassId) -> Option<RubyValue> {
    if !any_alloc_func() {
        return None;
    }
    (hooks()?.c_allocate)(id)
}

/// Load a compiled extension, or explain why this binary cannot.
pub fn load(path: &str, init: &str) -> Result<bool, Signal> {
    match hooks() {
        Some(h) => (h.load)(path, init),
        None => Err(crate::builtins::load_error!(
            "cannot load such file -- {path}: this zeo was built without C extension support"
        )),
    }
}

/// The shutdown half, a no-op without a C API.
pub fn run_vm_at_exit() {
    if let Some(h) = hooks() {
        (h.run_vm_at_exit)();
    }
}

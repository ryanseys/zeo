//! The generic user-object the Cranelift backend allocates: where the rustc
//! backend monomorphizes one Rust struct per Ruby class (`ruby_class!` +
//! `IvarCell<N>`), CLIF programs share ONE concrete type whose per-class
//! shape lives in a [`ClassLayout`] row -- ivar names in slot order plus the
//! hidden `Struct`/`Data` member count -- installed into [`LAYOUTS`] by
//! `register_program` (or [`register_layout`] directly, until M0-7 lands
//! it). Ivar storage is [`IvarSlots`], the size-erased twin of `IvarCell`,
//! so the lock discipline and sole-thread fast path are the same
//! implementation.

use crate::RubyValue;
use crate::dispatch::{RObj, RubyObject};
use crate::ivars::IvarSlots;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use zeo_abi::ClassId;

/// One compiled class's object shape. `'static` because layouts live in
/// the program's `.rodata` (or are leaked once at registration) -- an
/// object holds a plain reference, never a count.
pub struct ClassLayout {
    /// Declared ivar names WITHOUT the `@`, in slot order (the compiler's
    /// parent-first slot layout).
    pub names: &'static [&'static str],
    /// How many hidden `Struct`/`Data` member slots follow the named ones.
    pub hidden: usize,
}

impl ClassLayout {
    /// Total slot count: named ivars then hidden members.
    fn slots(&self) -> usize {
        self.names.len() + self.hidden
    }
}

/// The per-class layout table `zeo_rt_object_alloc` reads. Write-once-ish:
/// `register_program` fills it before the toplevel runs; the lock is
/// uncontended forever after.
static LAYOUTS: std::sync::RwLock<Option<crate::FMap<u32, &'static ClassLayout>>> =
    std::sync::RwLock::new(None);

/// Install one class's layout -- `register_program`'s per-`ClassDesc` step.
pub fn register_layout(id: ClassId, layout: &'static ClassLayout) {
    LAYOUTS
        .write()
        .unwrap()
        .get_or_insert_with(crate::FMap::default)
        .insert(id.0, layout);
}

/// The layout for `id`, or `None` for a class no `ClassDesc` declared
/// (a builtin, or a rustc-backend class -- neither allocates through here).
pub fn layout_of(id: ClassId) -> Option<&'static ClassLayout> {
    LAYOUTS.read().unwrap().as_ref()?.get(&id.0).copied()
}

/// The layout an instance of `id` is ALLOCATED with: its own, or -- for a
/// class minted at run time (`Class.new(Compiled)`) -- the nearest
/// compiled ancestor's. That is the rustc twin: a runtime subclass
/// inherits its parent's `__allocate`, which builds the PARENT's struct
/// and stamps the subclass's id.
pub fn alloc_layout_of(id: ClassId) -> Option<&'static ClassLayout> {
    if let Some(l) = layout_of(id) {
        return Some(l);
    }
    crate::dispatch::ancestors_of_value(id)
        .iter()
        .copied()
        .find_map(layout_of)
}

/// A compiled class's instance. The class word is atomic for the same
/// reason `ruby_class!` structs carry one: a `Ractor` move retags the husk
/// in place.
pub struct CompiledObject {
    class_id: AtomicU32,
    frozen: AtomicBool,
    layout: &'static ClassLayout,
    ivars: IvarSlots,
}

impl CompiledObject {
    /// A fresh instance of `id`: every slot `Nil`/unassigned, unfrozen.
    pub fn alloc(id: ClassId, layout: &'static ClassLayout) -> RObj {
        let o: RObj = std::sync::Arc::new(CompiledObject {
            class_id: AtomicU32::new(id.0),
            frozen: AtomicBool::new(false),
            layout,
            ivars: IvarSlots::with_len(layout.slots()),
        });
        crate::gc::record_object(&o);
        o
    }
}

impl RubyObject for CompiledObject {
    fn class_id(&self) -> ClassId {
        ClassId(self.class_id.load(Ordering::Relaxed))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_rc(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn std::any::Any + Send + Sync> {
        self
    }

    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }

    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Relaxed);
    }

    fn gc_visit(&self, out: &mut Vec<RubyValue>, take: bool) {
        self.ivars.gc_visit(out, take);
    }

    fn ivar_values(&self) -> Vec<RubyValue> {
        self.ivars.values(self.layout.names.len())
    }

    fn ivar_pairs(&self) -> Vec<(String, RubyValue)> {
        self.ivars.pairs(self.layout.names)
    }

    fn ivar_get_named(&self, name: &str) -> Option<RubyValue> {
        self.ivars.get_named(self.layout.names, name)
    }

    fn ivar_set_named(&self, name: &str, v: RubyValue) -> bool {
        self.ivars.set_named(self.layout.names, name, v);
        true
    }

    fn ivar_remove_named(&self, name: &str) -> Option<RubyValue> {
        self.ivars.remove_named(self.layout.names, name)
    }

    fn hidden_ivar_get(&self, i: usize) -> Option<RubyValue> {
        (i < self.layout.hidden).then(|| self.ivars.get(self.layout.names.len() + i))
    }

    fn hidden_ivar_set(&self, i: usize, v: RubyValue) -> bool {
        if i >= self.layout.hidden {
            return false;
        }
        self.ivars.set(self.layout.names.len() + i, v);
        true
    }

    fn ivar_slot_get(&self, slot: usize) -> RubyValue {
        if slot >= self.layout.slots() {
            return RubyValue::Nil;
        }
        self.ivars.get(slot)
    }

    fn ivar_slot_set(&self, slot: usize, value: RubyValue) {
        if slot < self.layout.slots() {
            self.ivars.set(slot, value);
        }
    }

    fn take_linked_ivars(&self, out: &mut Vec<RubyValue>) {
        self.ivars.take_linked(out);
    }

    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let copy: RObj = std::sync::Arc::new(CompiledObject {
            class_id: AtomicU32::new(self.class_id.load(Ordering::Relaxed)),
            frozen: AtomicBool::new(copy_frozen && self.is_frozen()),
            layout: self.layout,
            ivars: self.ivars.duplicate(),
        });
        crate::gc::record_object(&copy);
        copy
    }

    fn retag_moved(&self) -> bool {
        self.class_id
            .store(zeo_abi::RACTOR_MOVED_OBJECT_CLASS.0, Ordering::Relaxed);
        true
    }
}

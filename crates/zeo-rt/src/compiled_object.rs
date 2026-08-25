//! The generic user-object the Cranelift backend allocates: where the rustc
//! backend monomorphizes one Rust struct per Ruby class (`ruby_class!` +
//! `IvarCell<N>`), CLIF programs share ONE concrete type whose per-class
//! shape lives in a [`ClassLayout`] row -- ivar names in slot order plus the
//! hidden `Struct`/`Data` member count -- installed into [`LAYOUTS`] by
//! `register_program` (or [`register_layout`] directly, in unit tests).
//! Ivar storage comes in two shapes over one implementation (same lock
//! discipline, same sole-thread fast path): layouts of 8 slots or fewer
//! ride an inline `IvarCell<N>` in the `Arc` block, and only wider
//! layouts pay [`IvarSlots`], the size-erased boxed-slice twin.

use crate::RubyValue;
use crate::dispatch::{RObj, RubyObject};
use crate::ivars::{IvarCell, IvarSlots};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use zeo_abi::ClassId;

/// One compiled class's object shape. `'static` because layouts live in
/// the program's `.rodata` (or are leaked once at registration) -- an
/// object holds a plain reference, never a count.
pub struct ClassLayout {
    /// Declared ivar names WITHOUT the `@`, in slot order (the compiler's
    /// parent-first slot layout). They start at slot [`hidden`].
    pub names: &'static [&'static str],
    /// How many hidden `Struct`/`Data` member slots come FIRST, before the
    /// named ones.
    ///
    /// First, not last, and that is the whole point: a subclass of a compiled
    /// struct inherits the member list, and putting the members after the
    /// named ivars moved them whenever the subclass declared one of its own.
    /// `Sub < Struct.new(:example)` with an `@p` then had `@p` where its
    /// ancestor's compiled `initialize` writes the member, so `super` wrote
    /// the member's value into `@p` and the member read back nil.
    pub hidden: usize,
}

impl ClassLayout {
    /// Total slot count: hidden members then named ivars.
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

/// The small-layout twin: the same object with its ivar storage INLINE in
/// the `Arc` allocation (`IvarCell<N>`), so `.new` is one malloc instead of
/// three. `N` is a ladder rung, not the exact slot count -- the factory
/// rounds up, every access bound-checks against the LAYOUT, and a trailing
/// never-assigned `Nil` costs nothing in `gc_visit`/`Drop`.
struct CompiledObjN<const N: usize> {
    class_id: AtomicU32,
    frozen: AtomicBool,
    layout: &'static ClassLayout,
    ivars: IvarCell<N>,
}

impl CompiledObject {
    /// A fresh instance of `id`: every slot `Nil`/unassigned, unfrozen.
    /// Small layouts get an inline-storage [`CompiledObjN`]; only a class
    /// with more than 8 slots pays the size-erased boxed-slice shape.
    pub fn alloc(id: ClassId, layout: &'static ClassLayout) -> RObj {
        fn small<const N: usize>(id: ClassId, layout: &'static ClassLayout) -> RObj {
            let o: RObj = std::sync::Arc::new(CompiledObjN::<N> {
                class_id: AtomicU32::new(id.0),
                frozen: AtomicBool::new(false),
                layout,
                ivars: IvarCell::new(),
            });
            crate::gc::record_object(&o);
            o
        }
        match layout.slots() {
            0 => small::<0>(id, layout),
            1 => small::<1>(id, layout),
            2 => small::<2>(id, layout),
            3 => small::<3>(id, layout),
            4 => small::<4>(id, layout),
            5 | 6 => small::<6>(id, layout),
            7 | 8 => small::<8>(id, layout),
            n => {
                let o: RObj = std::sync::Arc::new(CompiledObject {
                    class_id: AtomicU32::new(id.0),
                    frozen: AtomicBool::new(false),
                    layout,
                    ivars: IvarSlots::with_len(n),
                });
                crate::gc::record_object(&o);
                o
            }
        }
    }
}

/// One `RubyObject` body for both storage shapes -- the two structs are
/// field-for-field identical and every `ivars` method is shared through
/// `IvarCellCore`, so the impls must not be allowed to drift.
macro_rules! compiled_object_impl {
    ($({$($gen:tt)*})? $t:ty) => {
        impl$(<$($gen)*>)? RubyObject for $t {
            fn class_id(&self) -> ClassId {
                ClassId(self.class_id.load(Ordering::Relaxed))
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            fn as_any_rc(
                self: std::sync::Arc<Self>,
            ) -> std::sync::Arc<dyn std::any::Any + Send + Sync> {
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
                self.ivars
                    .values(self.layout.hidden, self.layout.names.len())
            }

            fn ivar_pairs(&self) -> Vec<(String, RubyValue)> {
                self.ivars.pairs(self.layout.hidden, self.layout.names)
            }

            fn ivar_get_named(&self, name: &str) -> Option<RubyValue> {
                self.ivars
                    .get_named(self.layout.hidden, self.layout.names, name)
            }

            fn ivar_set_named(&self, name: &str, v: RubyValue) -> bool {
                self.ivars
                    .set_named(self.layout.hidden, self.layout.names, name, v);
                true
            }

            fn ivar_remove_named(&self, name: &str) -> Option<RubyValue> {
                self.ivars
                    .remove_named(self.layout.hidden, self.layout.names, name)
            }

            fn hidden_ivar_get(&self, i: usize) -> Option<RubyValue> {
                (i < self.layout.hidden).then(|| self.ivars.get(i))
            }

            fn hidden_ivar_set(&self, i: usize, v: RubyValue) -> bool {
                if i >= self.layout.hidden {
                    return false;
                }
                self.ivars.set(i, v);
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
                let copy: RObj = std::sync::Arc::new(Self {
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
    };
}

compiled_object_impl!(CompiledObject);
compiled_object_impl!({const N: usize} CompiledObjN<N>);

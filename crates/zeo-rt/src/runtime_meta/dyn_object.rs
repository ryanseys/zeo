//! DynObject: the instance type of a runtime (`Class.new`) class.

use super::*;

/// The generic constructor every runtime class registers: allocate a
/// name-keyed object tagged with the class id, then run the class's own
/// `initialize` if it defines one (mirroring `Class#new`). No user
/// `initialize` + extra args is CRuby's `ArgumentError`.
/// `Class.new(CompiledBase)`'s constructor: allocate through the nearest
/// compiled ancestor's `AllocatorFn`, stamped with the RUNTIME class id, so
/// every inherited compiled method's trampoline downcasts to the real
/// ancestor struct instead of aborting on a `DynObject`. Named ivars a
/// runtime body invents spill into `IvarCell`'s invented storage.
/// `initialize` resolves through full dispatch, so a runtime-defined body
/// wins over the inherited compiled one.
pub(crate) fn compiled_subclass_construct(
    id: ClassId,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let Some(alloc) = crate::dispatch::ancestor_allocator_of(id) else {
        return dyn_object_construct(id, args, block);
    };
    let handle = alloc(id);
    crate::dispatch::run_initialize(id, &handle, args, block)?;
    Ok(RubyValue::Object(handle))
}

pub(crate) fn dyn_object_construct(
    id: ClassId,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let obj: RObj = Arc::new(DynObject::new(id));
    let init = Symbol::intern("initialize");
    if walk_runtime_class(id, init).is_some() {
        crate::dispatch::send_in(0, &obj, init, args, block)?;
    } else if !args.is_empty() {
        return Err(arg_error!(
            "wrong number of arguments (given {}, expected 0)",
            args.len()
        ));
    }
    Ok(RubyValue::Object(obj))
}

/// A blank name-keyed instance tagged with `id` -- what `BasicObject.new`
/// answers (the root class is instantiable in real Ruby, and its instance is
/// exactly the blank object `Object.new` builds, under its own id).
pub(crate) fn blank_instance(id: ClassId) -> RubyValue {
    RubyValue::Object(Arc::new(DynObject::new(id)))
}

/// An instance of a runtime-created class. Like the root `Object`, its ivars
/// are name-keyed (a runtime class has no compile-time-materialized field
/// list), plus a stored `class_id` (unlike `Object`'s hardcoded `0`) and a
/// per-object frozen flag.
/// An `RObj` shell standing in for a CLASS as the receiver of an `extend`ed
/// user module's compiled method (see `extended_class_method`). It owns no
/// storage of its own: every ivar operation reads and writes the class's own
/// class-level store, so `@x` means the same slot inside the module's body as
/// it does in a `def self.x` or an `instance_variable_get` on the class.
pub(super) struct ClassSurrogate {
    pub(super) class_id: ClassId,
}

impl RubyObject for ClassSurrogate {
    fn class_id(&self) -> ClassId {
        self.class_id
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        crate::dispatch::class_frozen(self.class_id)
    }
    // A class freezes through `Module#freeze`, which the registry records --
    // there is nothing for this shell to mark.
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        crate::civars::class_ivar_names(self.class_id.0)
            .iter()
            .map(|n| crate::civars::class_ivar_get(self.class_id.0, n))
            .collect()
    }
    fn ivar_pairs(&self) -> Vec<(String, RubyValue)> {
        crate::civars::class_ivar_names(self.class_id.0)
            .iter()
            .map(|n| {
                (
                    format!("@{n}"),
                    crate::civars::class_ivar_get(self.class_id.0, n),
                )
            })
            .collect()
    }
    fn ivar_get_named(&self, name: &str) -> Option<RubyValue> {
        Some(crate::civars::class_ivar_get(self.class_id.0, name))
    }
    fn ivar_set_named(&self, name: &str, v: RubyValue) -> bool {
        crate::civars::class_ivar_set(self.class_id.0, name, v).is_ok()
    }
    fn ivar_remove_named(&self, name: &str) -> Option<RubyValue> {
        // `civars` has no remove; answering the current value and clearing it
        // to nil is the closest this store offers.
        let old = crate::civars::class_ivar_get(self.class_id.0, name);
        crate::civars::class_ivar_set(self.class_id.0, name, RubyValue::Nil).ok()?;
        Some(old)
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(ClassSurrogate {
            class_id: self.class_id,
        })
    }
}

pub(super) struct DynObject {
    /// Atomic so a `Ractor` move can RETAG the instance to
    /// `Ractor::MovedObject` in place (`retag_moved`); plain relaxed loads
    /// everywhere else.
    class_id: AtomicU32,
    frozen: AtomicBool,
    /// Insertion-ordered: ruby reports instance variables in FIRST-ASSIGNMENT
    /// order, and that order is a property of the object rather than of its
    /// class -- an ivar added after construction appears last. A compiled
    /// class gets a generated struct whose fields are already in source order;
    /// this is the same guarantee for a class minted at run time. It is what
    /// makes `p obj` stable enough to diff, which is most of what `inspect` is
    /// for.
    ivars: parking_lot::Mutex<indexmap::IndexMap<String, RubyValue>>,
}

impl DynObject {
    pub(super) fn new(class_id: ClassId) -> DynObject {
        DynObject {
            class_id: AtomicU32::new(class_id.0),
            frozen: AtomicBool::new(false),
            ivars: parking_lot::Mutex::new(indexmap::IndexMap::new()),
        }
    }
}

impl RubyObject for DynObject {
    fn class_id(&self) -> ClassId {
        ClassId(self.class_id.load(Ordering::Relaxed))
    }
    fn retag_moved(&self) -> bool {
        self.class_id
            .store(zeo_abi::RACTOR_MOVED_OBJECT_CLASS.0, Ordering::Relaxed);
        true
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Acquire)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Release);
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        self.ivars.lock().values().cloned().collect()
    }
    fn ivar_pairs(&self) -> Vec<(String, RubyValue)> {
        // Keys are stored without the leading `@` (see `ivar_set_named`); pair
        // in one lock so name/value order can't diverge.
        self.ivars
            .lock()
            .iter()
            .map(|(k, v)| (format!("@{k}"), v.clone()))
            .collect()
    }
    fn ivar_get_named(&self, name: &str) -> Option<RubyValue> {
        // A never-assigned ivar reads nil (Ruby's rule), so `Some(Nil)` not
        // `None` -- a name-keyed object always "has" the slot.
        Some(
            self.ivars
                .lock()
                .get(name)
                .cloned()
                .unwrap_or(RubyValue::Nil),
        )
    }
    fn ivar_set_named(&self, name: &str, v: RubyValue) -> bool {
        self.ivars.lock().insert(name.to_string(), v);
        true
    }
    fn ivar_remove_named(&self, name: &str) -> Option<RubyValue> {
        // Name-keyed: a missing key is genuinely absent (CRuby's `NameError`).
        // `shift_remove`, not `remove`: the latter swaps the last entry into
        // the hole, which would reorder the survivors.
        self.ivars.lock().shift_remove(name)
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let d = DynObject::new(RubyObject::class_id(self));
        *d.ivars.lock() = self.ivars.lock().clone();
        if copy_frozen && self.is_frozen() {
            d.set_frozen();
        }
        Arc::new(d)
    }
}

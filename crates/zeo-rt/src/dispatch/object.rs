//! The root `Object` type: the name-keyed instance behind `main` and every
//! `Object.new`, its `RubyObject` impl, the dynamic ivar accessors
//! (`ivar_*_dyn`), and the `RObj` downcast helpers.

use super::*;

/// The root of every class hierarchy. `ClassId(0)`, no superclass -- the
/// base case `ruby_class!`'s `$super` bottoms out at.
///
/// Unlike a generated class, its ivars live in a NAME-KEYED map rather than
/// struct fields: this is the type behind `main_object()`, and the top level's
/// `self` has no compile-time class whose ivar list codegen could have
/// materialized into fields. `@x` at the top level (or inside a block that
/// `instance_exec` later rebinds onto some other receiver) is resolved by
/// name at runtime, so a map is the honest representation.
#[derive(Default)]
pub struct Object {
    /// Insertion-ordered: ruby reports instance variables in
    /// FIRST-ASSIGNMENT order, and that order is a property of the object
    /// rather than of its class. Every `Object.new` is one of these (the
    /// emitter folds it to `zeo_rt_object_new_sentinel`), not just `main`.
    ivars: parking_lot::Mutex<indexmap::IndexMap<String, RubyValue>>,
    /// `Object.new.freeze` sets this; `main` never does. A generic Object had
    /// no frozen slot at all before, so `freeze`/`frozen?`/`clone(freeze:)`
    /// silently no-op'd (issue_3033).
    frozen: std::sync::atomic::AtomicBool,
    /// A `Ractor` move's poison bit -- this type has no per-instance class
    /// word to retag, so `class_id()` consults the flag instead (cold type;
    /// generated classes carry an atomic class word and skip this).
    moved: std::sync::atomic::AtomicBool,
}

impl Object {
    pub const CLASS_ID: ClassId = ClassId(0);

    /// A fresh `Object.new`, registered with the cycle collector.
    ///
    /// Every allocation of this type goes through here. Its ivars are the
    /// only edges it owns, and a name-keyed map of `RubyValue` is a place a
    /// reference cycle can close -- `o.instance_variable_set(:@self, o)` at
    /// the top level is the shortest one there is.
    pub fn new_value() -> RubyValue {
        let o: RObj = Arc::new(Object::default());
        crate::gc::record_object(&o);
        RubyValue::Object(o)
    }
}

impl RubyObject for Object {
    fn class_id(&self) -> ClassId {
        if self.moved.load(std::sync::atomic::Ordering::Relaxed) {
            return zeo_abi::RACTOR_MOVED_OBJECT_CLASS;
        }
        Self::CLASS_ID
    }
    fn retag_moved(&self) -> bool {
        self.moved.store(true, std::sync::atomic::Ordering::Relaxed);
        true
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn set_frozen(&self) {
        self.frozen
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        self.ivars.lock().values().cloned().collect()
    }
    fn ivar_pairs(&self) -> Vec<(String, RubyValue)> {
        // The root object's keys are stored WITHOUT the `@` (see
        // `ivar_get_named`); paired in a single lock so name/value order can't
        // diverge.
        self.ivars
            .lock()
            .iter()
            .map(|(k, v)| (format!("@{k}"), v.clone()))
            .collect()
    }
    fn ivar_get_named(&self, name: &str) -> Option<RubyValue> {
        // A never-assigned ivar reads `nil` rather than raising -- Ruby's
        // rule, and the reason this answers `Some(Nil)` instead of `None`
        // (`None` means "this class has no such slot", which for a
        // name-keyed object is never true).
        Some(
            self.ivars
                .lock()
                .get(name)
                .cloned()
                .unwrap_or(RubyValue::Nil),
        )
    }
    fn ivar_set_named(&self, name: &str, v: RubyValue) -> bool {
        // Probe before inserting: an ivar is written far more often than it is
        // first created, and `insert` allocated a fresh `String` key on every
        // one of those writes only to drop it again.
        let mut ivars = self.ivars.lock();
        match ivars.get_mut(name) {
            Some(slot) => *slot = v,
            None => {
                ivars.insert(name.to_string(), v);
            }
        }
        true
    }
    fn ivar_remove_named(&self, name: &str) -> Option<RubyValue> {
        // Name-keyed: a missing key is genuinely absent, so `None` cleanly
        // signals CRuby's `NameError` case. `shift_remove`, not `remove`: the
        // latter swaps the last entry into the hole and would reorder the
        // survivors.
        self.ivars.lock().shift_remove(name)
    }
    fn gc_visit(&self, out: &mut Vec<RubyValue>, take: bool) {
        let mut ivars = self.ivars.lock();
        if take {
            out.extend(std::mem::take(&mut *ivars).into_values());
        } else {
            out.extend(ivars.values().cloned());
        }
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let copy: RObj = Arc::new(Object {
            ivars: parking_lot::Mutex::new(self.ivars.lock().clone()),
            frozen: std::sync::atomic::AtomicBool::new(copy_frozen && self.is_frozen()),
            moved: std::sync::atomic::AtomicBool::new(false),
        });
        crate::gc::record_object(&copy);
        copy
    }
}

/// The top-level `self` -- CRuby's `main`, a plain `Object` instance.
/// Generated code passes it as the receiver of top-level-defined methods
/// (which live on `Object`, exactly like real Ruby's private-on-Object
/// rule) and as the value of a top-level `self` expression. One shared
/// instance so identity is stable across the program, lazily built since
/// most programs never touch it.
pub fn main_object() -> RubyValue {
    main_slot().clone()
}

fn main_slot() -> &'static RubyValue {
    static MAIN: std::sync::OnceLock<RubyValue> = std::sync::OnceLock::new();
    MAIN.get_or_init(Object::new_value)
}

/// Whether `o` IS `main`. CRuby installs `to_s`/`inspect` singletons on the
/// top-level self that answer `"main"`, so it never renders as an address;
/// zeo asks by identity instead, because installing a singleton at startup
/// would mark the runtime-overlay maps live for every program.
pub fn is_main_object(o: &RObj) -> bool {
    matches!(main_slot(), RubyValue::Object(m) if Arc::ptr_eq(m, o))
}

/// The names `main` answers as PRIVATE singleton methods. One owner, so
/// dispatch and reflection cannot drift: [`main_mixin`] routes exactly these,
/// `respond_to?(name, true)` reports them, and `private_methods` lists them.
///
/// `to_s` and `inspect` are main's other two singletons and are PUBLIC, so
/// they belong to `singleton_methods` instead (`builtins::kernel`).
pub const MAIN_PRIVATE_SINGLETONS: &[&str] = &[
    "define_method",
    "include",
    "private",
    "public",
    "ruby2_keywords",
    "using",
];

/// Whether `recv` is `main` and `name` is one of [`MAIN_PRIVATE_SINGLETONS`].
pub fn is_main_private_singleton(recv: &RubyValue, name: Symbol) -> bool {
    let RubyValue::Object(o) = recv else {
        return false;
    };
    o.class_id() == zeo_abi::OBJECT_CLASS
        && is_main_object(o)
        && MAIN_PRIVATE_SINGLETONS
            .iter()
            .any(|n| *n == name.name_str())
}

/// `main`'s private singleton methods: each lands on `Object`, which is what
/// makes what it does visible everywhere afterwards.
///
/// `None` means the name is not one of them, and the ordinary dispatch
/// continues -- so this costs one symbol compare on a path that already
/// checked object identity.
pub(super) fn main_mixin(
    name: Symbol,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Option<Result<RubyValue, Signal>> {
    // Oracle-checked: main's singletons are `define_method`, `include`,
    // `inspect`, `private`, `public`, `ruby2_keywords`, `to_s` and `using`.
    // `to_s`/`inspect` answer "main" in `value::inspect`; `using` is
    // `Kernel#using`, which zeo answers at compile time. The four routed
    // here mix into `Object`, which is where CRuby's own singletons put
    // them.
    //
    // The list is CLOSED on purpose. There is no `prepend` and no `extend`,
    // so routing either would shadow a user's own top-level `def` --
    // `spinel::anon_double_splat_forward.rb`'s `def prepend(**)` is an
    // ordinary method that happens to share the name.
    let target = RubyValue::Class(zeo_abi::OBJECT_CLASS);
    match name.name_str() {
        "include" => Some(crate::runtime_meta::runtime_include(&target, args)),
        // `private`, `public` and `define_method` are `Module`'s own rows on
        // Object. Without them a bare `private` at top level -- plain Ruby,
        // and the shape rubygems' own files open with -- was a NoMethodError
        // naming `main`.
        "private" | "public" | "define_method" | "ruby2_keywords" => {
            Some(crate::dispatch::send_value(&target, name, args, block))
        }
        // `using` is `Kernel#using`, which zeo answers at compile time; it
        // reaches its own row through the ordinary walk.
        _ => None,
    }
}

/// Read `@name` off a receiver whose concrete class isn't statically known
/// -- what codegen emits for an ivar access when `self` is a `RubyValue`
/// rather than an `Arc<Concrete>` (top level, or a block whose self
/// `instance_exec` may rebind). `name` excludes the `@`.
///
/// A receiver that never had `@name` assigned answers nil rather than raising,
/// which is Ruby's rule for reading any unset ivar. An immediate has nowhere to
/// store one and so is always in that case; a bare heap value has the
/// identity-keyed `value_ivars` store and may well have one.
pub fn ivar_get_dyn(recv: &RubyValue, name: &str) -> RubyValue {
    match recv {
        // The `or_else` serves the runtime's hand-written objects, which
        // declare no ivars and so decline the write in `ivar_set_dyn` -- see
        // `value_ivars::key`'s `Object` arm. A generated class never reaches
        // it: its named path is total over declared and invented ivars alike.
        RubyValue::Object(o) => o
            .ivar_get_named(name)
            .or_else(|| crate::value_ivars::get(recv, name))
            .unwrap_or(RubyValue::Nil),
        // A CLASS object's own ivars live in their own table (see
        // `civars`' docs for why they can't share `cvars`'). Reached when a
        // class-method body's `self` is dynamic rather than the static class
        // id codegen usually emits -- e.g. `def self.x; [1].each { @n } end`,
        // where the block captures `self` as a plain `RubyValue::Class`.
        RubyValue::Class(cid) => crate::civars::class_ivar_get(cid.0, name),
        // `[].instance_eval { @x }` -- see `value_ivars`.
        _ => crate::value_ivars::get(recv, name).unwrap_or(RubyValue::Nil),
    }
}

/// [`ivar_get_dyn`] behind CRuby's Ractor guard -- what codegen emits for a
/// dynamic self, the only receiver that can be a shareable object read from a
/// non-main ractor. See [`crate::ractor::ivar_isolation_check`].
pub fn ivar_get_dyn_isolated(recv: &RubyValue, name: &str) -> Result<RubyValue, Signal> {
    crate::ractor::ivar_isolation_check(recv)?;
    Ok(ivar_get_dyn(recv, name))
}

/// [`ivar_slot_get_dyn`] behind the same guard.
pub fn ivar_slot_get_dyn_isolated(
    recv: &RubyValue,
    slot: usize,
    name: &str,
) -> Result<RubyValue, Signal> {
    crate::ractor::ivar_isolation_check(recv)?;
    Ok(ivar_slot_get_dyn(recv, slot, name))
}

/// The declared name of `slot` on `cid`'s compiled layout, for the doors
/// that must route a slot access on a SLOTLESS receiver -- a C-allocated
/// instance of a compiled class -- to the name-keyed store.
pub(crate) fn slot_ivar_name(cid: crate::ClassId, slot: usize) -> Option<&'static str> {
    let layout = crate::compiled_object::layout_of(cid)?;
    layout.names.get(slot.checked_sub(layout.hidden)?).copied()
}

/// Read `@name` from a receiver whose class is one of a known set that all
/// place `@name` at `slot` -- the shared-body form of `ivar_get_dyn`.
///
/// The caller must prove the index agrees across the whole set before using
/// this, and every such receiver is a generated object. `name` serves the
/// arms that reach neither (a rebound `self`), which is exactly what
/// `ivar_get_dyn` already answers.
#[inline]
pub fn ivar_slot_get_dyn(recv: &RubyValue, slot: usize, name: &str) -> RubyValue {
    match recv {
        RubyValue::Object(o) if o.has_ivar_slots() => o.ivar_slot_get(slot),
        other => ivar_get_dyn(other, name),
    }
}

/// `ivar_slot_get_dyn`'s counterpart, carrying the same frozen check
/// `ivar_set_dyn` does.
#[inline]
pub fn ivar_slot_set_dyn(
    recv: &RubyValue,
    slot: usize,
    name: &str,
    v: RubyValue,
) -> Result<RubyValue, Signal> {
    match recv {
        RubyValue::Object(o) => {
            crate::builtins::check_frozen(recv)?;
            if !o.ivar_slot_set(slot, v.clone()) {
                return ivar_set_dyn(recv, name, v);
            }
            Ok(v)
        }
        other => ivar_set_dyn(other, name, v),
    }
}

/// Write `@name` on a statically-unknown receiver -- `ivar_get_dyn`'s
/// counterpart, carrying Ruby's frozen check (`rb_check_frozen`) and its
/// can't-modify-frozen raise, which the static field-write path emits
/// inline instead.
pub fn ivar_set_dyn(recv: &RubyValue, name: &str, v: RubyValue) -> Result<RubyValue, Signal> {
    crate::ractor::ivar_isolation_check(recv)?;
    match recv {
        RubyValue::Object(o) => {
            crate::builtins::check_frozen(recv)?;
            // A hand-written runtime object declares no ivars and answers
            // `false`. Dropping the write there would make `class StringScanner;
            // def tag=(v); @tag = v; end; end` silently read back nil, so it
            // goes to the identity-keyed store instead.
            if !o.ivar_set_named(name, v.clone()) {
                crate::value_ivars::set(recv, name, v.clone());
            }
            Ok(v)
        }
        // See `ivar_get_dyn`'s Class arm. The frozen-class guard lives
        // inside `class_ivar_set` itself.
        RubyValue::Class(cid) => {
            crate::civars::class_ivar_set(cid.0, name, v.clone())?;
            Ok(v)
        }
        // An unfrozen heap value (`[].instance_eval { @x = 1 }`) gets the
        // identity-keyed store; the permanently-frozen tier (immediates, and
        // `Range`) has nowhere to put one and raises, naming the class the way
        // CRuby's `rb_check_frozen` does.
        _ => {
            crate::builtins::check_frozen(recv)?;
            if !crate::value_ivars::set(recv, name, v.clone()) {
                return Err(frozen_error!(
                    "can't modify frozen {}: {}",
                    crate::builtins::class_name_of(recv),
                    recv.inspect_string()
                ));
            }
            Ok(v)
        }
    }
}

/// Downcasts an erased `RObj` to an owned `Arc<T>` -- the Path 2 trampoline
/// counterpart to `as_any().downcast_ref()`, needed because every generated
/// method takes `self: Arc<Self>` now (see `ruby_class!`'s docs). Cloning
/// `recv` first is a cheap `Arc` refcount bump, not a deep copy.
pub fn downcast_robj<T: RubyObject>(recv: &RObj) -> Option<Arc<T>> {
    recv.clone().as_any_rc().downcast::<T>().ok()
}

/// `downcast_robj` without the refcount traffic: a BORROW of the concrete
/// struct, for a trampoline that only reads or writes a field and never hands
/// the receiver to anything that could outlive the call. The owned form costs
/// an `Arc` clone plus its matching drop -- 4.08 ns measured, which is most of
/// an accessor call -- so an accessor trampoline (`zeo_tramp!`'s `rd`/`wr`
/// heads) takes this one instead.
#[inline]
pub fn downcast_robj_ref<T: RubyObject>(recv: &RObj) -> Option<&T> {
    recv.as_any().downcast_ref::<T>()
}

/// The frozen-receiver raise every ivar WRITE guards itself with, as one
/// out-of-line call instead of the `format!` + `construct_by_class_id`
/// codegen this used to inline at every ivar write -- thousands of them in a
/// program like prism. `#[cold]` so the caller's guard stays a
/// predictable never-taken branch around a single relaxed atomic load, and by
/// VALUE so the `Arc` bump the handle needs happens only on the raise path.
///
/// It also corrects the inlined form, which built its message from the
/// compiler's `ClassInfo::name` and carried no `receiver`: CRuby reports the
/// FULLY QUALIFIED name (`can't modify frozen M::Inner:`, not `Inner:`) and
/// answers `FrozenError#receiver`, both of which the runtime knows and a
/// literal baked at each write site did not.
#[cold]
#[inline(never)]
pub fn ivar_frozen_error(recv: RObj) -> Signal {
    crate::builtins::check_frozen(&RubyValue::Object(recv))
        .expect_err("only called once the receiver is known frozen")
}

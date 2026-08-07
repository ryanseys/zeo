//! Value-builtin subclasses (D3): `class Stack < Array`, `class Tag < String`,
//! `class Counter < Hash`.
//!
//! A subclass of an instantiable value builtin is ONE generic `ValueSubclass`
//! RObj wrapping a `RubyValue` PAYLOAD of the root kind (an `Array`/`String`/
//! `Hash` handle) plus name-keyed user ivars -- the same "one native type, many
//! class ids" shape the exception hierarchy uses (`exception::RubyException`),
//! rather than a per-class generated struct. Inherited builtin methods run
//! against the payload through the `send_in` payload bridge; the subclass's own
//! `def`s register as `define_method` deltas (dynamic self), exactly like an
//! exception subclass.
//!
//! The root list has grown well past the collection roots it started with --
//! `StringScanner`, `StringIO`, `File`, `Set`, `Enumerator`, `Time`, `Thread`,
//! `Range` -- because nothing about the bridge is Array/String/Hash-specific:
//! the payload is just a `RubyValue`. `Regexp` stays out only because
//! subclassing it is vanishingly rare, and `Class`/`Module` because a class id
//! has no per-value dispatch to hang a payload on.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use parking_lot::Mutex;
use zeo_abi::{ARRAY_CLASS, ClassId, HASH_CLASS, STRING_CLASS};

use crate::dispatch::{
    ClassRegistry, ConstructorFn, RObj, RubyObject, ancestors_of_value, has_instance_method,
    raise_error, run_initialize,
};
use crate::signal::Signal;
use crate::symbol::Symbol;
use crate::value::RubyValue;
use crate::{array_new, hash_new, string_new};

/// The single native type behind every value-builtin subclass instance,
/// distinguished by its `class_id`. `root` is the builtin whose methods it
/// inherits (`Array`/`String`/`Hash`); `payload` holds the wrapped value (a
/// `Mutex` so the whole handle can be RE-SEATED by `super`/`replace`/a rebuilt
/// collection, while ordinary in-place mutation goes through the payload's own
/// inner `Arc<Mutex<…>>` and never takes this outer lock). User ivars are
/// name-keyed (insertion-ordered), like `RubyException`.
pub struct ValueSubclass {
    /// Atomic so a `Ractor` move can retag the husk to `Ractor::MovedObject`
    /// in place (`retag_moved`); relaxed loads everywhere else.
    class_id: AtomicU32,
    root: ClassId,
    frozen: AtomicBool,
    payload: Mutex<RubyValue>,
    ivars: Mutex<Vec<(String, RubyValue)>>,
}

impl ValueSubclass {
    /// Allocates directly as the trait-object handle every caller stores.
    fn alloc(class_id: ClassId, root: ClassId, payload: RubyValue) -> RObj {
        Arc::new(ValueSubclass {
            class_id: AtomicU32::new(class_id.0),
            root,
            frozen: AtomicBool::new(false),
            payload: Mutex::new(payload),
            ivars: Mutex::new(Vec::new()),
        })
    }

    fn store_ivar(&self, name: &str, v: RubyValue) {
        let mut ivars = self.ivars.lock();
        match ivars.iter_mut().find(|(k, _)| k == name) {
            Some(slot) => slot.1 = v,
            None => ivars.push((name.to_string(), v)),
        }
    }
}

impl RubyObject for ValueSubclass {
    fn class_id(&self) -> ClassId {
        ClassId(self.class_id.load(Ordering::Relaxed))
    }
    fn retag_moved(&self) -> bool {
        self.class_id
            .store(zeo_abi::RACTOR_MOVED_OBJECT_CLASS.0, Ordering::Relaxed);
        true
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Relaxed);
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        self.ivars.lock().iter().map(|(_, v)| v.clone()).collect()
    }
    fn ivar_pairs(&self) -> Vec<(String, RubyValue)> {
        self.ivars
            .lock()
            .iter()
            .map(|(k, v)| (format!("@{k}"), v.clone()))
            .collect()
    }
    fn ivar_get_named(&self, name: &str) -> Option<RubyValue> {
        self.ivars
            .lock()
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }
    fn ivar_set_named(&self, name: &str, v: RubyValue) -> bool {
        self.store_ivar(name, v);
        true
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        // Deep-copy the payload (a fresh collection with the same elements), so
        // `dup`/`clone` yield an independent subclass instance -- CRuby's shallow
        // rule for the wrapped container.
        // INFALLIBLE: a value-subclass payload is always Str/Array/Hash,
        // whose dup arms never raise.
        let payload = self
            .payload
            .lock()
            .dup_value(false)
            .expect("value-subclass payloads are copyable collections");
        Arc::new(ValueSubclass {
            class_id: AtomicU32::new(self.class_id.load(Ordering::Relaxed)),
            root: self.root,
            frozen: AtomicBool::new(copy_frozen && self.is_frozen()),
            payload: Mutex::new(payload),
            ivars: Mutex::new(self.ivars.lock().clone()),
        })
    }
    fn builtin_payload(&self) -> Option<RubyValue> {
        Some(self.payload.lock().clone())
    }
    fn builtin_root(&self) -> Option<ClassId> {
        Some(self.root)
    }
    fn set_builtin_payload(&self, v: RubyValue) -> bool {
        *self.payload.lock() = v;
        true
    }
}

/// Whether `id` is an instantiable value-builtin payload root (D3).
///
/// `StringScanner` earns a place next to the collection roots because it is the
/// same shape: an instantiable native object a user class wants to inherit the
/// behaviour of while adding ivars of its own (`csv`'s
/// `class Scanner < StringScanner` keeps a `@keeps` stack). Nothing about the
/// bridge is Array/String/Hash-specific -- the payload is just a `RubyValue`,
/// and here it is the `RubyValue::Object` holding the native scanner.
pub fn is_payload_root(id: ClassId) -> bool {
    matches!(
        id,
        ARRAY_CLASS
            | STRING_CLASS
            | HASH_CLASS
            | zeo_abi::STRING_SCANNER_CLASS
            | zeo_abi::STRINGIO_CLASS
            | zeo_abi::FILE_CLASS
            | zeo_abi::SET_CLASS
            | zeo_abi::ENUMERATOR_CLASS
            | zeo_abi::TIME_CLASS
            | zeo_abi::THREAD_CLASS
            | zeo_abi::RANGE_CLASS
    )
}

/// Whether a builtin method found on `anc` belongs to the PAYLOAD of a value
/// subclass whose root is `root`, rather than to the boxed object.
///
/// The root itself always does. So does a builtin SUPERCLASS of the root:
/// `File`'s payload is what `IO#read` reads, and aws-sdk's `ManagedFile <
/// File` inherits every one of its methods from `IO`. Included modules and
/// `Object`/`Kernel` never do -- `Stack.new.class` must answer `Stack`, not
/// `Array`, and `class` resolves on `Object`.
///
/// A no-op for the roots whose superclass is already `Object` (every one but
/// `File`).
pub fn payload_owns(root: ClassId, anc: ClassId) -> bool {
    if root == anc {
        return true;
    }
    let mut at = root;
    while let Some(parent) = builtin_superclass(at) {
        if parent == zeo_abi::OBJECT_CLASS {
            return false;
        }
        if parent == anc {
            return true;
        }
        at = parent;
    }
    false
}

fn builtin_superclass(id: ClassId) -> Option<ClassId> {
    let i = (id.0 as usize).checked_sub(1)?;
    zeo_abi::BUILTINS.get(i)?.superclass
}

/// Allocate a value-subclass instance of `class_id` directly around `payload`,
/// bypassing `initialize` -- `Marshal.load`'s `C`-tag path, which seats the
/// deserialized builtin body itself. `None` when `class_id` isn't a value
/// subclass (no payload root in its ancestry).
pub fn marshal_alloc(class_id: ClassId, payload: RubyValue) -> Option<RObj> {
    let root = value_root_of(class_id)?;
    Some(ValueSubclass::alloc(class_id, root, payload))
}

/// The payload root a value-subclass id inherits from -- the first payload-root
/// builtin in its linearized ancestry (so a multi-level `B < A < Array` finds
/// `Array`). `None` for a non-value-subclass.
pub fn value_root_of(class_id: ClassId) -> Option<ClassId> {
    ancestors_of_value(class_id)
        .iter()
        .copied()
        .find(|&a| is_payload_root(a))
}

/// Register a user value-builtin subclass (D3): just the name + linearized
/// ancestors + the shared `value_subclass_construct` constructor. It installs NO
/// methods -- inherited builtin behavior comes through the `send_in` payload
/// bridge, and the subclass's own `def`s register as `define_method` deltas.
pub fn register_value_subclass(
    registry: &mut ClassRegistry,
    id: ClassId,
    name: &str,
    ancestors: Vec<ClassId>,
) {
    registry.register(
        id,
        name,
        false,
        ancestors,
        Some(value_subclass_construct as ConstructorFn),
    );
}

/// The CLASS methods a value subclass inherits from its payload root:
/// `DOSTime.local(...)`, `IOBuffer.open(...)`, `Tags[1, 2]`. Nothing copies a
/// builtin's class-method TABLE rows onto a subclass's registry entry the way
/// materialization copies a user `def self.x`, so without this the root's
/// constructors are simply invisible -- and rubyzip's `DOSTime.from_time`
/// calls the inherited `local` directly.
///
/// Only the ROOT's own table, never the whole ancestry: every class has
/// `Object`/`Kernel` above it, and letting those tables answer here would give
/// each of them class methods CRuby's singleton chain never reaches.
pub fn root_class_method_target(class_id: ClassId, name: &str) -> Option<ClassId> {
    let root = value_root_of(class_id)?;
    if root == class_id {
        return None;
    }
    // NEVER `new`: construction belongs to `Class#new` and `constructor_of`,
    // which is also what decides a class has NO allocator. `Process::Waiter`
    // is a Thread subclass whose `new` CRuby undefs, and answering it here
    // would build one instead of raising.
    if name == "new" {
        return None;
    }
    // A private row (`Time._load`) stays unreachable, exactly as an inherited
    // private class method is in CRuby.
    if crate::builtins::builtin_class_method_is_private(root, name) {
        return None;
    }
    crate::builtins::class_method_table(root)
        .and_then(|lookup| lookup(name))
        .map(|_| root)
}

/// Run a class method inherited from the payload root. A row marked `allocs`
/// comes back re-tagged as the SUBCLASS, because that row allocates through the
/// receiver class -- `DOSTime.local(...)` is a `DOSTime`. Everything else
/// passes through untouched, so `Managed.read(path)` still answers a String,
/// `Traced.current` still answers the `Thread` that already exists, and
/// `Seq.produce {}` still answers a plain `Enumerator`.
///
/// The marker lives on the row, which is where CRuby keeps the same knowledge:
/// every class-method C function receives the real receiver as `klass` and
/// decides for itself, so `rb_ary_s_create` threads it while
/// `enumerator_s_produce` and `thread_s_current` ignore it. zeo needs the
/// answer OUT here only because a value-builtin payload carries no class id of
/// its own. See `zeo_dsl::MethodDef::allocs`.
pub fn call_root_class_method(
    class_id: ClassId,
    root: ClassId,
    name: &str,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let table = crate::builtins::class_method_table(root)
        .expect("root_class_method_target proved the table exists");
    let f = table(name).expect("root_class_method_target proved the row exists");
    let result = f(&RubyValue::Class(root), args, block)?;
    match crate::builtins::builtin_class_method_allocs(root, name) && result.class_id() == root {
        true => Ok(RubyValue::Object(ValueSubclass::alloc(
            class_id, root, result,
        ))),
        false => Ok(result),
    }
}

/// An empty payload of `root`'s kind -- the pre-`initialize` default for the
/// user-`initialize` path (a `super` then re-seats it). Each payload root has a
/// real empty form; nothing else is a payload root.
fn empty_payload(root: ClassId) -> RubyValue {
    match root {
        ARRAY_CLASS => RubyValue::Array(array_new(Vec::new())),
        STRING_CLASS => RubyValue::Str(string_new(String::new())),
        HASH_CLASS => RubyValue::Hash(hash_new(Vec::new())),
        // A scanner over the empty string -- built through the root's own
        // constructor, since the native object isn't a `RubyValue` variant.
        // Built through the root's own constructor: the native object is not
        // a `RubyValue` variant, so there is nothing to spell directly.
        zeo_abi::STRING_SCANNER_CLASS | zeo_abi::STRINGIO_CLASS => {
            let empty = RubyValue::Str(string_new(String::new()));
            construct_root_payload(root, &[empty], None).unwrap_or(RubyValue::Nil)
        }
        // A File has no empty form -- opening one needs a path. A subclass
        // with its own `initialize` seats the real payload through `super`,
        // which is the only way to get a File in the first place.
        zeo_abi::FILE_CLASS => RubyValue::Nil,
        // An Enumerator has no empty form either: `Enumerator.new` without a
        // block is an ArgumentError, because the block IS the sequence. Same
        // `File` shape -- a subclass's own `initialize` seats the real payload
        // through `super() { |y| ... }`, which is the only way to get one.
        zeo_abi::ENUMERATOR_CLASS => RubyValue::Nil,
        // `Time.new` with no arguments IS the empty form -- it answers `now`,
        // exactly as CRuby's does.
        zeo_abi::TIME_CLASS => construct_root_payload(root, &[], None).unwrap_or(RubyValue::Nil),
        // A blockless Thread cannot be built -- the block IS the work -- so this
        // is the `File` shape again. The gems subclass Thread precisely to wrap
        // `initialize`, which seats the real thread through `super`.
        zeo_abi::THREAD_CLASS => RubyValue::Nil,
        // `Range.new` demands both endpoints, so there is no empty form here
        // either -- the `File` shape again.
        zeo_abi::RANGE_CLASS => RubyValue::Nil,
        zeo_abi::SET_CLASS => construct_root_payload(root, &[], None).unwrap_or(RubyValue::Nil),
        _ => RubyValue::Nil,
    }
}

/// Build the root builtin's own value from constructor args -- reuses the
/// runtime's existing `Array.new`/`String.new`/`Hash.new` class method, so
/// `Stack.new(3)` seeds `[nil, nil, nil]` exactly as `Array.new(3)` does.
fn construct_root_payload(
    root: ClassId,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let table = crate::builtins::class_method_table(root)
        .expect("value payload root has a class-method table");
    let ctor = table("new").expect("value payload root has a `new` constructor");
    ctor(&RubyValue::Class(root), args, block)
}

/// The `ConstructorFn` behind every value-builtin subclass. With a user
/// `initialize`, allocate an EMPTY payload and run it (a `super` re-seats via
/// `value_super`); without one, seed the payload directly from the root
/// constructor with the args (`Stack.new(3)` == `Array.new(3)`).
pub fn value_subclass_construct(
    class_id: ClassId,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let root = value_root_of(class_id).unwrap_or_else(|| {
        panic!(
            "no value payload root in ancestry of class id {}",
            class_id.0
        )
    });
    if has_instance_method(class_id, Symbol::intern("initialize")) {
        let handle = ValueSubclass::alloc(class_id, root, empty_payload(root));
        run_initialize(class_id, &handle, args, block)?;
        Ok(RubyValue::Object(handle))
    } else {
        let payload = construct_root_payload(root, args, block)?;
        Ok(RubyValue::Object(ValueSubclass::alloc(
            class_id, root, payload,
        )))
    }
}

/// `super` from a value-subclass method into the inherited builtin (D3). A
/// `super` in `initialize` REBUILDS the payload from the args (`super(3)` ==
/// `Array.new(3)`); any other `super` runs the root builtin method against the
/// payload and re-wraps a self-return back to the subclass. The runtime
/// counterpart of codegen's HIR splice, needed because the inherited methods are
/// native (`class_table`), not registry entries.
pub fn value_super(
    recv: &RubyValue,
    mname: &str,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let obj = recv.as_object_unchecked();
    let root = obj
        .builtin_root()
        .expect("value_super on a non-value-subclass receiver");
    if mname == "initialize" {
        let seeded = construct_root_payload(root, args, block)?;
        obj.set_builtin_payload(seeded);
        return Ok(RubyValue::Nil);
    }
    let payload = obj
        .builtin_payload()
        .expect("value subclass carries a payload");
    let table = crate::builtins::class_table(root).expect("value payload root has a class_table");
    let f = table(mname).ok_or_else(|| {
        raise_error(
            "NoMethodError",
            format!("super: no superclass method '{mname}'"),
        )
    })?;
    let result = f(&payload, args, block)?;
    Ok(rewrap_self_return(result, &payload, &obj, mname))
}

/// The identity CONVERSIONS, which CRuby specifies to return a plain
/// base-class object when the receiver is a subclass -- unlike their
/// near-twins `to_ary`/`to_hash`, which return self and keep the subclass
/// (oracle-verified: `A.new.to_a` is an `Array` but `A.new.to_ary` is an `A`;
/// likewise `to_h` vs `to_hash`). They return the same handle a self-returning
/// mutator does, so the identity check below cannot tell them apart on its own.
const DEMOTING_CONVERSIONS: &[&str] = &["to_s", "to_str", "to_a", "to_h"];

/// Re-wrap a builtin method's return value back into the subclass when it
/// returned the SAME payload handle (a self-returning mutator like `push`/`<<`/
/// `concat`/`replace`) -- so `stack.push(1)` is a `Stack`, while `stack.map { }`
/// (a NEW array) stays a plain `Array`, matching CRuby with almost no method
/// allowlist: `DEMOTING_CONVERSIONS` is the one exception the handle identity
/// genuinely can't distinguish. Rewrapping those was not merely a wrong class
/// -- a subclass whose `<=>` read `o.to_s <=> to_s` never reached a plain
/// String, so the user method re-dispatched until the stack overflowed.
pub fn rewrap_self_return(
    result: RubyValue,
    payload: &RubyValue,
    recv: &RObj,
    mname: &str,
) -> RubyValue {
    if DEMOTING_CONVERSIONS.contains(&mname) {
        return result;
    }
    match (value_identity(&result), value_identity(payload)) {
        (Some(a), Some(b)) if a == b => RubyValue::Object(recv.clone()),
        _ => result,
    }
}

/// The `Arc` identity of a payload-kind value (`Array`/`Hash`/`String`), for the
/// self-return check. A different `Arc` means a freshly-built collection.
fn value_identity(v: &RubyValue) -> Option<usize> {
    match v {
        RubyValue::Array(a) => Some(Arc::as_ptr(a) as usize),
        RubyValue::Hash(h) => Some(Arc::as_ptr(h) as usize),
        RubyValue::Str(s) => Some(Arc::as_ptr(s) as *const () as usize),
        _ => None,
    }
}

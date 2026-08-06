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
//! Range/Regexp are NOT payload roots here: `Range` has no runtime constructor
//! at all, and `Regexp` subclassing is vanishingly rare -- both stay a clean
//! analyze rejection (documented), so this covers the collection roots that
//! matter (`Array`/`String`/`Hash`).

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

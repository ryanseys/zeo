//! The built-in exception hierarchy, hand-written natively.
//!
//! Every generated program used to embed ~6,600 lines of `ruby_class!`-expanded
//! exception classes (`Exception`, `StandardError`, the whole tree) plus a
//! factory -- ~76% of the smallest program, recompiled cold once per binary.
//! Those classes are FIXED (the same in every program), so they belong compiled
//! once, here. `register_exceptions` installs them into a program's
//! `ClassRegistry` at the ids `spinel-abi` reserves for them (`EXCEPTION_CLASSES`),
//! which the compiler independently assigns the same way and asserts.
//!
//! One native `RubyException` type backs all of them, distinguished by its
//! `class_id`; the six `Exception` methods (`initialize`/`message`/`to_s`/
//! `backtrace`/`full_message`/`inspect`) plus `StopIteration`'s two are shared
//! fn pointers that read the receiver's class dynamically. The compiler keeps the
//! exception HIR (for resolving names, inlining `super`, and materializing user
//! subclasses), so a `class MyError < StandardError` is unchanged -- only the
//! fixed classes themselves move here.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use spinel_abi::{declared_ancestors, ClassId, EXCEPTION_CLASS, EXCEPTION_CLASSES};

use crate::dispatch::{
    class_name, downcast_robj, raise_error, run_initialize, send, ClassRegistry, ConstructorFn,
    RObj, RubyObject,
};
use crate::signal::Signal;
use crate::symbol::Symbol;
use crate::value::RubyValue;
use crate::{array_new, string_new};

/// `StopIteration`'s id -- the classes that also carry `__set_result`/`#result`
/// are it and its descendants (`ClosedQueueError`), matched by ancestry.
const STOP_ITERATION_ID: ClassId = ClassId(78);

/// The single native type backing every built-in exception class. The message
/// and StopIteration's result live in DEDICATED internal slots (`mesg`/`res`),
/// NOT among the user ivars -- matching CRuby, where a raised exception's
/// `instance_variables` is `[]` and `instance_variable_get(:@message)` is `nil`
/// (the message is a hidden `mesg` field, not `@message`). User ivars are
/// name-keyed (insertion-ordered, like Ruby) rather than typed struct fields,
/// since there is no per-class Rust struct here -- which incidentally makes
/// `instance_variable_set` on an arbitrary name actually store (a generated
/// exception silently dropped unknown names).
pub struct RubyException {
    class_id: ClassId,
    frozen: AtomicBool,
    /// The hidden message slot -- `Exception#message`/`#to_s` read it; a user
    /// `@message = x` does NOT (that lands in `ivars`), exactly as in CRuby.
    mesg: Mutex<RubyValue>,
    /// StopIteration's hidden result slot (`#result`/`__set_result`), likewise
    /// invisible to `instance_variables`.
    res: Mutex<RubyValue>,
    ivars: Mutex<Vec<(String, RubyValue)>>,
}

impl RubyException {
    fn new(class_id: ClassId) -> Arc<Self> {
        Arc::new(RubyException {
            class_id,
            frozen: AtomicBool::new(false),
            mesg: Mutex::new(RubyValue::Nil),
            res: Mutex::new(RubyValue::Nil),
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

impl RubyObject for RubyException {
    fn class_id(&self) -> ClassId {
        self.class_id
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
        Arc::new(RubyException {
            class_id: self.class_id,
            frozen: AtomicBool::new(copy_frozen && self.is_frozen()),
            mesg: Mutex::new(self.mesg.lock().clone()),
            res: Mutex::new(self.res.lock().clone()),
            ivars: Mutex::new(self.ivars.lock().clone()),
        })
    }
}

/// Downcast a receiver these methods are registered on -- always a
/// `RubyException` (they are installed only on the native exception classes), so
/// a miss is an invariant violation, not a Ruby-level condition.
fn exc(recv: &RObj) -> Arc<RubyException> {
    downcast_robj::<RubyException>(recv)
        .expect("exception method received a non-RubyException receiver")
}

/// The frozen-write guard the generated `initialize` emitted: writing a frozen
/// exception (its message or result slot) raises `FrozenError` with CRuby's
/// message shape.
fn guard_frozen(recv: &RObj, e: &RubyException) -> Result<(), Signal> {
    if e.is_frozen() {
        let cls = class_name(e.class_id).unwrap_or_default();
        let inspected = RubyValue::Object(recv.clone()).inspect_string();
        return Err(raise_error(
            "FrozenError",
            format!("can't modify frozen {cls}: {inspected}"),
        ));
    }
    Ok(())
}

// --- the shared Exception methods -----------------------------------------

/// `def initialize(msg = nil); @message = msg; end`
fn exc_initialize(recv: &RObj, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    let msg = args.first().cloned().unwrap_or(RubyValue::Nil);
    guard_frozen(recv, &e)?;
    *e.mesg.lock() = msg.clone();
    Ok(msg)
}

/// `def to_s; message_slot || self.class.name; end` -- reads the hidden `mesg`
/// slot (NOT a `@message` ivar, which a subclass may set independently).
fn exc_to_s(recv: &RObj, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    let msg = e.mesg.lock().clone();
    if msg.truthy() {
        Ok(msg)
    } else {
        Ok(RubyValue::Str(string_new(
            class_name(e.class_id).unwrap_or_default(),
        )))
    }
}

/// `def message; to_s; end` -- a DYNAMIC send, so a subclass that overrides
/// `to_s` (but not `message`) has its `to_s` honored here, exactly as in CRuby
/// (`Custom#message` follows `Custom#to_s`). Calling `exc_to_s` directly would
/// bypass the override.
fn exc_message(recv: &RObj, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    send(recv, Symbol::intern("to_s"), &[], None)
}

/// `def backtrace; []; end`
fn exc_backtrace(_recv: &RObj, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Array(array_new(Vec::new())))
}

/// `def full_message; self.class.name + ": " + message; end`
fn exc_full_message(recv: &RObj, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    let name = class_name(e.class_id).unwrap_or_default();
    // Dynamic `to_s` (honors a subclass override), matching `#message`/`#inspect`.
    let msg = send(recv, Symbol::intern("to_s"), &[], None)?.to_display_string();
    Ok(RubyValue::Str(string_new(format!("{name}: {msg}"))))
}

/// The exception `inspect`: empty message -> the class name; a message with a
/// newline -> `#<Name:<message.inspect>>`; else `#<Name: message>`.
fn exc_inspect(recv: &RObj, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    let name = class_name(e.class_id).unwrap_or_default();
    // `#inspect` reads `to_s` (NOT `message`) -- oracle: a `message`-only
    // override does not change `inspect`, but a `to_s` override does. Dynamic
    // so the subclass's `to_s` is honored.
    let s = send(recv, Symbol::intern("to_s"), &[], None)?.to_display_string();
    let out = if s.is_empty() {
        name
    } else if s.contains('\n') {
        let inspected = RubyValue::Str(string_new(s)).inspect_string();
        format!("#<{name}:{inspected}>")
    } else {
        format!("#<{name}: {s}>")
    };
    Ok(RubyValue::Str(string_new(out)))
}

/// `StopIteration#__set_result(v); @result = v; end`
fn stop_set_result(recv: &RObj, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    let v = args.first().cloned().unwrap_or(RubyValue::Nil);
    guard_frozen(recv, &e)?;
    *e.res.lock() = v.clone();
    Ok(v)
}

/// `StopIteration#result` -- the hidden `res` slot (not a `@result` ivar).
fn stop_result(recv: &RObj, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(exc(recv).res.lock().clone())
}

/// The one `ConstructorFn` behind every exception class: allocate a
/// `RubyException` tagged with the class the `Class#new`/factory call named, and
/// run `initialize` through the ordinary trampoline.
fn exception_construct(
    class: ClassId,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let handle: RObj = RubyException::new(class);
    run_initialize(class, &handle, args, block)?;
    Ok(RubyValue::Object(handle))
}

/// Install the whole built-in exception hierarchy into `registry`. Called from
/// `ClassRegistry::with_core` in place of the ~6,600 lines of `ruby_class!`
/// blocks each program used to emit. Ancestors come from the single core-class
/// linearizer (`declared_ancestors`), so `rescue`/`is_a?` agree with the
/// compiler's own materialized `ancestors`.
pub fn register_exceptions(registry: &mut ClassRegistry) {
    for row in EXCEPTION_CLASSES {
        let ancestors = declared_ancestors(row.id);
        if row.is_module {
            // The `Errno` namespace: a module (no constructor, ancestors = self).
            registry.register(row.id, row.name, true, ancestors, None);
            continue;
        }
        register_exception_subclass(registry, row.id, row.name, ancestors);
    }
    // A guard for future edits: `Exception` must be the first exception id.
    debug_assert_eq!(EXCEPTION_CLASSES[0].id, EXCEPTION_CLASS);
}

/// Install ONE exception class's registry entry plus the native `Exception`
/// method set on its own id: the shared `RubyException` constructor and the six
/// `Exception` methods (`initialize`/`message`/`to_s`/`backtrace`/`full_message`/
/// `inspect`), plus StopIteration's `result`/`__set_result` when the class
/// descends from it. Shared by `register_exceptions` (the built-in hierarchy,
/// from `with_core`) and, for D3, by generated `main()` for each USER
/// `class MyErr < StandardError` -- unifying user exception subclasses onto the
/// same native `RubyException` rather than a divergent generated struct. The
/// subclass's own `def`s then `define_method` OVER these defaults (an override)
/// or beside them (an addition). `ancestors` is the full linearized chain, so
/// `carries_result` is decided the same way for a user `class Done < StopIteration`
/// as for the built-in tree.
pub fn register_exception_subclass(
    registry: &mut ClassRegistry,
    id: ClassId,
    name: &str,
    ancestors: Vec<ClassId>,
) {
    let carries_result = ancestors.contains(&STOP_ITERATION_ID);
    registry.register(
        id,
        name,
        false,
        ancestors,
        Some(exception_construct as ConstructorFn),
    );
    // Flat dispatch: every class needs the full materialized method set on
    // its own id (the same shape the compiler emits per generated class).
    registry.define_method(id, Symbol::intern("initialize"), exc_initialize);
    registry.define_method(id, Symbol::intern("message"), exc_message);
    registry.define_method(id, Symbol::intern("to_s"), exc_to_s);
    registry.define_method(id, Symbol::intern("backtrace"), exc_backtrace);
    registry.define_method(id, Symbol::intern("full_message"), exc_full_message);
    registry.define_method(id, Symbol::intern("inspect"), exc_inspect);
    if carries_result {
        registry.define_method(id, Symbol::intern("__set_result"), stop_set_result);
        registry.define_method(id, Symbol::intern("result"), stop_result);
    }
}

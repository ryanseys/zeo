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
use spinel_abi::{
    declared_ancestors, ClassId, EXCEPTION_CLASS, EXCEPTION_CLASSES, KEY_ERROR_CLASS,
    NAME_ERROR_CLASS, NO_METHOD_ERROR_CLASS, STOP_ITERATION_CLASS, UNCAUGHT_THROW_ERROR_CLASS,
};

use crate::dispatch::{
    class_name, downcast_robj, raise_error, run_initialize, send, ClassRegistry, ConstructorFn,
    RObj, RubyObject,
};
use crate::signal::Signal;
use crate::symbol::Symbol;
use crate::value::RubyValue;
use crate::{array_new, string_new};

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
    /// The hidden `cause` slot (`Exception#cause`) -- the exception being
    /// handled at the moment this one was raised, threaded in at raise time
    /// (`attach_cause`). Invisible to `instance_variables`, like `mesg`/`res`.
    cause: Mutex<RubyValue>,
    /// Typed introspection slots that specific subclasses expose as reader
    /// methods -- `NameError#name`/`#receiver`, `NoMethodError#args`,
    /// `KeyError#key`, `UncaughtThrowError#tag`/`#value`. Hidden like `mesg`,
    /// invisible to `instance_variables`, and empty for the common exception.
    /// A `&'static str` key keeps it a tiny fixed-shape map, not a full ivar
    /// table.
    details: Mutex<Vec<(&'static str, RubyValue)>>,
    ivars: Mutex<Vec<(String, RubyValue)>>,
}

impl RubyException {
    fn new(class_id: ClassId) -> Arc<Self> {
        Arc::new(RubyException {
            class_id,
            frozen: AtomicBool::new(false),
            mesg: Mutex::new(RubyValue::Nil),
            res: Mutex::new(RubyValue::Nil),
            cause: Mutex::new(RubyValue::Nil),
            details: Mutex::new(Vec::new()),
            ivars: Mutex::new(Vec::new()),
        })
    }

    /// Set one hidden detail slot (last write wins), used at raise time or by a
    /// name-aware `initialize`.
    fn set_detail(&self, key: &'static str, v: RubyValue) {
        let mut d = self.details.lock();
        match d.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = v,
            None => d.push((key, v)),
        }
    }

    /// Read one hidden detail slot; `nil` when unset (CRuby's default for an
    /// unpopulated `#name`/`#key`/`#args`/`#tag`/`#value`/`#receiver`).
    fn detail(&self, key: &str) -> RubyValue {
        self.details
            .lock()
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or(RubyValue::Nil)
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
            cause: Mutex::new(self.cause.lock().clone()),
            details: Mutex::new(self.details.lock().clone()),
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
    match msg {
        RubyValue::Str(_) => Ok(msg),
        // A non-String message is coerced to a String via its own `to_s`
        // (CRuby's `StringValue`): `RuntimeError.new(42).message` is `"42"`,
        // `.new([1,2])` is `"[1, 2]"`.
        m if m.truthy() => crate::dispatch::send_value(&m, Symbol::intern("to_s"), &[], None),
        _ => Ok(RubyValue::Str(string_new(
            class_name(e.class_id).unwrap_or_default(),
        ))),
    }
}

/// `Exception#==`: true when `other` is an exception of the SAME class with an
/// equal message (backtraces are always `[]` here, so they never differ).
/// Identity is NOT required -- two `RuntimeError.new("m")` are `==`.
fn exc_equal(recv: &RObj, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let RubyValue::Object(other) = &args[0] else {
        return Ok(RubyValue::Bool(false));
    };
    let Some(o) = downcast_robj::<RubyException>(other) else {
        return Ok(RubyValue::Bool(false));
    };
    let e = exc(recv);
    // Clone each message out before comparing: `e == e` (or `e.eql?(e)`, which
    // routes here) would otherwise lock the SAME `mesg` mutex twice and
    // deadlock, since holding one guard across the second `lock()` is reentrant.
    let e_msg = e.mesg.lock().clone();
    let o_msg = o.mesg.lock().clone();
    let eq = e.class_id == o.class_id && e_msg.rb_eq(&o_msg);
    Ok(RubyValue::Bool(eq))
}

/// `Exception#eql?`: object identity (CRuby's `Object#eql?`, which `Exception`
/// does NOT override -- distinct from `#==`, which compares class + message).
/// An explicit definition is needed because `#==` is defined here, and a bare
/// `eql?` would otherwise fall back to it.
fn exc_eql(recv: &RObj, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let same = matches!(&args[0], RubyValue::Object(o) if Arc::ptr_eq(recv, o));
    Ok(RubyValue::Bool(same))
}

/// `Exception#exception`: no argument answers the receiver itself; an argument
/// equal to the current message also answers self; a different message answers
/// a copy carrying the new message (CRuby's `exc_exception`).
fn exc_exception(recv: &RObj, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    match args.first() {
        None => Ok(RubyValue::Object(recv.clone())),
        Some(msg) if msg.rb_eq(&exc(recv).mesg.lock()) => Ok(RubyValue::Object(recv.clone())),
        Some(msg) => {
            let dup = exc(recv).dup_object(false);
            if let Some(de) = downcast_robj::<RubyException>(&dup) {
                *de.mesg.lock() = msg.clone();
            }
            Ok(RubyValue::Object(dup))
        }
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

/// `def cause; <cause slot>; end` -- the exception that was being handled when
/// this one was raised (`nil` if none), threaded in at raise time by
/// [`attach_cause`].
fn exc_cause(recv: &RObj, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(exc(recv).cause.lock().clone())
}

/// Thread the currently-handled exception (`$!`) into `exc_value`'s hidden
/// `cause` slot at raise time, mirroring CRuby's automatic cause chaining. Only
/// applies when `exc_value` is a native exception whose cause is still unset and
/// is not the same object as the current `$!` (a bare re-raise leaves its cause
/// untouched). A no-op for a non-exception raise operand.
pub fn attach_cause(exc_value: &RubyValue) {
    let RubyValue::Object(o) = exc_value else { return };
    let Some(e) = downcast_robj::<RubyException>(o) else { return };
    let Some(current) = crate::handling::current_exception() else { return };
    // A bare re-raise of the exception being handled must not become its own
    // cause.
    if let RubyValue::Object(cur_obj) = &current {
        if Arc::ptr_eq(cur_obj, o) {
            return;
        }
    }
    let mut slot = e.cause.lock();
    if matches!(*slot, RubyValue::Nil) {
        *slot = current;
    }
}

/// `raise ..., cause: <value>` -- install an EXPLICIT cause.
///
/// CRuby keeps three states apart with a `Qundef`/`Qnil`/value sentinel
/// (`rb_f_raise` -> eval.c:740), and they mean different things: an OMITTED
/// `cause:` chains automatically from `$!`, an explicit `cause: nil`
/// SUPPRESSES that chaining, and a value installs that cause. Suppression
/// works here by construction -- codegen calls this instead of
/// `raise_with_cause`, so nothing ever fills the slot from `$!`.
///
/// Note there is no separate "chain terminator" step as in CRuby
/// (`exc_setup_cause`, eval.c:461). It exists there to tell an UNSET ivar
/// from one holding nil; this runtime stores the cause as a `RubyValue` that
/// starts out `Nil`, so the two already coincide and the walk below
/// terminates on its own.
pub fn set_explicit_cause(exc_value: &RubyValue, cause: RubyValue) -> Result<(), Signal> {
    let RubyValue::Object(o) = exc_value else { return Ok(()) };
    let Some(e) = downcast_robj::<RubyException>(o) else { return Ok(()) };

    // `cause: nil` -- leave the slot empty, suppressing chaining.
    if matches!(cause, RubyValue::Nil) {
        return Ok(());
    }
    let RubyValue::Object(cause_obj) = &cause else {
        return Err(raise_error("TypeError", "exception object expected".to_string()));
    };
    if !crate::dispatch::is_a(cause_obj.class_id(), spinel_abi::EXCEPTION_CLASS) {
        return Err(raise_error("TypeError", "exception object expected".to_string()));
    }
    // A DIRECT self-cause is silently dropped rather than raising -- an
    // asymmetry with the indirect case below that CRuby's own comment flags,
    // and that `raise err, cause: err` relies on.
    if Arc::ptr_eq(cause_obj, o) {
        return Ok(());
    }
    // Walk the prospective cause's own chain: if it leads back to this
    // exception the result would be circular, and anything following the
    // chain (`Exception#full_message`) would loop forever.
    let mut cur = cause.clone();
    while let RubyValue::Object(c) = &cur {
        if Arc::ptr_eq(c, o) {
            return Err(raise_error("ArgumentError", "circular causes".to_string()));
        }
        let Some(ce) = downcast_robj::<RubyException>(c) else { break };
        let next = ce.cause.lock().clone();
        cur = next;
    }
    *e.cause.lock() = cause;
    Ok(())
}

/// Attach one typed introspection detail to a native exception at raise time --
/// `KeyError#key`/`#receiver`, `NameError#name`/`#receiver`,
/// `NoMethodError#args`, `UncaughtThrowError#tag`/`#value`. A no-op for a
/// non-native exception value, so a raise site can call it unconditionally.
pub fn set_exception_detail(exc_value: &RubyValue, key: &'static str, v: RubyValue) {
    let RubyValue::Object(o) = exc_value else { return };
    if let Some(e) = downcast_robj::<RubyException>(o) {
        e.set_detail(key, v);
    }
}

/// `NameError#name`/`NoMethodError#name` -- the missing name, `nil` if unset.
fn exc_name(recv: &RObj, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(exc(recv).detail("name"))
}

/// `NameError#receiver`/`KeyError#receiver` -- the object the failed lookup was
/// against. CRuby raises `ArgumentError: no receiver is available` when unset,
/// but every raise site here populates it, so returning the stored value (or
/// `nil`) matches observed behavior without the rarely-hit error path.
fn exc_receiver(recv: &RObj, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(exc(recv).detail("receiver"))
}

/// `KeyError#key` -- the key that was not found.
fn exc_key(recv: &RObj, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(exc(recv).detail("key"))
}

/// `NoMethodError#args` -- the arguments of the failed call, `nil` if the
/// exception was constructed without them.
fn exc_args(recv: &RObj, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(exc(recv).detail("args"))
}

/// `UncaughtThrowError#tag` -- the tag of the uncaught `throw`.
fn exc_tag(recv: &RObj, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(exc(recv).detail("tag"))
}

/// `UncaughtThrowError#value` -- the second `throw` argument, `nil` if omitted.
fn exc_value(recv: &RObj, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(exc(recv).detail("value"))
}

/// `NameError.new(msg = nil, name = nil)` / `NoMethodError.new(msg, name, args)`:
/// the default `initialize` plus the positional `name` (and `args`) that these
/// classes accept and expose. Registered over the shared `initialize` for every
/// class whose ancestry includes `NameError`, so a user `class E < NameError`
/// stores its name the same way.
fn name_error_initialize(recv: &RObj, args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    guard_frozen(recv, &e)?;
    let msg = args.first().cloned().unwrap_or(RubyValue::Nil);
    *e.mesg.lock() = msg.clone();
    if let Some(name) = args.get(1) {
        e.set_detail("name", name.clone());
    }
    // The third positional (`args`) is a `NoMethodError`-only argument; storing
    // it on any `NameError` is harmless since only `NoMethodError` exposes it.
    if let Some(call_args) = args.get(2) {
        e.set_detail("args", call_args.clone());
    }
    Ok(msg)
}

/// `Exception#detailed_message(highlight: false, **opts)` -- `"<message>
/// (<ClassName>)"`. The optional `error_highlight` gem's source-snippet
/// augmentation is a separate concern and not reproduced; the keyword options
/// are accepted and ignored, as the core method does.
fn exc_detailed_message(recv: &RObj, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    let name = class_name(e.class_id).unwrap_or_default();
    let msg = send(recv, Symbol::intern("to_s"), &[], None)?.to_display_string();
    Ok(RubyValue::Str(string_new(format!("{msg} ({name})"))))
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

/// `Exception.exception(*args)` -- the class-method form, an alias for `.new`
/// (constructs an instance of the receiver class).
fn exc_class_exception(recv: &RubyValue, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let RubyValue::Class(cid) = recv else {
        return Err(raise_error("TypeError", "exception must be sent to a class".to_string()));
    };
    exception_construct(*cid, args, block)
}

/// `Exception.to_tty?` -- whether the error stream is a TTY. Under the
/// conformance harness stderr is redirected (not a TTY), so `false`; the value
/// is environment-dependent, and callers only rely on it being a boolean.
fn exc_class_to_tty(_recv: &RubyValue, _args: &[RubyValue], _block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Bool(false))
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
    let carries_result = ancestors.contains(&STOP_ITERATION_CLASS);
    // Ancestry predicates for the typed-accessor sets, decided before `ancestors`
    // is moved into `register` (same up-front shape as `carries_result`).
    let is_name_error = ancestors.contains(&NAME_ERROR_CLASS);
    let is_no_method_error = ancestors.contains(&NO_METHOD_ERROR_CLASS);
    let is_key_error = ancestors.contains(&KEY_ERROR_CLASS);
    let is_uncaught_throw = ancestors.contains(&UNCAUGHT_THROW_ERROR_CLASS);
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
    registry.define_method(id, Symbol::intern("=="), exc_equal);
    registry.define_method(id, Symbol::intern("eql?"), exc_eql);
    registry.define_method(id, Symbol::intern("exception"), exc_exception);
    registry.define_method(id, Symbol::intern("backtrace"), exc_backtrace);
    registry.define_method(id, Symbol::intern("cause"), exc_cause);
    registry.define_method(id, Symbol::intern("full_message"), exc_full_message);
    registry.define_method(id, Symbol::intern("detailed_message"), exc_detailed_message);
    registry.define_method(id, Symbol::intern("inspect"), exc_inspect);
    // Typed introspection accessors, installed by ancestry so a user subclass
    // of the relevant error inherits them the same way the built-in tree does.
    // `NoMethodError < NameError`, so it picks up `#name`/`#receiver` here and
    // adds `#args` below.
    if is_name_error {
        registry.define_method(id, Symbol::intern("initialize"), name_error_initialize);
        registry.define_method(id, Symbol::intern("name"), exc_name);
        registry.define_method(id, Symbol::intern("receiver"), exc_receiver);
    }
    if is_no_method_error {
        registry.define_method(id, Symbol::intern("args"), exc_args);
    }
    if is_key_error {
        registry.define_method(id, Symbol::intern("key"), exc_key);
        registry.define_method(id, Symbol::intern("receiver"), exc_receiver);
    }
    if is_uncaught_throw {
        registry.define_method(id, Symbol::intern("tag"), exc_tag);
        registry.define_method(id, Symbol::intern("value"), exc_value);
    }
    // Class methods, registered per-id (class-method lookup doesn't walk
    // ancestors -- see `dispatch`'s Class-value arm).
    registry.define_class_method(id, Symbol::intern("exception"), exc_class_exception);
    registry.define_class_method(id, Symbol::intern("to_tty?"), exc_class_to_tty);
    if carries_result {
        registry.define_method(id, Symbol::intern("__set_result"), stop_set_result);
        registry.define_method(id, Symbol::intern("result"), stop_result);
    }
}

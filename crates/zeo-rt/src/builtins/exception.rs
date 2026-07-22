//! The built-in exception hierarchy, hand-written natively.
//!
//! Every generated program used to embed ~6,600 lines of `ruby_class!`-expanded
//! exception classes (`Exception`, `StandardError`, the whole tree) plus a
//! factory -- ~76% of the smallest program, recompiled cold once per binary.
//! Those classes are FIXED (the same in every program), so they belong compiled
//! once, here. `register_exceptions` installs them into a program's
//! `ClassRegistry` at the ids `zeo-abi` reserves for them (`EXCEPTION_CLASSES`),
//! which the compiler independently assigns the same way and asserts.
//!
//! One native `RubyException` type backs all of them, distinguished by its
//! `class_id`; the six `Exception` methods (`initialize`/`message`/`to_s`/
//! `backtrace`/`full_message`/`inspect`) plus `StopIteration`'s two are shared
//! fn pointers that read the receiver's class dynamically. The compiler keeps the
//! exception HIR (for resolving names, inlining `super`, and materializing user
//! subclasses), so a `class MyError < StandardError` is unchanged -- only the
//! fixed classes themselves move here.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use zeo_abi::{
    ClassId, EXCEPTION_CLASS, EXCEPTION_CLASSES, FROZEN_ERROR_CLASS, INTERRUPT_CLASS,
    KEY_ERROR_CLASS, LOCAL_JUMP_ERROR_CLASS, NAME_ERROR_CLASS, NO_METHOD_ERROR_CLASS,
    SIGNAL_EXCEPTION_CLASS, STOP_ITERATION_CLASS, SYSTEM_EXIT_CLASS, UNCAUGHT_THROW_ERROR_CLASS,
    declared_ancestors,
};

use crate::builtins::{arg_error, type_error};
use crate::dispatch::{
    ClassRegistry, ConstructorFn, RObj, RubyObject, class_name, downcast_robj, run_initialize, send,
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
    /// The hidden backtrace slot -- `None` until this exception is RAISED
    /// (CRuby: `Exception.new.backtrace` is nil), then the formatted frame
    /// lines captured once at first raise ([`attach_backtrace`]; a re-raise
    /// keeps the original). An explicit `raise exc, msg, backtrace` /
    /// `#set_backtrace` overwrites it.
    backtrace: Mutex<Option<Vec<String>>>,
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
            backtrace: Mutex::new(None),
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
    /// unpopulated `#name`/`#args`/`#tag`/`#value`).
    fn detail(&self, key: &str) -> RubyValue {
        self.detail_opt(key).unwrap_or(RubyValue::Nil)
    }

    /// Read one detail slot as `Some(v)` when PRESENT (even if the value is nil)
    /// vs `None` when never set -- the distinction `#key`/`#receiver` need,
    /// since CRuby raises ArgumentError for an unset one but a `nil.foo` miss
    /// genuinely sets `receiver` to nil.
    fn detail_opt(&self, key: &str) -> Option<RubyValue> {
        self.details
            .lock()
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.clone())
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
            // The copy keeps the original's stamped backtrace (CRuby's
            // `init_copy` copies the whole object, backtrace included).
            backtrace: Mutex::new(self.backtrace.lock().clone()),
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
        return Err(crate::dispatch::raise_error_details(
            "FrozenError",
            format!("can't modify frozen {cls}: {inspected}"),
            &[("receiver", RubyValue::Object(recv.clone()))],
        ));
    }
    Ok(())
}

// --- the shared Exception methods -----------------------------------------

/// `def initialize(msg = nil); @message = msg; end`
fn exc_initialize(
    recv: &RObj,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    let msg = args.first().cloned().unwrap_or(RubyValue::Nil);
    guard_frozen(recv, &e)?;
    *e.mesg.lock() = msg.clone();
    Ok(msg)
}

/// `def to_s; message_slot || self.class.name; end` -- reads the hidden `mesg`
/// slot (NOT a `@message` ivar, which a subclass may set independently).
fn exc_to_s(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
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
fn exc_equal(
    recv: &RObj,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
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
fn exc_exception(
    recv: &RObj,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
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
fn exc_message(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    send(recv, Symbol::intern("to_s"), &[], None)
}

/// `Exception#set_backtrace(lines)` -- an Array of Strings / one String
/// installs a custom backtrace; nil CLEARS the slot (so `#backtrace`
/// answers nil again). Returns the argument, CRuby's contract.
fn exc_set_backtrace(
    recv: &RObj,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1);
    if matches!(args[0], RubyValue::Nil) {
        *exc(recv).backtrace.lock() = None;
        return Ok(RubyValue::Nil);
    }
    apply_custom_backtrace(&RubyValue::Object(recv.clone()), &args[0])?;
    Ok(args[0].clone())
}

/// `Exception#backtrace` -- the formatted frame lines stamped at raise
/// time ([`attach_backtrace`]), or nil for a never-raised exception
/// (CRuby's contract).
fn exc_backtrace(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    match exc(recv).backtrace.lock().as_ref() {
        Some(lines) => Ok(RubyValue::Array(array_new(
            lines
                .iter()
                .map(|l| RubyValue::Str(crate::string_new(l.clone())))
                .collect(),
        ))),
        None => Ok(RubyValue::Nil),
    }
}

/// `def cause; <cause slot>; end` -- the exception that was being handled when
/// this one was raised (`nil` if none), threaded in at raise time by
/// [`attach_cause`].
fn exc_cause(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(exc(recv).cause.lock().clone())
}

/// Thread the currently-handled exception (`$!`) into `exc_value`'s hidden
/// `cause` slot at raise time, mirroring CRuby's automatic cause chaining. Only
/// applies when `exc_value` is a native exception whose cause is still unset and
/// is not the same object as the current `$!` (a bare re-raise leaves its cause
/// untouched). A no-op for a non-exception raise operand.
pub fn attach_cause(exc_value: &RubyValue) {
    let RubyValue::Object(o) = exc_value else {
        return;
    };
    let Some(e) = downcast_robj::<RubyException>(o) else {
        return;
    };
    let Some(current) = crate::handling::current_exception() else {
        return;
    };
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

/// Stamp the CURRENT frame stack onto `exc_value`'s hidden backtrace slot
/// at raise time -- once: a re-raise (or a raise of an exception whose
/// backtrace was set explicitly) keeps what it has, CRuby's rule. A no-op
/// for a non-exception operand. Called wherever `attach_cause` is (the
/// runtime raise channel and codegen's raise emission).
pub fn attach_backtrace(exc_value: &RubyValue) {
    let RubyValue::Object(o) = exc_value else {
        return;
    };
    let Some(e) = downcast_robj::<RubyException>(o) else {
        return;
    };
    let mut slot = e.backtrace.lock();
    if slot.is_none() {
        *slot = Some(crate::frames::capture_backtrace());
    }
}

/// `Exception#set_backtrace(lines)` / `raise exc, msg, backtrace`'s storage
/// half: overwrite the slot with the given formatted lines.
pub fn set_backtrace_lines(exc_value: &RubyValue, lines: Vec<String>) {
    let RubyValue::Object(o) = exc_value else {
        return;
    };
    let Some(e) = downcast_robj::<RubyException>(o) else {
        return;
    };
    *e.backtrace.lock() = Some(lines);
}

/// `raise Class, msg, backtrace` -- the third argument installs a CUSTOM
/// backtrace: an Array of Strings (used verbatim), a single String (a
/// one-line backtrace), or nil (no-op: the raise stamps the real stack).
/// Anything else is CRuby's TypeError.
pub fn apply_custom_backtrace(exc_value: &RubyValue, bt: &RubyValue) -> Result<(), Signal> {
    match bt {
        RubyValue::Nil => Ok(()),
        RubyValue::Str(s) => {
            set_backtrace_lines(exc_value, vec![s.lock().to_utf8_lossy().into_owned()]);
            Ok(())
        }
        RubyValue::Array(a) => {
            let mut lines = Vec::new();
            for v in a.lock().iter() {
                match v {
                    RubyValue::Str(s) => lines.push(s.lock().to_utf8_lossy().into_owned()),
                    _ => {
                        return Err(type_error!("backtrace must be an Array of String"));
                    }
                }
            }
            set_backtrace_lines(exc_value, lines);
            Ok(())
        }
        _ => Err(type_error!(
            "backtrace must be an Array of String or a single String"
        )),
    }
}

/// The top-level uncaught-exception report, CRuby's exact shape:
///
/// ```text
/// file.rb:2:in 'Object#inner': boom (RuntimeError)
///         from file.rb:3:in 'Object#outer'
///         from file.rb:5:in '<main>'
/// ```
///
/// (the continuation indent is one TAB). The innermost frame heads the
/// message line; a frame-less exception (a custom empty backtrace)
/// degrades to the bare `msg (Class)` form. A raising user `message`
/// contributes an empty message rather than a crash.
pub fn report_uncaught(exc_value: &RubyValue) {
    let msg = crate::dispatch::send(
        &exc_value.as_object_unchecked(),
        crate::Symbol::intern("message"),
        &[],
        None,
    )
    .and_then(|v| v.try_display_string())
    .unwrap_or_default();
    let cls = crate::builtins::class_name_of(exc_value);
    match backtrace_lines(exc_value) {
        Some(lines) if !lines.is_empty() => {
            eprintln!("{}: {} ({})", lines[0], msg, cls);
            for l in &lines[1..] {
                eprintln!("\tfrom {l}");
            }
        }
        _ => eprintln!("{msg} ({cls})"),
    }
}

/// The raised exception's formatted backtrace lines (`None` = never
/// raised) -- what the top-level uncaught reporter renders.
pub fn backtrace_lines(exc_value: &RubyValue) -> Option<Vec<String>> {
    let RubyValue::Object(o) = exc_value else {
        return None;
    };
    let e = downcast_robj::<RubyException>(o)?;
    let slot = e.backtrace.lock();
    slot.clone()
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
    let RubyValue::Object(o) = exc_value else {
        return Ok(());
    };
    let Some(e) = downcast_robj::<RubyException>(o) else {
        return Ok(());
    };

    // `cause: nil` -- leave the slot empty, suppressing chaining.
    if matches!(cause, RubyValue::Nil) {
        return Ok(());
    }
    let RubyValue::Object(cause_obj) = &cause else {
        return Err(type_error!("exception object expected"));
    };
    if !crate::dispatch::is_a(cause_obj.class_id(), zeo_abi::EXCEPTION_CLASS) {
        return Err(type_error!("exception object expected"));
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
            return Err(arg_error!("circular causes"));
        }
        let Some(ce) = downcast_robj::<RubyException>(c) else {
            break;
        };
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
    let RubyValue::Object(o) = exc_value else {
        return;
    };
    if let Some(e) = downcast_robj::<RubyException>(o) {
        e.set_detail(key, v);
    }
}

/// `NameError#name`/`NoMethodError#name` -- the missing name, `nil` if unset.
fn exc_name(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(exc(recv).detail("name"))
}

/// `NameError#receiver`/`KeyError#receiver` -- the object the failed lookup was
/// against. CRuby raises `ArgumentError: no receiver is available` when it was
/// never set (a manually-built `NameError.new("m")`), while a real miss like
/// `nil.foo` sets it (to nil, here), which still answers nil.
fn exc_receiver(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    match exc(recv).detail_opt("receiver") {
        Some(v) => Ok(v),
        None => Err(arg_error!("no receiver is available")),
    }
}

/// `KeyError#key` -- the key that was not found. `ArgumentError` when unset
/// (`KeyError.new("m").key`), CRuby's behavior.
fn exc_key(recv: &RObj, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    match exc(recv).detail_opt("key") {
        Some(v) => Ok(v),
        None => Err(arg_error!("no key is available")),
    }
}

/// `NoMethodError#args` -- the arguments of the failed call, `nil` if the
/// exception was constructed without them.
fn exc_args(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(exc(recv).detail("args"))
}

/// `NoMethodError#private_call?` -- whether the missing method was invoked
/// function-style (no explicit receiver). False for an ordinary `recv.meth`
/// miss, which is every method_missing zeo raises today.
fn exc_private_call(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Bool(exc(recv).detail("private_call").truthy()))
}

/// `UncaughtThrowError#tag` -- the tag of the uncaught `throw`.
fn exc_tag(recv: &RObj, _args: &[RubyValue], _blk: Option<RubyValue>) -> Result<RubyValue, Signal> {
    Ok(exc(recv).detail("tag"))
}

/// `UncaughtThrowError#value` -- the second `throw` argument, `nil` if omitted.
fn exc_value(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(exc(recv).detail("value"))
}

/// `NameError.new(msg = nil, name = nil)` / `NoMethodError.new(msg, name, args)`:
/// the default `initialize` plus the positional `name` (and `args`) that these
/// classes accept and expose. Registered over the shared `initialize` for every
/// class whose ancestry includes `NameError`, so a user `class E < NameError`
/// stores its name the same way.
fn name_error_initialize(
    recv: &RObj,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
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
    // The fourth positional is `NoMethodError`'s `private_call?` flag.
    if let Some(private_call) = args.get(3) {
        e.set_detail("private_call", private_call.clone());
    }
    Ok(msg)
}

/// `KeyError.new(msg = nil, receiver:, key:)` -- the default message plus the
/// `receiver:`/`key:` keywords the class accepts, which arrive as one trailing
/// options Hash (the G2 convention). Registered over the shared `initialize` for
/// `KeyError` and its subclasses.
fn key_error_initialize(
    recv: &RObj,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    guard_frozen(recv, &e)?;
    // The trailing keyword Hash, if present, is NOT the message.
    let (msg, kw) = match args.last() {
        Some(RubyValue::Hash(h)) => (args.first().filter(|_| args.len() > 1).cloned(), Some(h)),
        _ => (args.first().cloned(), None),
    };
    let msg = msg.unwrap_or(RubyValue::Nil);
    *e.mesg.lock() = msg.clone();
    if let Some(h) = kw {
        let get = |name: &str| crate::hash_get(h, &RubyValue::Symbol(Symbol::intern(name)));
        let key = get("key");
        if !matches!(key, RubyValue::Nil) {
            e.set_detail("key", key);
        }
        let receiver = get("receiver");
        if !matches!(receiver, RubyValue::Nil) {
            e.set_detail("receiver", receiver);
        }
    }
    Ok(msg)
}

/// `SystemExit.new(status = 0, message = "SystemExit")` -- `status` is an
/// Integer, or `true`/`false` (0 / 1). Exposes `#status`/`#success?`.
fn system_exit_initialize(
    recv: &RObj,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    guard_frozen(recv, &e)?;
    let (status, msg) = match args.first() {
        None => (0, None),
        Some(RubyValue::Bool(b)) => (if *b { 0 } else { 1 }, args.get(1).cloned()),
        Some(RubyValue::Int(n)) => (*n, args.get(1).cloned()),
        // A non-status first argument is the message; status defaults to 0.
        Some(other) => (0, Some(other.clone())),
    };
    let message = msg.unwrap_or_else(|| RubyValue::Str(string_new("SystemExit".to_string())));
    *e.mesg.lock() = message.clone();
    e.set_detail("status", RubyValue::Int(status));
    Ok(message)
}

/// `SystemExit#status` -- the exit status (0 when unset).
fn exc_status(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(match exc(recv).detail("status") {
        RubyValue::Int(n) => RubyValue::Int(n),
        _ => RubyValue::Int(0),
    })
}

/// `SystemExit#success?` -- whether the status is 0.
fn exc_success(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Bool(matches!(
        exc(recv).detail("status"),
        RubyValue::Int(0) | RubyValue::Nil
    )))
}

/// `LocalJumpError#reason` -- the jump kind (`:noreason`/`:break`/`:return`/...);
/// `#exit_value` the value carried by the jump.
fn exc_reason(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(match exc(recv).detail("reason") {
        RubyValue::Nil => RubyValue::Symbol(Symbol::intern("noreason")),
        v => v,
    })
}

fn exc_exit_value(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(exc(recv).detail("exit_value"))
}

/// `Exception#detailed_message(highlight: false, **opts)` -- `"<message>
/// (<ClassName>)"`. The optional `error_highlight` gem's source-snippet
/// augmentation is a separate concern and not reproduced; the keyword options
/// are accepted and ignored, as the core method does.
fn exc_detailed_message(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    let name = class_name(e.class_id).unwrap_or_default();
    let msg = send(recv, Symbol::intern("to_s"), &[], None)?.to_display_string();
    Ok(RubyValue::Str(string_new(format!("{msg} ({name})"))))
}

/// `SignalException.new(signo)` / `.new(signo, message)` / `.new(name)` --
/// resolve the signal and set the hidden `signo` slot plus the message. An
/// Integer first argument is a signal number (an optional second argument is the
/// message, else `"SIG<name>"`); a String/Symbol first argument is a signal
/// NAME and must be the only one (a second argument is `ArgumentError`, as in
/// CRuby). An unknown name/number is `ArgumentError`.
fn signal_exception_initialize(
    recv: &RObj,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    guard_frozen(recv, &e)?;
    let (signo, message) = match args.first() {
        Some(RubyValue::Int(i)) => {
            let signo = *i as i32;
            let msg = match args.get(1) {
                Some(m) => m.clone(),
                None => {
                    let name = crate::builtins::signal::name_from_signo(signo)
                        .ok_or_else(|| arg_error!("invalid signal number ({signo})"))?;
                    RubyValue::Str(string_new(format!("SIG{name}")))
                }
            };
            (*i, msg)
        }
        Some(name @ (RubyValue::Str(_) | RubyValue::Symbol(_))) => {
            if args.len() > 1 {
                return Err(arg_error!(
                    "wrong number of arguments (given {}, expected 1)",
                    args.len()
                ));
            }
            let spelled = match name {
                RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
                RubyValue::Symbol(s) => s.name(),
                _ => unreachable!(),
            };
            let signo = crate::builtins::signal::signo_from_name(&spelled).ok_or_else(|| {
                let bare = spelled.strip_prefix("SIG").unwrap_or(&spelled);
                arg_error!("unsupported signal `SIG{bare}'")
            })?;
            let canonical = crate::builtins::signal::name_from_signo(signo).unwrap_or(&spelled);
            (
                signo as i64,
                RubyValue::Str(string_new(format!("SIG{canonical}"))),
            )
        }
        _ => {
            return Err(arg_error!(
                "wrong number of arguments (given 0, expected 1+)"
            ));
        }
    };
    *e.mesg.lock() = message.clone();
    e.set_detail("signo", RubyValue::Int(signo));
    Ok(message)
}

/// `Interrupt.new(message = nil)` -- a `SignalException` pinned to `SIGINT`
/// (signo 2), whose message defaults to `"Interrupt"` (its class name) rather
/// than being derived from the signal, matching CRuby.
fn interrupt_initialize(
    recv: &RObj,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    guard_frozen(recv, &e)?;
    let msg = match args.first() {
        Some(m) if m.truthy() => m.clone(),
        _ => RubyValue::Str(string_new("Interrupt".to_string())),
    };
    *e.mesg.lock() = msg.clone();
    e.set_detail("signo", RubyValue::Int(2));
    Ok(msg)
}

/// `SignalException#signo` -- the signal number.
fn exc_signo(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(exc(recv).detail("signo"))
}

/// `SignalException#signm` -- an alias for `#message` (a DYNAMIC send, so a
/// subclass override of `message`/`to_s` is honored).
fn exc_signm(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    send(recv, Symbol::intern("message"), &[], None)
}

/// `def full_message; self.class.name + ": " + message; end`
fn exc_full_message(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    let name = class_name(e.class_id).unwrap_or_default();
    // Dynamic `to_s` (honors a subclass override), matching `#message`/`#inspect`.
    let msg = send(recv, Symbol::intern("to_s"), &[], None)?.to_display_string();
    // The uncaught-report shape (`file:line:in 'frame': msg (Class)` +
    // tab-indented `from` lines, trailing newline) when a backtrace was
    // stamped -- always the PLAIN rendering (the `highlight:` bold/reverse
    // escapes aren't modeled; the conformance oracle disables highlighting
    // too). A backtrace-less exception keeps the bare `Class: msg` form.
    match e.backtrace.lock().as_ref() {
        Some(lines) if !lines.is_empty() => {
            let mut out = format!("{}: {msg} ({name})\n", lines[0]);
            for l in &lines[1..] {
                out.push_str(&format!("\tfrom {l}\n"));
            }
            Ok(RubyValue::Str(string_new(out)))
        }
        _ => Ok(RubyValue::Str(string_new(format!("{name}: {msg}")))),
    }
}

/// The exception `inspect`: empty message -> the class name; a message with a
/// newline -> `#<Name:<message.inspect>>`; else `#<Name: message>`.
fn exc_inspect(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
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
fn stop_set_result(
    recv: &RObj,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    let v = args.first().cloned().unwrap_or(RubyValue::Nil);
    guard_frozen(recv, &e)?;
    *e.res.lock() = v.clone();
    Ok(v)
}

/// `StopIteration#result` -- the hidden `res` slot (not a `@result` ivar).
fn stop_result(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
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
fn exc_class_exception(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let RubyValue::Class(cid) = recv else {
        return Err(type_error!("exception must be sent to a class"));
    };
    exception_construct(*cid, args, block)
}

/// `Exception.to_tty?` -- whether the error stream is a TTY. Under the
/// conformance harness stderr is redirected (not a TTY), so `false`; the value
/// is environment-dependent, and callers only rely on it being a boolean.
fn exc_class_to_tty(
    _recv: &RubyValue,
    _args: &[RubyValue],
    _block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
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
    let is_signal_exception = ancestors.contains(&SIGNAL_EXCEPTION_CLASS);
    let is_interrupt = ancestors.contains(&INTERRUPT_CLASS);
    let is_local_jump = ancestors.contains(&LOCAL_JUMP_ERROR_CLASS);
    let is_frozen_error = ancestors.contains(&FROZEN_ERROR_CLASS);
    let is_system_exit = ancestors.contains(&SYSTEM_EXIT_CLASS);
    registry.register(
        id,
        name,
        false,
        ancestors,
        Some(exception_construct as ConstructorFn),
    );
    // Flat dispatch AND own-`super`-target rows in one: every class needs
    // the full materialized method set on its own id (the same shape the
    // compiler emits per generated class), and the natives double as the
    // id's own `super` targets (see `ClassEntry::own_impls`).
    registry.define_method_own(id, Symbol::intern("initialize"), exc_initialize);
    registry.define_method_own(id, Symbol::intern("message"), exc_message);
    registry.define_method_own(id, Symbol::intern("to_s"), exc_to_s);
    registry.define_method_own(id, Symbol::intern("=="), exc_equal);
    registry.define_method_own(id, Symbol::intern("eql?"), exc_eql);
    registry.define_method_own(id, Symbol::intern("exception"), exc_exception);
    registry.define_method_own(id, Symbol::intern("backtrace"), exc_backtrace);
    registry.define_method_own(id, Symbol::intern("set_backtrace"), exc_set_backtrace);
    registry.define_method_own(id, Symbol::intern("cause"), exc_cause);
    registry.define_method_own(id, Symbol::intern("full_message"), exc_full_message);
    registry.define_method_own(id, Symbol::intern("detailed_message"), exc_detailed_message);
    registry.define_method_own(id, Symbol::intern("inspect"), exc_inspect);
    // Typed introspection accessors, installed by ancestry so a user subclass
    // of the relevant error inherits them the same way the built-in tree does.
    // `NoMethodError < NameError`, so it picks up `#name`/`#receiver` here and
    // adds `#args` below.
    if is_name_error {
        registry.define_method_own(id, Symbol::intern("initialize"), name_error_initialize);
        registry.define_method_own(id, Symbol::intern("name"), exc_name);
        registry.define_method_own(id, Symbol::intern("receiver"), exc_receiver);
    }
    if is_no_method_error {
        registry.define_method_own(id, Symbol::intern("args"), exc_args);
        registry.define_method_own(id, Symbol::intern("private_call?"), exc_private_call);
    }
    if is_key_error {
        registry.define_method_own(id, Symbol::intern("initialize"), key_error_initialize);
        registry.define_method_own(id, Symbol::intern("key"), exc_key);
        registry.define_method_own(id, Symbol::intern("receiver"), exc_receiver);
    }
    if is_frozen_error {
        registry.define_method_own(id, Symbol::intern("receiver"), exc_receiver);
    }
    if is_local_jump {
        registry.define_method_own(id, Symbol::intern("reason"), exc_reason);
        registry.define_method_own(id, Symbol::intern("exit_value"), exc_exit_value);
    }
    if is_system_exit {
        registry.define_method_own(id, Symbol::intern("initialize"), system_exit_initialize);
        registry.define_method_own(id, Symbol::intern("status"), exc_status);
        registry.define_method_own(id, Symbol::intern("success?"), exc_success);
    }
    if is_uncaught_throw {
        registry.define_method_own(id, Symbol::intern("tag"), exc_tag);
        registry.define_method_own(id, Symbol::intern("value"), exc_value);
    }
    if is_signal_exception {
        // `Interrupt` pins SIGINT and defaults its message to the class name, so
        // it takes a distinct `initialize`; both expose `#signo`/`#signm`.
        let ctor = if is_interrupt {
            interrupt_initialize
        } else {
            signal_exception_initialize
        };
        registry.define_method_own(id, Symbol::intern("initialize"), ctor);
        registry.define_method_own(id, Symbol::intern("signo"), exc_signo);
        registry.define_method_own(id, Symbol::intern("signm"), exc_signm);
    }
    // Class methods, registered per-id (class-method lookup doesn't walk
    // ancestors -- see `dispatch`'s Class-value arm).
    registry.define_class_method(id, Symbol::intern("exception"), exc_class_exception);
    registry.define_class_method(id, Symbol::intern("to_tty?"), exc_class_to_tty);
    if carries_result {
        registry.define_method_own(id, Symbol::intern("__set_result"), stop_set_result);
        registry.define_method_own(id, Symbol::intern("result"), stop_result);
    }
}

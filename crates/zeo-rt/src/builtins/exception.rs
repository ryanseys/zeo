//! The built-in exception hierarchy, hand-written natively.
//!
//! The exception classes (`Exception`, `StandardError`, the whole tree) are
//! FIXED -- identical in every program -- so they are compiled once here
//! instead of embedding the `ruby_class!`-expanded classes and a factory into
//! every binary, which dominated the size of a small program. `register_exceptions` installs them into a program's
//! `ClassRegistry` at the ids `zeo-abi` reserves for them (`EXCEPTION_CLASSES`),
//! which the compiler independently assigns the same way and asserts.
//!
//! One native `RubyException` type backs all of them, distinguished by its
//! `class_id`; the six `Exception` methods (`initialize`/`message`/`to_s`/
//! `backtrace`/`full_message`/`inspect`) plus `StopIteration`'s two are shared
//! fn pointers that read the receiver's class dynamically. The compiler keeps the
//! exception HIR (for resolving names, inlining `super`, and materializing user
//! subclasses), so a `class MyError < StandardError` works normally; only the
//! fixed classes live here.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use zeo_abi::{
    ClassId, ERRNO_ALIASES, ERRNO_CLASSES, ERRNO_MODULE, EXCEPTION_CLASS, EXCEPTION_CLASSES,
    FROZEN_ERROR_CLASS, INTERRUPT_CLASS, KEY_ERROR_CLASS, LOAD_ERROR_CLASS, LOCAL_JUMP_ERROR_CLASS,
    NAME_ERROR_CLASS, NO_METHOD_ERROR_CLASS, SIGNAL_EXCEPTION_CLASS, STOP_ITERATION_CLASS,
    SYSTEM_CALL_ERROR_CLASS, SYSTEM_EXIT_CLASS, UNCAUGHT_THROW_ERROR_CLASS, declared_ancestors,
    errno_class,
};

use crate::builtins::{arg_error, type_error};
use crate::dispatch::{
    ClassRegistry, ConstructorFn, RObj, RubyObject, class_name, downcast_robj, run_initialize, send,
};
use crate::method_meta::ParamKind;
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
    crate::builtins::check_arity(args.len(), 1, Some(1))?;
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
    if let RubyValue::Object(cur_obj) = &current
        && Arc::ptr_eq(cur_obj, o)
    {
        return;
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
    attach_backtrace_quiet(exc_value);
    // Every raise channel passes through here exactly once (a RE-raise
    // passes again, and fires again -- CRuby's rule), making it
    // `TracePoint`'s `:raise` choke point. After the stamp, so the handler
    // sees a backtraced exception. `stamp_backtrace` -- CONSTRUCTION, not
    // a raise -- takes the quiet path below instead.
    #[cfg(feature = "ext-tracepoint")]
    if crate::ext::tracepoint::tracing() {
        crate::ext::tracepoint::fire_raise(exc_value);
    }
}

/// [`attach_backtrace`]'s stamp without the `:raise` event, for the one
/// caller that is not a raise: `stamp_backtrace`, which runs at exception
/// CONSTRUCTION (codegen's `emit_boxed_new`) just before the raise path
/// stamps -- and fires -- for the same exception.
pub(crate) fn attach_backtrace_quiet(exc_value: &RubyValue) {
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
    report_exception(exc_value, None);
}

/// [`report_uncaught`]'s body, with an optional line printed ahead of it --
/// what `Thread`'s at-termination report puts there
/// (`#<Thread:0x… f.rb:4 run> terminated with exception (...)`). Emitting the
/// pair as ONE write keeps another thread's report from interleaving into the
/// middle of it.
pub(crate) fn report_exception(exc_value: &RubyValue, preamble: Option<&str>) {
    use std::io::IsTerminal;
    let mut out = String::new();
    if let Some(preamble) = preamble {
        out.push_str(preamble);
        out.push('\n');
    }
    // The same renderer `Exception#full_message` runs -- head line, `from`
    // trail, then the cause chain. An exception carrying NO backtrace hangs
    // off nothing here (there is no reporting method to name, and by the time
    // the top level reports there is no frame left to read a position from).
    render_exception(
        exc_value,
        None,
        std::io::stderr().is_terminal(),
        false,
        &mut out,
    );
    eprint!("{out}");
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

/// `LoadError#path` -- the feature that would not load, `nil` if unset (what
/// CRuby answers for a hand-built `LoadError.new("m")`, rather than the error
/// an unset `#key` raises). Backed by a REAL `@path` ivar, not a hidden detail
/// slot: CRuby lists it in `instance_variables`.
fn exc_path(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(recv.ivar_get_named("path").unwrap_or(RubyValue::Nil))
}

/// Stamp `@path` on a `LoadError` at raise time. A no-op for a non-native
/// exception value, so a raise site can call it unconditionally.
pub fn set_load_error_path(exc_value: &RubyValue, path: &str) {
    if let RubyValue::Object(o) = exc_value {
        o.ivar_set_named("path", RubyValue::Str(crate::string_new(path.to_string())));
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

/// `Ractor::RemoteError#ractor` -- see the install site for why this is
/// always nil here.
fn exc_ractor(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(exc(recv).detail_opt("ractor").unwrap_or(RubyValue::Nil))
}

/// Record a failed conversion's encoding pair and offending input on the
/// exception it raises, so `Encoding::UndefinedConversionError#error_char` and
/// `InvalidByteSequenceError#error_bytes` can read them back. Called from
/// `encoding::transcode_signal`, the one place both errors are built.
pub fn attach_transcode_detail(exc: &RubyValue, detail: &crate::enc::TranscodeDetail) {
    let RubyValue::Object(o) = exc else { return };
    let Some(e) = downcast_robj::<RubyException>(o) else {
        return;
    };
    let enc = |id| crate::builtins::encoding::encoding_value(id);
    e.set_detail("source_encoding", enc(detail.source));
    e.set_detail("destination_encoding", enc(detail.destination));
    e.set_detail(
        "error_bytes",
        RubyValue::Str(crate::string_from_bytes(
            detail.error_bytes.clone(),
            detail.source,
        )),
    );
    e.set_detail(
        "error_char",
        match detail.error_char {
            Some(c) => RubyValue::Str(crate::string_new(c.to_string())),
            None => RubyValue::Nil,
        },
    );
    e.set_detail("incomplete_input", RubyValue::Bool(detail.incomplete));
}

/// One of the two encoding slots as an `Encoding`, or as its NAME for the
/// `*_name` twin. Both are nil when the exception came from anywhere but a
/// transcode (a user `raise Encoding::UndefinedConversionError`).
fn exc_encoding(recv: &RObj, key: &str, as_name: bool) -> Result<RubyValue, Signal> {
    let v = exc(recv).detail(key);
    if !as_name {
        return Ok(v);
    }
    match v {
        RubyValue::Nil => Ok(RubyValue::Nil),
        enc => crate::dispatch::send_value(&enc, Symbol::intern("name"), &[], None),
    }
}

fn exc_source_encoding(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    exc_encoding(recv, "source_encoding", false)
}

fn exc_source_encoding_name(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    exc_encoding(recv, "source_encoding", true)
}

fn exc_destination_encoding(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    exc_encoding(recv, "destination_encoding", false)
}

fn exc_destination_encoding_name(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    exc_encoding(recv, "destination_encoding", true)
}

fn exc_error_bytes(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(exc(recv).detail("error_bytes"))
}

fn exc_error_char(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(exc(recv).detail("error_char"))
}

fn exc_incomplete_input(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Bool(
        exc(recv).detail("incomplete_input").truthy(),
    ))
}

/// `InvalidByteSequenceError#readagain_bytes` -- the bytes AFTER the bad
/// sequence that a converter would re-feed. zeo's transcoder converts a whole
/// string in one pass and never resumes, so there is nothing put back -- which
/// is the nil CRuby answers for a one-shot `String#encode` too.
fn exc_readagain_bytes(
    _recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Nil)
}

/// `NoMatchingPatternKeyError#matchee` -- the Hash a `key:` pattern asked of.
/// Unset is an `ArgumentError`, the shape `KeyError#key` already uses.
fn exc_matchee(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    match exc(recv).detail_opt("matchee") {
        Some(v) => Ok(v),
        None => Err(arg_error!("no matchee is available")),
    }
}

/// `NoMatchingPatternKeyError.new(matchee:, key:)` -- CRuby takes both as
/// keywords and leaves the message the class name, so the pair travels on the
/// exception rather than in its text.
fn pattern_key_error_initialize(
    recv: &RObj,
    args: &[RubyValue],
    blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    if let Some(RubyValue::Hash(h)) = args.first() {
        let e = exc(recv);
        for name in ["matchee", "key"] {
            let v = crate::hash_get(h, &RubyValue::Symbol(Symbol::intern(name)));
            if !matches!(v, RubyValue::Nil) {
                e.set_detail(
                    match name {
                        "matchee" => "matchee",
                        _ => "key",
                    },
                    v,
                );
            }
        }
        // The keyword Hash is not the message; CRuby leaves that the class name.
        return exc_initialize(recv, &[], blk);
    }
    exc_initialize(recv, args, blk)
}

/// `FrozenError.new(msg = nil, receiver: nil)` -- unlike the pattern-key pair,
/// the positional MESSAGE still counts here, so the trailing keyword Hash is
/// split off and the rest handed to `Exception#initialize` unchanged.
fn frozen_error_initialize(
    recv: &RObj,
    args: &[RubyValue],
    blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let mut positional = args;
    if let Some(RubyValue::Hash(h)) = args.last() {
        let v = crate::hash_get(h, &RubyValue::Symbol(Symbol::intern("receiver")));
        if !matches!(v, RubyValue::Nil) {
            exc(recv).set_detail("receiver", v);
            positional = &args[..args.len() - 1];
        }
    }
    exc_initialize(recv, positional, blk)
}

/// `Exception#backtrace_locations` -- `#backtrace`'s object form. The stored
/// lines are the ones `frames::format_frame` wrote, so parsing them back is
/// exact rather than a guess; storing the triples twice would cost every raise
/// for the sake of a rarely-read accessor.
fn exc_backtrace_locations(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let Some(lines) = exc(recv).backtrace.lock().clone() else {
        return Ok(RubyValue::Nil);
    };
    let locations = lines
        .iter()
        .map(|line| {
            // `<path>:<lineno>:in '<label>'`, and a path may itself contain a
            // colon, so the split runs from the RIGHT.
            let (head, label) = match line.split_once(":in '") {
                Some((head, rest)) => (head, rest.trim_end_matches('\'')),
                None => (line.as_str(), ""),
            };
            let (path, lineno) = match head.rsplit_once(':') {
                Some((p, n)) => (p, n.parse::<u32>().unwrap_or(0)),
                None => (head, 0),
            };
            crate::builtins::backtrace_location::location_new(path, lineno, label)
        })
        .collect();
    Ok(RubyValue::Array(array_new(locations)))
}

/// Which exception class CRuby files each native on -- see the call site.
/// Every other id carries the same rows for dispatch and lists none of them,
/// exactly as a CRuby subclass with an empty body does.
fn mark_owned_names(registry: &mut ClassRegistry, id: ClassId) {
    /// One owning class and the names it declares. The id is a THUNK because a
    /// `ClassId` const is not usable in a const initializer here.
    type Owned = (fn() -> ClassId, &'static [&'static str]);
    const BY_OWNER: &[Owned] = &[
        (
            || EXCEPTION_CLASS,
            &[
                "message",
                "to_s",
                "==",
                "exception",
                "backtrace",
                "backtrace_locations",
                "set_backtrace",
                "cause",
                "full_message",
                "detailed_message",
                "inspect",
                "respond_to?",
            ],
        ),
        (
            || NAME_ERROR_CLASS,
            &["name", "receiver", "local_variables"],
        ),
        (|| NO_METHOD_ERROR_CLASS, &["args", "private_call?"]),
        (|| KEY_ERROR_CLASS, &["key", "receiver"]),
        (|| FROZEN_ERROR_CLASS, &["receiver"]),
        (|| zeo_abi::RACTOR_REMOTE_ERROR_CLASS, &["ractor"]),
        (|| LOAD_ERROR_CLASS, &["path"]),
        (|| zeo_abi::SYNTAX_ERROR_CLASS, &["path"]),
        (|| SYSTEM_CALL_ERROR_CLASS, &["errno"]),
        (|| LOCAL_JUMP_ERROR_CLASS, &["reason", "exit_value"]),
        (|| SYSTEM_EXIT_CLASS, &["status", "success?"]),
        (|| UNCAUGHT_THROW_ERROR_CLASS, &["to_s", "tag", "value"]),
        (|| SIGNAL_EXCEPTION_CLASS, &["signm", "signo"]),
        (|| STOP_ITERATION_CLASS, &["result"]),
        (
            || zeo_abi::NO_MATCHING_PATTERN_KEY_ERROR_CLASS,
            &["key", "matchee"],
        ),
        (
            || zeo_abi::UNDEFINED_CONVERSION_ERROR_CLASS,
            &[
                "source_encoding",
                "source_encoding_name",
                "destination_encoding",
                "destination_encoding_name",
                "error_char",
            ],
        ),
        (
            || zeo_abi::INVALID_BYTE_SEQUENCE_ERROR_CLASS,
            &[
                "source_encoding",
                "source_encoding_name",
                "destination_encoding",
                "destination_encoding_name",
                "error_bytes",
                "readagain_bytes",
                "incomplete_input?",
            ],
        ),
    ];
    for (owner, names) in BY_OWNER {
        if owner() != id {
            continue;
        }
        for name in *names {
            registry.mark_own(id, Symbol::intern(name));
        }
    }
    // `Exception`'s three PRIVATE rows. The mark goes on every id -- flat
    // dispatch put a row on each, and without it
    // `RuntimeError.new.respond_to?(:initialize)` answers true. Only
    // `Exception` OWNS `method_missing`/`respond_to_missing?`, so only it
    // lists those in `private_instance_methods(false)`.
    for name in ["initialize", "method_missing", "respond_to_missing?"] {
        let sym = Symbol::intern(name);
        registry.mark_private(id, sym);
        if id == EXCEPTION_CLASS && name != "initialize" {
            registry.mark_own(id, sym);
        }
    }
    // `initialize` is different: every class below takes ARGUMENTS `Exception`
    // does not, so CRuby declares one on each and reflection has to say so.
    // (`Interrupt` pins SIGINT, `KeyError` takes `key:`/`receiver:`, and so
    // on -- each already has its own body registered further down.)
    const OWN_INITIALIZE: &[fn() -> ClassId] = &[
        || EXCEPTION_CLASS,
        || FROZEN_ERROR_CLASS,
        || zeo_abi::INTERRUPT_CLASS,
        || KEY_ERROR_CLASS,
        || NAME_ERROR_CLASS,
        || zeo_abi::NO_MATCHING_PATTERN_KEY_ERROR_CLASS,
        || NO_METHOD_ERROR_CLASS,
        || SIGNAL_EXCEPTION_CLASS,
        || zeo_abi::SYNTAX_ERROR_CLASS,
        || SYSTEM_CALL_ERROR_CLASS,
        || SYSTEM_EXIT_CLASS,
        || UNCAUGHT_THROW_ERROR_CLASS,
    ];
    if OWN_INITIALIZE.iter().any(|owner| owner() == id) {
        registry.mark_own(id, Symbol::intern("initialize"));
    }
}

/// The `#arity`/`#parameters` shape of a hand-registered exception CLASS
/// method. These bypass the `ruby_class!` DSL, so they reach no arity table --
/// and once the owner mark below is in place, the accidental answer they used
/// to get from `Module`'s row is gone too. Parameters are unnamed, the way
/// CRuby reports every C method's.
fn class_method_signature(id: ClassId, name: &str, kinds: &[crate::method_meta::ParamKind]) {
    crate::method_meta::MethodMeta::singleton(id.0, name)
        .with_params(kinds.iter().map(|&k| (k, None)).collect())
        .register();
}

/// `NameError#local_variables` -- CRuby fills this with the caller's locals at
/// the point a bare name missed. zeo raises from native code, which has no Ruby
/// scope to walk, so the list is empty.
fn exc_local_variables(
    _recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Array(crate::array_new(Vec::new())))
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

/// `UncaughtThrowError.new(tag, value, msg = ...)` -- unlike a plain
/// Exception it REQUIRES the tag and value (2..3 args); `.new`/`.new(:t)`
/// raise ArgumentError. Stores `tag`/`value` and defaults the message to
/// `uncaught throw <tag.inspect>`.
fn uncaught_throw_initialize(
    recv: &RObj,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    guard_frozen(recv, &e)?;
    // The internal `throw` path constructs the error with a single String
    // message (`raise_error_details` -> `construct_exception`, which then sets
    // `tag`/`value` itself); accept that form. A genuine `.new`/`.new(tag)`
    // (0 or 1 non-String arg) is the ArgumentError CRuby raises.
    let internal_msg_only = matches!(args, [RubyValue::Str(_)]);
    if !internal_msg_only && !(2..=3).contains(&args.len()) {
        return Err(arg_error!(
            "wrong number of arguments (given {}, expected 2..3)",
            args.len()
        ));
    }
    if args.len() >= 2 {
        e.set_detail("tag", args[0].clone());
        e.set_detail("value", args[1].clone());
    }
    match args.get(2) {
        // An explicit message wins and is stored in the `mesg` slot.
        Some(m) => {
            *e.mesg.lock() = m.clone();
            Ok(m.clone())
        }
        None if internal_msg_only => {
            *e.mesg.lock() = args[0].clone();
            Ok(args[0].clone())
        }
        // `.new(tag, value)` with no explicit message: leave `mesg` nil so the
        // message is derived from the tag by `uncaught_throw_to_s` and equality
        // stays tag-agnostic, matching the uncaught-throw raise path.
        None => Ok(RubyValue::Nil),
    }
}

/// Build (and package as a raise `Signal`) the `UncaughtThrowError` an uncaught
/// `throw` raises. The internal message slot is left NIL: CRuby's `exc_equal`
/// compares that slot, so two uncaught-throw errors raised on the same source
/// line compare equal even though their tags (and rendered messages) differ.
/// The human-readable "uncaught throw <tag>" text is produced on demand by
/// [`uncaught_throw_to_s`]. `tag`/`value` populate the hidden accessor slots,
/// and cause + backtrace are attached at raise time (as for any raise).
pub fn raise_uncaught_throw(tag: RubyValue, value: RubyValue) -> Signal {
    let e = RubyException::new(UNCAUGHT_THROW_ERROR_CLASS);
    e.set_detail("tag", tag);
    e.set_detail("value", value);
    let exc = RubyValue::Object(e);
    attach_cause(&exc);
    attach_backtrace(&exc);
    Signal::Raise(exc)
}

/// `UncaughtThrowError#to_s`: an explicit message (`.new(tag, val, "m")`) wins;
/// otherwise the message is derived from the tag as `uncaught throw <tag>` --
/// keeping the internal `mesg` slot nil so equality compares tag-agnostically
/// (see [`raise_uncaught_throw`]). `#message` routes here via `to_s`.
fn uncaught_throw_to_s(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    if e.mesg.lock().truthy() {
        return exc_to_s(recv, &[], None);
    }
    let tag = e.detail("tag");
    Ok(RubyValue::Str(string_new(format!(
        "uncaught throw {}",
        tag.inspect_string()
    ))))
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

/// `SystemCallError#initialize(msg = nil, errno = nil)`, which composes the
/// message rather than storing it: `strerror(errno)`, then `" - #{msg}"` when a
/// message came in. The errno is the explicit argument, else the one the class
/// itself names (`Errno::ENOENT::Errno`), else none -- and with none CRuby says
/// "unknown error" rather than asking `strerror`.
///
/// `SystemCallError.new` also RETURNS a subclass when the errno names one; that
/// half happens in [`exception_construct`], which picks the class before
/// allocating.
fn syscall_error_initialize(
    recv: &RObj,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    guard_frozen(recv, &e)?;
    let (msg, errno) = syscall_error_args(e.class_id, args);
    let mut text = match errno {
        Some(n) => strerror(n),
        None => "unknown error".to_string(),
    };
    if let Some(msg) = msg {
        let msg = crate::builtins::convert::to_rstr(&msg)?;
        text.push_str(" - ");
        text.push_str(&msg.lock().to_utf8_lossy());
    }
    // Only a bare `SystemCallError` needs the slot: every `Errno::` class
    // answers `#errno` from its own constant.
    if let Some(n) = errno.filter(|_| e.class_id == SYSTEM_CALL_ERROR_CLASS) {
        e.set_detail("errno", RubyValue::Int(i64::from(n)));
    }
    let message = RubyValue::Str(string_new(text));
    *e.mesg.lock() = message.clone();
    Ok(message)
}

/// Split `SystemCallError.new`'s arguments into `(message, errno)`. A lone
/// Integer is the ERRNO on `SystemCallError` itself (`SystemCallError.new(2)`
/// is an `Errno::ENOENT`) but a message on a subclass, whose errno is already
/// settled -- CRuby raises TypeError for `Errno::ENOENT.new(2)`, which the
/// `to_str` conversion in the caller reproduces.
fn syscall_error_args(class: ClassId, args: &[RubyValue]) -> (Option<RubyValue>, Option<i32>) {
    let own = zeo_abi::errno_of_class(class);
    match (args.first(), args.get(1)) {
        (Some(RubyValue::Int(n)), None) if own.is_none() => (None, Some(*n as i32)),
        (first, second) => {
            let msg = first.filter(|v| !matches!(v, RubyValue::Nil)).cloned();
            let errno = match second {
                Some(RubyValue::Int(n)) => Some(*n as i32),
                _ => own,
            };
            (msg, errno)
        }
    }
}

/// The platform's own text for `errno` -- CRuby calls `strerror` too, so an
/// unnamed value reads exactly as it does there ("Unknown error: 9999").
pub(crate) fn strerror(errno: i32) -> String {
    // SAFETY: `strerror` returns a pointer to a static (or thread-local)
    // NUL-terminated string that stays valid until the next call on this
    // thread; the copy happens before returning, so nothing outlives it.
    unsafe {
        let p = libc::strerror(errno);
        if p.is_null() {
            return format!("Unknown error: {errno}");
        }
        std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}

/// `SystemCallError#errno` -- the value the class names, or the one a bare
/// `SystemCallError.new(msg, n)` was handed. `nil` when neither applies.
fn exc_errno(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    if let Some(n) = zeo_abi::errno_of_class(e.class_id) {
        return Ok(RubyValue::Int(i64::from(n)));
    }
    Ok(e.detail("errno"))
}

/// Put `msg` in the message slot as given, past the `SystemCallError`
/// initialize that would have prepended `strerror` to it. See
/// `ClassRegistry::construct_exception`, its only caller.
pub(crate) fn set_verbatim_message(exc: &RubyValue, msg: String) {
    if let RubyValue::Object(o) = exc
        && let Some(e) = downcast_robj::<RubyException>(o)
    {
        *e.mesg.lock() = RubyValue::Str(string_new(msg));
    }
}

/// Seed each `Errno` class's own `Errno` constant (the value it stands for)
/// and every second spelling `zeo-abi::ERRNO_ALIASES` names. The aliases are
/// CONSTANTS rather than classes: the compiler resolves `Errno::EWOULDBLOCK` to
/// `Errno::EAGAIN` statically, so without these the name would answer a
/// `rescue` but never appear in `Errno.constants`.
pub fn seed_errno_constants() {
    for row in ERRNO_CLASSES {
        let id = zeo_abi::errno_class_id(row.name);
        crate::constants::const_set(id.0, "Errno", RubyValue::Int(i64::from(row.errno)));
    }
    for (alias, target) in ERRNO_ALIASES {
        let bare = alias.rsplit("::").next().unwrap_or(alias);
        let id = zeo_abi::errno_class_id(target);
        crate::constants::const_set(ERRNO_MODULE.0, bare, RubyValue::Class(id));
    }
    // The readiness classes follow the same errno, so `IO` carries the same
    // second spellings and needs the same treatment.
    for (alias, target) in [
        ("EWOULDBLOCKWaitReadable", "IO::EAGAINWaitReadable"),
        ("EWOULDBLOCKWaitWritable", "IO::EAGAINWaitWritable"),
    ] {
        let Some(id) = crate::dispatch::class_id_by_name(target) else {
            continue;
        };
        crate::constants::const_set(zeo_abi::IO_CLASS.0, alias, RubyValue::Class(id));
    }
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

/// CRuby's ANSI decorations (`eval_error.c:81`): bold for the message,
/// underline for the class name, reset for both.
const BOLD: &str = "\x1b[1m";
const UNDERLINE: &str = "\x1b[1;4m";
const RESET: &str = "\x1b[m";

/// One keyword out of a call's trailing options Hash.
fn opt_kw(args: &[RubyValue], name: &str) -> Option<RubyValue> {
    let RubyValue::Hash(h) = args.last()? else {
        return None;
    };
    let key = RubyValue::Symbol(Symbol::intern(name));
    crate::hash_pairs(h)
        .into_iter()
        .find(|(k, _)| k.rb_eq(&key))
        .map(|(_, v)| v)
}

/// `highlight:` -- true/false/absent, anything else is CRuby's own refusal.
/// Absent means "decorate only for a terminal", which the conformance oracle
/// runs without, so it resolves to false here for the same reason.
fn highlight_kw(args: &[RubyValue]) -> Result<bool, Signal> {
    match opt_kw(args, "highlight") {
        None | Some(RubyValue::Nil) => {
            use std::io::IsTerminal;
            Ok(std::io::stderr().is_terminal())
        }
        Some(RubyValue::Bool(b)) => Ok(b),
        Some(other) => Err(arg_error!(
            "expected true or false as highlight: {}",
            other.inspect_string()
        )),
    }
}

/// `order:` -- `:top` (the default) puts the failing frame first; `:bottom`
/// prints CRuby's numbered `Traceback (most recent call last):` block.
fn reverse_kw(args: &[RubyValue]) -> Result<bool, Signal> {
    match opt_kw(args, "order") {
        None | Some(RubyValue::Nil) => Ok(false),
        Some(RubyValue::Symbol(s)) if s.name() == "top" => Ok(false),
        Some(RubyValue::Symbol(s)) if s.name() == "bottom" => Ok(true),
        Some(other) => Err(arg_error!(
            "expected :top or :bottom as order: {}",
            other.inspect_string()
        )),
    }
}

/// CRuby's `rb_decorate_message` (`eval_error.c:128`): the `detailed_message`
/// body, and the head of every rendered report. An EMPTY message prints the
/// class name alone -- `unhandled exception` for a bare `RuntimeError`, which
/// is what an argumentless `raise` builds. An ANONYMOUS class (its name spells
/// `#<Class:...>`) contributes no ` (Class)` tag, and under `highlight:` never
/// closes the bold it opened -- CRuby's own asymmetry, reproduced. A
/// multi-line message keeps the tag on its FIRST line and runs the rest
/// underneath, each line bolded on its own.
fn decorate_message(class_id: ClassId, msg: &str, highlight: bool) -> String {
    let (bold, under, reset) = if highlight {
        (BOLD, UNDERLINE, RESET)
    } else {
        ("", "", "")
    };
    let name = class_name(class_id).unwrap_or_default();
    if msg.is_empty() {
        let bare = if class_id == zeo_abi::RUNTIME_ERROR_CLASS {
            "unhandled exception"
        } else {
            name.as_str()
        };
        return format!("{under}{bare}{reset}");
    }
    let (head, tail) = match msg.split_once('\n') {
        Some((head, tail)) => (head, Some(tail)),
        None => (msg, None),
    };
    let mut out = format!("{bold}{head}");
    if !name.starts_with('#') {
        out.push_str(&format!(" ({under}{name}{reset}{bold}){reset}"));
    }
    if let Some(tail) = tail {
        out.push('\n');
        if highlight {
            let mut first = true;
            for line in tail.split('\n') {
                if !first {
                    out.push('\n');
                }
                first = false;
                if !line.is_empty() {
                    out.push_str(&format!("{BOLD}{line}{RESET}"));
                }
            }
        } else {
            out.push_str(tail);
        }
    }
    out
}

/// `Exception#detailed_message(highlight: nil, **opts)`. The optional
/// `error_highlight` gem's source-snippet augmentation is a separate concern
/// and not reproduced; the remaining keyword options are accepted and ignored,
/// as the core method does.
fn exc_detailed_message(
    recv: &RObj,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let e = exc(recv);
    let msg = send(recv, Symbol::intern("to_s"), &[], None)?.to_display_string();
    Ok(RubyValue::Str(string_new(decorate_message(
        e.class_id,
        &msg,
        highlight_kw(args)?,
    ))))
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

/// `Exception#respond_to?` -- Kernel's answer, re-declared. CRuby owns a row
/// here because `Exception` also carries a private `method_missing`, and the
/// override keeps that hook out of the question; the ANSWER is Kernel's, so
/// this routes straight to it rather than restating the protocol.
fn exc_respond_to(
    recv: &RObj,
    args: &[RubyValue],
    blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::inherited_row!(
        kernel,
        "respond_to?",
        &RubyValue::Object(recv.clone()),
        args,
        blk
    )
}

/// `Exception`'s own private `method_missing` and `respond_to_missing?`, the
/// other two thirds of its `private_instance_methods(false)`. CRuby declares
/// both here rather than inheriting them, and both answer exactly what the root
/// pair answers -- so each routes to that root instead of restating it, the way
/// `exc_respond_to` routes to Kernel's.
fn exc_method_missing(
    recv: &RObj,
    args: &[RubyValue],
    blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::inherited_row!(
        basic_object,
        "method_missing",
        &RubyValue::Object(recv.clone()),
        args,
        blk
    )
}

fn exc_respond_to_missing(
    recv: &RObj,
    args: &[RubyValue],
    blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::inherited_row!(
        kernel,
        "respond_to_missing?",
        &RubyValue::Object(recv.clone()),
        args,
        blk
    )
}

/// `SignalException#signo` -- the signal number.
fn exc_signo(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    Ok(exc(recv).detail("signo"))
}

/// `SignalException#signm` -- the signal's name. It sends `to_s`, exactly as
/// the DEFAULT `#message` does, so it steps AROUND a subclass override of
/// `message` while still honoring one of `to_s`.
fn exc_signm(
    recv: &RObj,
    _args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    send(recv, Symbol::intern("to_s"), &[], None)
}

/// CRuby's `print_errinfo` (`eval_error.c:86`): the position the report hangs
/// off, then the decorated message. The position is the exception's innermost
/// backtrace entry -- or, when it carries none, the CALLER's own source
/// position under the name of the method doing the reporting (`error_pos`),
/// which is why `StandardError.new("x").full_message` names `full_message`.
fn print_errinfo(exc_value: &RubyValue, at: Option<&str>, highlight: bool, out: &mut String) {
    let RubyValue::Object(o) = exc_value else {
        return;
    };
    // SENT, not computed: `rb_get_detailed_message` dispatches, so a subclass
    // overriding `detailed_message` changes what every report of it prints --
    // which is the seam `error_highlight` and `did_you_mean` hang off.
    let opts = crate::collections::hash_new(vec![(
        RubyValue::Symbol(Symbol::intern("highlight")),
        RubyValue::Bool(highlight),
    )]);
    crate::collections::hash_mark_kwargs(&opts);
    let msg = send(
        o,
        Symbol::intern("detailed_message"),
        &[RubyValue::Hash(opts)],
        None,
    )
    .and_then(|v| v.try_display_string())
    .unwrap_or_default();
    let lines = backtrace_lines(exc_value).unwrap_or_default();
    if let Some(pos) = lines.first().map(String::as_str).or(at) {
        out.push_str(&format!("{pos}: "));
    }
    out.push_str(&msg);
    out.push('\n');
}

/// CRuby's `print_backtrace` (`eval_error.c:219`): the `from` trail below the
/// head line, skipping the entry the head already showed. `:bottom` order
/// walks it outward-in and numbers each line, right-aligned to the widest
/// index -- the `Traceback (most recent call last):` block.
fn print_backtrace(exc_value: &RubyValue, reverse: bool, out: &mut String) {
    let lines = backtrace_lines(exc_value).unwrap_or_default();
    let len = lines.len();
    let width = if len <= 1 {
        0
    } else {
        (len - 1).to_string().len()
    };
    for i in 1..len {
        let line = &lines[if reverse { len - i } else { i }];
        if reverse {
            out.push_str(&format!("\t{:>width$}: from {line}\n", len - i));
        } else {
            out.push_str(&format!("\tfrom {line}\n"));
        }
    }
}

/// CRuby's `show_cause` (`eval_error.c:290`): every exception in the `cause`
/// chain is rendered after (or, in `:bottom` order, before) the one it caused.
/// `set_explicit_cause` already refuses a circular chain, so the walk
/// terminates without a seen-set.
fn show_cause(
    exc_value: &RubyValue,
    at: Option<&str>,
    highlight: bool,
    reverse: bool,
    out: &mut String,
) {
    let RubyValue::Object(o) = exc_value else {
        return;
    };
    let Some(e) = downcast_robj::<RubyException>(o) else {
        return;
    };
    let cause = e.cause.lock().clone();
    if !matches!(cause, RubyValue::Object(_)) {
        return;
    }
    write_exception(&cause, at, highlight, reverse, out);
}

/// CRuby's `rb_error_write0` (`eval_error.c:323`) -- the one renderer behind
/// `Exception#full_message` and the uncaught-exception report. `:top` order
/// reads head, trail, cause; `:bottom` reads them backwards, under a
/// `Traceback` banner.
fn render_exception(
    exc_value: &RubyValue,
    at: Option<&str>,
    highlight: bool,
    reverse: bool,
    out: &mut String,
) {
    if reverse {
        out.push_str(&if highlight {
            format!("{BOLD}Traceback{RESET} (most recent call last):\n")
        } else {
            "Traceback (most recent call last):\n".to_string()
        });
    }
    write_exception(exc_value, at, highlight, reverse, out);
}

/// The banner-less body, which is also what each `cause` recurses into -- one
/// `Traceback` heads the whole report, however long the chain under it is.
fn write_exception(
    exc_value: &RubyValue,
    at: Option<&str>,
    highlight: bool,
    reverse: bool,
    out: &mut String,
) {
    if reverse {
        show_cause(exc_value, at, highlight, reverse, out);
        print_backtrace(exc_value, true, out);
        print_errinfo(exc_value, at, highlight, out);
    } else {
        print_errinfo(exc_value, at, highlight, out);
        print_backtrace(exc_value, false, out);
        show_cause(exc_value, at, highlight, reverse, out);
    }
}

/// `Exception#full_message(highlight: nil, order: :top)`.
fn exc_full_message(
    recv: &RObj,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let highlight = highlight_kw(args)?;
    let reverse = reverse_kw(args)?;
    let mut out = String::new();
    render_exception(
        &RubyValue::Object(recv.clone()),
        error_pos("full_message").as_deref(),
        highlight,
        reverse,
        &mut out,
    );
    Ok(RubyValue::Str(string_new(out)))
}

/// CRuby's `error_pos_str` (`eval_error.c:39`): `file:line:in 'callee': ` for
/// the frame the report is being asked from, which is what an exception with
/// no backtrace of its own hangs off.
fn error_pos(callee: &str) -> Option<String> {
    let (file, line) = crate::frames::current_location()?;
    if line == 0 {
        return Some(file.to_string());
    }
    Some(format!("{file}:{line}:in '{callee}'"))
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
///
/// `SystemCallError` is the one class that does not get the class it was asked
/// for: an errno argument picks the matching `Errno::` subclass, so
/// `SystemCallError.new("x", 2)` IS an `Errno::ENOENT`, as in CRuby. The class
/// has to be decided here rather than in `initialize`, since a
/// `RubyException`'s class is fixed at allocation.
pub(crate) fn exception_construct(
    class: ClassId,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // Normalized to `(message, errno)` so the subclass reads the errno out of
    // the second slot: `SystemCallError.new(2)` and `SystemCallError.new(nil, 2)`
    // are the same call, and `Errno::ENOENT.new(2)` -- where the lone Integer IS
    // a message, and a bad one -- must keep raising TypeError.
    let mut redirected = None;
    if class == SYSTEM_CALL_ERROR_CLASS {
        let (msg, errno) = syscall_error_args(class, args);
        if let Some((id, row)) = errno.and_then(errno_class) {
            redirected = Some((
                id,
                [
                    msg.unwrap_or(RubyValue::Nil),
                    RubyValue::Int(i64::from(row.errno)),
                ],
            ));
        }
    }
    let (class, args) = match &redirected {
        Some((id, args)) => (*id, args.as_slice()),
        None => (class, args),
    };
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

/// `SystemCallError.===(other)` -- `rescue Errno::ENOENT` matches on the ERRNO
/// NUMBER, not on the class, so any object answering that number matches. The
/// bare `SystemCallError` matches every one of its instances outright.
///
/// The receiver's own `Errno` constant carries the number to compare. On
/// `SystemCallError` itself that name reaches the top-level `Errno` MODULE
/// through `Object`, which equals no integer -- so the duck-typed arm answers
/// false there, exactly as CRuby's does.
fn syscall_error_eqq(
    recv: &RubyValue,
    args: &[RubyValue],
    _block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::check_arity(args.len(), 1, Some(1))?;
    let RubyValue::Class(cid) = recv else {
        return Err(type_error!("=== must be sent to a class"));
    };
    let other = &args[0];
    let errno = Symbol::intern("errno");
    if crate::dispatch::is_a_value(other, SYSTEM_CALL_ERROR_CLASS) {
        if *cid == SYSTEM_CALL_ERROR_CLASS {
            return Ok(RubyValue::Bool(true));
        }
    } else if !crate::dispatch::responds_to_or_missing(other, errno, false)? {
        return Ok(RubyValue::Bool(false));
    }
    let Some(want) = crate::constants::const_get(cid.0, "Errno") else {
        return Ok(RubyValue::Bool(false));
    };
    let got = crate::dispatch::send_value(other, errno, &[], None)?;
    crate::dispatch::send_value(&got, Symbol::intern("=="), &[want], None)
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
/// `ClassRegistry::with_core`, so the fixed classes install here instead of
/// emitting `ruby_class!` blocks into each program. Ancestors
/// come from the single core-class linearizer (`declared_ancestors`), so
/// `rescue`/`is_a?` agree with the compiler's own materialized `ancestors`.
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
    let is_load_error = ancestors.contains(&LOAD_ERROR_CLASS);
    let is_system_exit = ancestors.contains(&SYSTEM_EXIT_CLASS);
    let is_system_call_error = ancestors.contains(&SYSTEM_CALL_ERROR_CLASS);
    let is_undefined_conversion = ancestors.contains(&zeo_abi::UNDEFINED_CONVERSION_ERROR_CLASS);
    let is_pattern_key_error = ancestors.contains(&zeo_abi::NO_MATCHING_PATTERN_KEY_ERROR_CLASS);
    let is_invalid_byte_sequence = ancestors.contains(&zeo_abi::INVALID_BYTE_SEQUENCE_ERROR_CLASS);
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
    registry.define_method_own(
        id,
        Symbol::intern("backtrace_locations"),
        exc_backtrace_locations,
    );
    registry.define_method_own(id, Symbol::intern("set_backtrace"), exc_set_backtrace);
    registry.define_method_own(id, Symbol::intern("cause"), exc_cause);
    registry.define_method_own(id, Symbol::intern("full_message"), exc_full_message);
    registry.define_method_own(id, Symbol::intern("detailed_message"), exc_detailed_message);
    registry.define_method_own(id, Symbol::intern("inspect"), exc_inspect);
    registry.define_method_own(id, Symbol::intern("respond_to?"), exc_respond_to);
    registry.define_method_own(id, Symbol::intern("method_missing"), exc_method_missing);
    registry.define_method_own(
        id,
        Symbol::intern("respond_to_missing?"),
        exc_respond_to_missing,
    );
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
        registry.define_method_own(id, Symbol::intern("initialize"), frozen_error_initialize);
        registry.define_method_own(id, Symbol::intern("receiver"), exc_receiver);
    }
    // `Ractor::RemoteError#ractor` -- the ractor whose failure was relayed.
    // zeo runs no ractors, so this exception is never raised here and the
    // slot is never filled; the reader answers nil, exactly as CRuby's does
    // for a hand-constructed one.
    if id == zeo_abi::RACTOR_REMOTE_ERROR_CLASS {
        registry.define_method_own(id, Symbol::intern("ractor"), exc_ractor);
    }
    if is_load_error {
        registry.define_method_own(id, Symbol::intern("path"), exc_path);
    }
    // `SyntaxError#path` reads the same slot `LoadError#path` does -- the file
    // whose parse failed. The two are unrelated in CRuby's tree and share only
    // the accessor's shape.
    if id == zeo_abi::SYNTAX_ERROR_CLASS {
        registry.define_method_own(id, Symbol::intern("path"), exc_path);
    }
    // The two conversion errors, whose accessors read the encoding pair and
    // the offending input `transcode_signal` attached.
    if is_undefined_conversion || is_invalid_byte_sequence {
        registry.define_method_own(id, Symbol::intern("source_encoding"), exc_source_encoding);
        registry.define_method_own(
            id,
            Symbol::intern("source_encoding_name"),
            exc_source_encoding_name,
        );
        registry.define_method_own(
            id,
            Symbol::intern("destination_encoding"),
            exc_destination_encoding,
        );
        registry.define_method_own(
            id,
            Symbol::intern("destination_encoding_name"),
            exc_destination_encoding_name,
        );
    }
    if is_undefined_conversion {
        registry.define_method_own(id, Symbol::intern("error_char"), exc_error_char);
    }
    if is_invalid_byte_sequence {
        registry.define_method_own(id, Symbol::intern("error_bytes"), exc_error_bytes);
        registry.define_method_own(id, Symbol::intern("readagain_bytes"), exc_readagain_bytes);
        registry.define_method_own(
            id,
            Symbol::intern("incomplete_input?"),
            exc_incomplete_input,
        );
    }
    // `NameError#local_variables` -- the caller's locals at the point of the
    // miss, which CRuby fills in for a bare-name NameError. zeo raises from
    // native code with no scope to walk, so the list is empty.
    if is_name_error {
        registry.define_method_own(id, Symbol::intern("local_variables"), exc_local_variables);
    }
    // Flat dispatch installs every native on EVERY exception id, which is what
    // makes `super` and the ancestor walk work -- but reflection has to answer
    // the class CRuby OWNS each one on, or `MyError.instance_methods(false)`
    // would report Exception's twelve. So the listing is told separately, and
    // only on the owning id.
    mark_owned_names(registry, id);
    if is_pattern_key_error {
        registry.define_method_own(
            id,
            Symbol::intern("initialize"),
            pattern_key_error_initialize,
        );
        registry.define_method_own(id, Symbol::intern("key"), exc_key);
        registry.define_method_own(id, Symbol::intern("matchee"), exc_matchee);
    }
    if is_system_call_error {
        registry.define_method_own(id, Symbol::intern("initialize"), syscall_error_initialize);
        registry.define_method_own(id, Symbol::intern("errno"), exc_errno);
        // On every descendant, not just the owner: class-method lookup does
        // not walk ancestors, so `Errno::ENOENT === x` needs its own row.
        registry.define_class_method(id, Symbol::intern("==="), syscall_error_eqq);
        if id == SYSTEM_CALL_ERROR_CLASS {
            registry.mark_own_class_method(id, Symbol::intern("==="));
            class_method_signature(id, "===", &[ParamKind::Req]);
        }
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
        registry.define_method_own(id, Symbol::intern("initialize"), uncaught_throw_initialize);
        registry.define_method_own(id, Symbol::intern("to_s"), uncaught_throw_to_s);
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
    // ancestors -- see `dispatch`'s Class-value arm). The OWN mark goes on
    // `Exception` alone, the same split `mark_own` makes on the instance side:
    // every id needs a row to dispatch, only the owner may list one in
    // `singleton_methods(false)`.
    registry.define_class_method(id, Symbol::intern("exception"), exc_class_exception);
    registry.define_class_method(id, Symbol::intern("to_tty?"), exc_class_to_tty);
    if id == EXCEPTION_CLASS {
        registry.mark_own_class_method(id, Symbol::intern("exception"));
        registry.mark_own_class_method(id, Symbol::intern("to_tty?"));
        class_method_signature(id, "exception", &[ParamKind::Rest]);
        class_method_signature(id, "to_tty?", &[]);
    }
    if carries_result {
        registry.define_method_own(id, Symbol::intern("__set_result"), stop_set_result);
        registry.define_method_own(id, Symbol::intern("result"), stop_result);
    }
}

// The `(key, matchee)` of the most recent hash-pattern KEY miss.
//
// A pattern compiles to one boolean, so by the time the `expr => pattern` arm
// raises, the expression tree that knew which key was absent is gone. The miss
// is therefore recorded where it happens and read back at the raise. Thread-
// local because a pattern match is entirely within one thread, and cleared at
// the start of every required match so a stale record from an earlier one --
// or from a `case/in` arm that simply did not apply -- cannot leak into it.
thread_local! {
    static PATTERN_KEY_MISS: std::cell::RefCell<Option<(RubyValue, RubyValue)>> =
        const { std::cell::RefCell::new(None) };
    static PATTERN_FAIL: std::cell::RefCell<Option<PatternFail>> =
        const { std::cell::RefCell::new(None) };
}

/// WHY a sub-pattern rejected the value, in the shapes ruby's
/// `NoMatchingPatternError` message is built from. The detail is the whole
/// diagnostic: a `case/in` with nested sub-patterns gives no other clue about
/// which sub-test rejected the value, and ruby's message points straight at it
/// (Bug #17925) rather than staying generic.
pub enum PatternFail {
    /// `String === 1 does not return true`
    CaseEq(RubyValue, RubyValue),
    /// `[1] length mismatch (given 1, expected 2)`, `2+` with a rest.
    Length {
        matchee: RubyValue,
        expected: usize,
        open: bool,
    },
    /// `#<Object> does not respond to #deconstruct[_keys]`
    NoDeconstruct { matchee: RubyValue, keys: bool },
    /// `{a: 1} is not empty` / `rest of {b: 2} is not empty`
    NotEmpty { matchee: RubyValue, rest: bool },
    /// `[1, 2, 3] does not match to find pattern`
    Find(RubyValue),
    /// `guard clause does not return true`
    Guard,
}

impl PatternFail {
    fn message(&self) -> String {
        match self {
            PatternFail::CaseEq(pattern, matchee) => format!(
                "{} === {} does not return true",
                pattern.inspect_string(),
                matchee.inspect_string()
            ),
            PatternFail::Length {
                matchee,
                expected,
                open,
            } => {
                let given = match matchee {
                    RubyValue::Array(a) => a.lock().len(),
                    _ => 0,
                };
                let plus = if *open { "+" } else { "" };
                format!(
                    "{} length mismatch (given {given}, expected {expected}{plus})",
                    matchee.inspect_string()
                )
            }
            PatternFail::NoDeconstruct { matchee, keys } => {
                let suffix = if *keys { "_keys" } else { "" };
                format!(
                    "{} does not respond to #deconstruct{suffix}",
                    matchee.inspect_string()
                )
            }
            PatternFail::NotEmpty { matchee, rest } => {
                if *rest {
                    format!("rest of {} is not empty", matchee.inspect_string())
                } else {
                    format!("{} is not empty", matchee.inspect_string())
                }
            }
            PatternFail::Find(matchee) => {
                format!(
                    "{} does not match to find pattern",
                    matchee.inspect_string()
                )
            }
            PatternFail::Guard => "guard clause does not return true".to_string(),
        }
    }
}

/// Arm a required match: forget any earlier miss.
pub fn pattern_key_miss_clear() {
    PATTERN_KEY_MISS.with(|m| *m.borrow_mut() = None);
    PATTERN_FAIL.with(|m| *m.borrow_mut() = None);
}

/// Record why a sub-pattern rejected its value. Recorded on FAILURE only, so a
/// matching pattern pays nothing, and the LAST record before the raise is the
/// one reported -- which is where ruby's own error-string slot ends up, since
/// it overwrites the slot per sub-test as the match walks.
pub fn pattern_fail_record(fail: PatternFail) {
    PATTERN_FAIL.with(|m| *m.borrow_mut() = Some(fail));
}

pub fn pattern_fail_case_eq(pattern: &RubyValue, matchee: &RubyValue) {
    pattern_fail_record(PatternFail::CaseEq(pattern.clone(), matchee.clone()));
}

pub fn pattern_fail_length(matchee: &RubyValue, expected: usize, open: bool) {
    pattern_fail_record(PatternFail::Length {
        matchee: matchee.clone(),
        expected,
        open,
    });
}

pub fn pattern_fail_deconstruct(matchee: &RubyValue, keys: bool) {
    pattern_fail_record(PatternFail::NoDeconstruct {
        matchee: matchee.clone(),
        keys,
    });
}

pub fn pattern_fail_not_empty(matchee: &RubyValue, rest: bool) {
    pattern_fail_record(PatternFail::NotEmpty {
        matchee: matchee.clone(),
        rest,
    });
}

pub fn pattern_fail_find(matchee: &RubyValue) {
    pattern_fail_record(PatternFail::Find(matchee.clone()));
}

pub fn pattern_fail_guard() {
    pattern_fail_record(PatternFail::Guard);
}

/// A `case/in` with MORE THAN ONE `in` clause names only the value: with
/// several branches, each with its own sub-patterns, no single failing test
/// describes the whole match, so ruby declines to guess and reports the
/// subject alone. It is also the plain parent class there, never the key error.
pub fn pattern_match_error_bare(subject: &RubyValue) -> crate::Signal {
    crate::dispatch::raise_error("NoMatchingPatternError", subject.inspect_string())
}

/// Record that `key` was absent from `matchee`. The LAST miss wins, which is
/// what ruby reports for a nested pattern: `{a: {b: 1}} => {a: {c:}}` names
/// `:c` and the INNER hash, not `:a` and the outer one.
pub fn pattern_key_miss_record(key: &RubyValue, matchee: &RubyValue) {
    PATTERN_KEY_MISS.with(|m| *m.borrow_mut() = Some((key.clone(), matchee.clone())));
}

/// The exception an `expr => pattern` raises. A recorded key miss makes it
/// `NoMatchingPatternKeyError`, carrying the key and the hash it was asked of;
/// anything else stays the parent class. `subject` is the OUTERMOST subject,
/// which is what ruby puts in the message even when `matchee` is a nested hash.
pub fn pattern_match_error(subject: &RubyValue) -> crate::Signal {
    let miss = PATTERN_KEY_MISS.with(|m| m.borrow_mut().take());
    let fail = PATTERN_FAIL.with(|m| m.borrow_mut().take());
    let Some((key, matchee)) = miss else {
        // `"%p: %s"` -- the subject, then the sentence naming the sub-test
        // that rejected it. Without a recorded reason the subject stands
        // alone, which is also what ruby prints.
        let message = match fail {
            Some(f) => format!("{}: {}", subject.inspect_string(), f.message()),
            None => subject.inspect_string(),
        };
        return crate::dispatch::raise_error("NoMatchingPatternError", message);
    };
    let message = format!(
        "{}: key not found: {}",
        subject.inspect_string(),
        key.inspect_string()
    );
    let exc = crate::dispatch::construct_exception_value("NoMatchingPatternKeyError", &message);
    set_exception_detail(&exc, "key", key);
    set_exception_detail(&exc, "matchee", matchee);
    crate::Signal::Raise(crate::stamp_backtrace(exc))
}

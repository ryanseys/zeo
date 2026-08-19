//! Exception construction and the raise channels: `raise_error` (THE runtime
//! raise channel), arity errors, `method_missing`, `NameError` construction,
//! cause/backtrace stamping, and the raise-argument coercions. Near-zero
//! inbound coupling with the rest of `dispatch` beyond the registry it
//! constructs exception objects through.

use super::*;

/// THE runtime raise channel: a rescuable `Signal::Raise` carrying a
/// `class_name` exception once the registry is installed (every generated
/// program), a loud panic otherwise (this crate's own unit tests, which run
/// registry-less). The registry constructs the object itself via the class's
/// registered `ConstructorFn` -- see `ClassRegistry::construct_exception`.
pub fn raise_error(class_name: &str, msg: String) -> Signal {
    let msg = arity_debug_context(msg);
    match REGISTRY.get() {
        Some(reg) => {
            let exc = reg.construct_exception(class_name, msg);
            crate::builtins::exception::attach_cause(&exc);
            crate::builtins::exception::attach_backtrace(&exc);
            Signal::Raise(exc)
        }
        None => panic!("{class_name}: {msg}"),
    }
}

/// [`arity_error`] for a signature whose expected count is a RANGE or an
/// open-ended minimum -- `"1..3"`, `"2+"` -- already spelled by codegen.
///
/// `#[cold]`, and deliberately not `#[inline]`: every trampoline and every
/// lambda emits a call to this on its error leg, and each used to carry ~35
/// tokens of `format!` machinery that can only run when the program is about
/// to raise. `emit_dynamic_trampoline` runs once per (class x visible method)
/// over the FLATTENED ancestry -- 22,623 entries behind 3,624 definitions for
/// activemodel -- so the machinery was multiplying through inheritance.
#[cold]
pub fn wrong_arity(given: usize, expected: &str) -> Signal {
    raise_error(
        "ArgumentError",
        format!("wrong number of arguments (given {given}, expected {expected})"),
    )
}

/// The fixed-arity `ArgumentError` (`zeo_tramp!`'s error leg): one call in
/// the generated program where a `format!` used to be.
pub fn arity_error(given: usize, expected: usize) -> Signal {
    raise_error(
        "ArgumentError",
        format!("wrong number of arguments (given {given}, expected {expected})"),
    )
}

/// [`raise_error`] plus typed introspection details stamped onto the freshly
/// built exception -- `KeyError#key`/`#receiver`, `NameError#name`/`#receiver`,
/// `NoMethodError#name`/`#args`/`#receiver`. The message is built the same way;
/// each `(slot, value)` is attached before cause chaining so the reader methods
/// see it.
pub fn raise_error_details(
    class_name: &str,
    msg: String,
    details: &[(&'static str, RubyValue)],
) -> Signal {
    let msg = arity_debug_context(msg);
    match REGISTRY.get() {
        Some(reg) => {
            let exc = reg.construct_exception(class_name, msg);
            for (slot, value) in details {
                crate::builtins::exception::set_exception_detail(&exc, slot, value.clone());
            }
            crate::builtins::exception::attach_cause(&exc);
            crate::builtins::exception::attach_backtrace(&exc);
            Signal::Raise(exc)
        }
        None => panic!("{class_name}: {msg}"),
    }
}

/// The `LocalJumpError` a `yield` with no block raises, carrying `#reason`
/// `:noreason` (CRuby's reason for a missing block). Shared by every
/// no-block-yield site so the accessor is populated uniformly.
pub fn raise_no_block_yield() -> Signal {
    raise_error_details(
        "LocalJumpError",
        "no block given (yield)".to_string(),
        &[("reason", RubyValue::Symbol(Symbol::intern("noreason")))],
    )
}

/// Build (do not raise) a `NameError` VALUE carrying the typed `#name` (a
/// Symbol) and `#receiver` an `uninitialized constant` reference exposes.
/// Codegen emits this for a const miss so the boxed error the branch raises or
/// stores answers `e.name`/`e.receiver`, matching CRuby. `receiver` is
/// `RubyValue::Nil` where the lexical scope isn't statically known (a bare
/// top-level reference), which reads back as `nil` -- the same as an unset slot.
/// An exception VALUE of `class_name` carrying `message`, for a raise site that
/// must set details on it before raising. `raise_error` is the shortcut for
/// every site that does not.
pub fn construct_exception_value(class_name: &str, message: &str) -> RubyValue {
    match REGISTRY.get() {
        Some(reg) => reg.construct_exception(class_name, message.to_string()),
        None => panic!("{class_name}: {message}"),
    }
}

pub fn make_name_error(message: String, name: &str, receiver: RubyValue) -> RubyValue {
    match REGISTRY.get() {
        Some(reg) => {
            let exc = reg.construct_exception("NameError", message);
            crate::builtins::exception::set_exception_detail(
                &exc,
                "name",
                RubyValue::Symbol(Symbol::intern(name)),
            );
            crate::builtins::exception::set_exception_detail(&exc, "receiver", receiver);
            exc
        }
        None => panic!("NameError: {message}"),
    }
}

/// Why a method lookup failed, mirroring CRuby's `method_missing_reason`
/// (`internal/vm.h:32`). CRuby routes every one of these through a SINGLE
/// raiser (`raise_method_missing`, `vm_eval.c:968`) that picks a format string
/// per reason, rather than growing a separate error site per failure mode --
/// so a `super` that finds nothing above its defining class and a
/// `public_send` aimed at a private method produce their messages from one
/// place, and stay in step by construction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MissingReason {
    /// No entry anywhere in the receiver's ancestry.
    NoEntry,
    /// Found, but private and reached with an explicit receiver
    /// (`obj.secret`, or any `public_send`).
    Private,
    /// Found, but protected and the CALLER's `self` isn't a kind of the
    /// method's owner -- a receiver-sensitive test, hence runtime-only.
    Protected,
    /// A bare identifier that resolved to neither a local nor a method.
    /// The one reason that raises `NameError` instead of `NoMethodError`.
    VCall,
    /// `super` found no definition above the defining class.
    Super,
}

impl MissingReason {
    /// The exception class and message template CRuby pairs with this reason
    /// (`vm_eval.c:985-1006`). `{name}` is the method, `{recv}` the
    /// [`describe_receiver`] rendering.
    fn format(self) -> (&'static str, &'static str) {
        match self {
            Self::NoEntry => ("NoMethodError", "undefined method '{name}' for {recv}"),
            Self::Private => ("NoMethodError", "private method '{name}' called for {recv}"),
            Self::Protected => (
                "NoMethodError",
                "protected method '{name}' called for {recv}",
            ),
            Self::VCall => (
                "NameError",
                "undefined local variable or method '{name}' for {recv}",
            ),
            Self::Super => (
                "NoMethodError",
                "super: no superclass method '{name}' for {recv}",
            ),
        }
    }
}

/// CRuby's receiver description for a method-missing message -- the
/// `"%3$s%4$s"` prefix+name pair built in `name_err_mesg_to_str`
/// (`error.c:2660`).
///
/// This deliberately NEVER calls `inspect` on the receiver. None of CRuby's
/// five default formats contains the receiver-inspect directive (`%2$s`), and
/// the formatter only computes one when the format actually asks for it
/// (`error.c:2641`). That is precisely what lets a `BasicObject` subclass --
/// whose blank slate has no `inspect` -- raise a `NoMethodError` describing
/// itself, instead of failing a second lookup while building the message for
/// the first.
pub fn describe_receiver(recv: &RubyValue) -> String {
    // Real Ruby names the class of an anonymous/unregistered receiver with
    // its id rather than raising while building an error message.
    let named = |id: ClassId| class_name(id).unwrap_or_else(|| format!("#<Class:{}>", id.0));
    match recv {
        // nil/true/false render bare -- no "an instance of" prefix.
        RubyValue::Nil => "nil".to_string(),
        RubyValue::Bool(b) => b.to_string(),
        // A class or module receiver gets its own shape ("for class Widget" /
        // "for module Helper"), oracle-verified.
        RubyValue::Class(cid) => {
            let kind = if class_is_module(*cid).unwrap_or(false) {
                "module"
            } else {
                "class"
            };
            format!("{kind} {}", named(*cid))
        }
        // The top-level `self` is rendered literally as `main`
        // (`error.c:2678`), not as `#<Object:0x...>`.
        RubyValue::Object(o) => match main_object() {
            RubyValue::Object(m) if Arc::ptr_eq(&m, o) => "main".to_string(),
            _ => format!("an instance of {}", named(o.class_id())),
        },
        // Every remaining variant maps to a builtin ClassId via the ABI table.
        _ => format!(
            "an instance of {}",
            zeo_abi::builtin_name(recv.class_id()).unwrap_or("Object")
        ),
    }
}

/// THE method-missing raise: CRuby's single `raise_method_missing`
/// (`vm_eval.c:968`), reason-selected message and all.
///
/// Every failed lookup -- a plain miss, a visibility rejection, a bare
/// identifier, a `super` with nothing above it -- ends here, so the five
/// message shapes cannot drift apart.
pub fn raise_method_missing(
    recv: &RubyValue,
    name: &str,
    args: &[RubyValue],
    reason: MissingReason,
) -> Signal {
    let (class_name, template) = reason.format();
    let msg = template
        .replace("{name}", name)
        .replace("{recv}", &describe_receiver(recv));
    // Populate the raised error's introspection slots (`NameError#name`/
    // `#receiver`, `NoMethodError#args`). A `VCall` (bare undefined identifier)
    // is a `NameError` with no call arguments; every other reason is a
    // `NoMethodError` whose `#args` is the failed call's argument list -- `[]`
    // for a zero-arg call, matching CRuby.
    let name_detail = ("name", RubyValue::Symbol(Symbol::intern(name)));
    let receiver_detail = ("receiver", recv.clone());
    match reason {
        MissingReason::VCall => {
            raise_error_details(class_name, msg, &[name_detail, receiver_detail])
        }
        _ => {
            let args_detail = ("args", RubyValue::Array(crate::array_new(args.to_vec())));
            raise_error_details(
                class_name,
                msg,
                &[name_detail, receiver_detail, args_detail],
            )
        }
    }
}

/// Thread the currently-handled exception (`$!`) into `exc`'s `cause` slot
/// (CRuby's automatic cause chaining) and return `exc` unchanged, for use at a
/// `raise` site: `Err(Signal::Raise(raise_with_cause(exc)))`. A no-op for a
/// bare re-raise or a non-exception operand (see `attach_cause`).
pub fn raise_with_cause(exc: RubyValue) -> RubyValue {
    crate::builtins::exception::attach_cause(&exc);
    crate::builtins::exception::attach_backtrace(&exc);
    exc
}

/// The backtrace stamp ALONE -- for codegen's internal error constructors
/// (`emit_boxed_new`), which run at their raise sites but must NOT chain a
/// cause (that is `raise`'s job, and an explicit `cause: nil` SUPPRESSES
/// chaining precisely by never calling `raise_with_cause`).
pub fn stamp_backtrace(exc: RubyValue) -> RubyValue {
    crate::builtins::exception::attach_backtrace_quiet(&exc);
    exc
}

/// Coerce a `raise <value>` operand to the exception value to raise:
/// an Exception object raises itself, an Exception CLASS raises a fresh
/// instance of itself, a String becomes a `RuntimeError` with that message,
/// and anything else is CRuby's `TypeError: exception class/object expected`
/// -- instead of panicking when the raise machinery later unwraps a
/// non-Object. `Exception`'s id is fixed (`zeo-abi`), so codegen need not
/// bake it in.
pub fn coerce_raise_arg(value: RubyValue) -> Result<RubyValue, Signal> {
    let build = |class_name: &str, msg: String| match REGISTRY.get() {
        Some(reg) => reg.construct_exception(class_name, msg),
        None => panic!("{class_name}: {msg}"),
    };
    Ok(match &value {
        RubyValue::Object(o) if is_a(o.class_id(), zeo_abi::EXCEPTION_CLASS) => value,
        // A class reached through a VALUE -- a local, an element of a table of
        // error classes -- rather than a literal name, which codegen builds
        // directly. `#exception` runs any custom `initialize`.
        RubyValue::Class(cid) if is_a(*cid, zeo_abi::EXCEPTION_CLASS) => {
            build_exception(&value, &[])?
        }
        RubyValue::Str(s) => build("RuntimeError", s.lock().to_utf8_lossy().into_owned()),
        // Any object answering #exception may be raised (CRuby's
        // rb_make_exception protocol); the hook must return an Exception.
        _ if responds_to_value(&value, Symbol::intern("exception"), false) => {
            checked_exception_hook(&value, &[])?
        }
        _ => build("TypeError", "exception class/object expected".to_string()),
    })
}

/// `raise <class-or-exception>, message` with a RUNTIME-computed first operand
/// (a variable/call zeo couldn't resolve to a literal class). Mirrors
/// `Kernel#raise`'s two-arg coercion: an Exception CLASS or INSTANCE becomes
/// `obj.exception(msg)` (running any custom `#exception`/`#initialize`), and
/// anything else is CRuby's `TypeError`. Returns the exception value to raise.
pub fn coerce_raise_arg_with_message(
    value: RubyValue,
    msg: RubyValue,
) -> Result<RubyValue, Signal> {
    let msg = std::slice::from_ref(&msg);
    let is_exc = match &value {
        RubyValue::Class(cid) => is_a(*cid, zeo_abi::EXCEPTION_CLASS),
        RubyValue::Object(o) => is_a(o.class_id(), zeo_abi::EXCEPTION_CLASS),
        _ => false,
    };
    if !is_exc {
        // Any object answering #exception may be raised (CRuby's
        // rb_make_exception protocol); the hook must return an Exception.
        if responds_to_value(&value, Symbol::intern("exception"), false) {
            return checked_exception_hook(&value, msg);
        }
        return Err(raise_error(
            "TypeError",
            "exception class/object expected".to_string(),
        ));
    }
    build_exception(&value, msg)
}

/// Dispatch a NON-Exception receiver's `#exception` hook and validate the
/// result: CRuby raises `TypeError: exception object expected` when the hook
/// answers anything that isn't an Exception instance.
fn checked_exception_hook(value: &RubyValue, msg: &[RubyValue]) -> Result<RubyValue, Signal> {
    let built = send_value(value, Symbol::intern("exception"), msg, None)?;
    match &built {
        RubyValue::Object(o) if is_a(o.class_id(), zeo_abi::EXCEPTION_CLASS) => Ok(built),
        _ => Err(raise_error(
            "TypeError",
            "exception object expected".to_string(),
        )),
    }
}

/// `obj.exception(msg)` -- the constructor `raise` reaches an Exception class
/// or instance through. A class MINTED at runtime (`Foo = Class.new(StdErr)`)
/// carries no `exception` of its own, and class-method lookup does not walk
/// ancestors, so it is built through `new` instead; both run the same
/// `initialize`.
fn build_exception(value: &RubyValue, msg: &[RubyValue]) -> Result<RubyValue, Signal> {
    let exception = Symbol::intern("exception");
    // The REACHABILITY test, not `class_method_owner`: the latter is a
    // reflection-grade ancestor scan, and class-method dispatch does not walk
    // ancestors -- so an id an ancestor merely OWNS `exception` on would be
    // sent a call it cannot answer.
    let has_own = match value {
        RubyValue::Class(cid) => class_receiver_responds(*cid, exception),
        _ => true,
    };
    let name = if has_own {
        exception
    } else {
        Symbol::intern("new")
    };
    send_value(value, name, msg, None)
}

/// The exhausted-iteration raise: a rescuable `StopIteration`
/// whose `result` is `result` (a fresh instance per raise -- CRuby rebuilds
/// one from `stop_exc` each time too). Constructs `StopIteration.new(msg)` via
/// the registry, then stamps the result through the exception class's own
/// `__set_result`. Loud panic registry-less (unit tests).
pub fn raise_stop_iteration(result: RubyValue) -> Signal {
    match REGISTRY.get() {
        Some(reg) => {
            let exc =
                reg.construct_exception("StopIteration", "iteration reached an end".to_string());
            send(
                &exc.as_object_unchecked(),
                Symbol::intern("__set_result"),
                std::slice::from_ref(&result),
                None,
            )
            .expect("StopIteration#__set_result can't signal");
            crate::builtins::exception::attach_cause(&exc);
            crate::builtins::exception::attach_backtrace(&exc);
            Signal::Raise(exc)
        }
        None => panic!("StopIteration: iteration reached an end"),
    }
}

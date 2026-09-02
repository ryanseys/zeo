//! Raising: exceptions, `errno`, warnings and arity.
//!
//! Every entry here ends in a raise, and MRI marks most of them
//! `__attribute__((noreturn))` -- which an extension relies on. A C compiler
//! that believes a function returns emits the code after the call; one that
//! knows it does not, does not. So each of these leaves through
//! [`super::jmp`], which longjmps and never comes back.
//!
//! # `errno` is the second thing Rust cannot express
//!
//! `errno` is a macro over a per-thread location, and `rb_errno_ptr` hands
//! out that location's address. There is no way to write that in Rust, so the
//! three entries live in `csrc/cext_va.c` beside the variadic ones, for the
//! same reason: C is where the language feature is.
//!
//! What is here is the mapping from a code to a class. That has to agree with
//! the one every other raise site in the runtime uses, so it calls the same
//! `zeo_abi::errno_class`, and an unmapped code falls back to
//! `SystemCallError` -- which is what CRuby answers too.

use super::convert::{to_value, value_of};
use super::object::{cstr, send};
use super::value::Value;
use std::ffi::{c_char, c_int, c_long};
use zeo_rt::builtins::wrong_arg_type;
use zeo_rt::{RubyValue, Signal};

/// The `Errno::*` class for a code, and the strerror text CRuby prints.
fn errno_row(code: c_int) -> (&'static str, String) {
    match zeo_abi::errno_class(code) {
        Some((_, row)) => (row.name, zeo_rt::builtins::exception::strerror(row.errno)),
        None => ("SystemCallError", "Unknown error".to_string()),
    }
}

/// CRuby's own two message shapes: `"<strerror> - <what>"` when the caller
/// named something to blame, and the bare strerror when it did not.
fn syserr(code: c_int, what: Option<&str>) -> Signal {
    let (class, desc) = errno_row(code);
    match what {
        Some(w) if !w.is_empty() => zeo_rt::dispatch::raise_error(class, format!("{desc} - {w}")),
        _ => zeo_rt::dispatch::raise_error(class, desc),
    }
}

/// The exception OBJECT for a code, for the `_new` entries that do not raise.
fn syserr_value(code: c_int, what: Option<&str>) -> Result<RubyValue, Signal> {
    match syserr(code, what) {
        Signal::Raise(e) => Ok(e),
        other => Err(other),
    }
}

/// # Safety
///
/// `v` must be a live `VALUE`.
unsafe fn text_of(v: Value) -> Result<String, Signal> {
    match unsafe { value_of(v) } {
        RubyValue::Str(s) => Ok(s.lock().to_utf8_lossy().into_owned()),
        RubyValue::Nil => Ok(String::new()),
        other => Err(wrong_arg_type(&other, "String")),
    }
}

/// Build `klass.new(msg)` and hand back the object rather than raising it.
/// An extension uses this to attach state before it raises.
fn exc_new(klass: Value, msg: &str) -> Result<Value, Signal> {
    let k = unsafe { value_of(klass) };
    let text = zeo_rt::builtins::string::str_value_in_enc(zeo_rt::encoding::UTF_8, msg);
    to_value(&send(&k, "new", &[text])?)
}

crate::cext_fn! {
    // ---- building an exception -----------------------------------------

    fn rb_exc_new(klass: Value, p: *const c_char, len: c_long) -> Value {
        let bytes = unsafe { super::string::borrow_bytes(p, len) };
        exc_new(klass, &String::from_utf8_lossy(&bytes))
    }

    fn rb_exc_new_cstr(klass: Value, p: *const c_char) -> Value {
        exc_new(klass, &unsafe { cstr(p) })
    }

    fn rb_exc_new_str(klass: Value, msg: Value) -> Value {
        exc_new(klass, &unsafe { text_of(msg)? })
    }

    // ---- raising -------------------------------------------------------

    /// `rb_exc_fatal`: MRI's unrescuable raise. zeo has no `fatal` class an
    /// extension can name, and `Exception` is the closest thing a `rescue`
    /// without a class list still does not catch.
    fn rb_exc_fatal(exc: Value) -> () {
        Err(Signal::Raise(unsafe { value_of(exc) }))
    }

    fn rb_interrupt() -> () {
        Err(zeo_rt::dispatch::raise_error("Interrupt", String::new()))
    }

    fn rb_memerror() -> () {
        Err(zeo_rt::builtins::no_memory_error!("failed to allocate memory"))
    }

    fn rb_num_zerodiv() -> () {
        Err(zeo_rt::builtins::zero_division_error!("divided by 0"))
    }

    /// `rb_error_arity(argc, min, max)`. `max` is `UNLIMITED_ARGUMENTS` when
    /// there is no ceiling, which MRI spells as `-1`.
    fn rb_error_arity(argc: c_int, min: c_int, max: c_int) -> () {
        let want = if max < 0 {
            format!("{min}+")
        } else if min == max {
            min.to_string()
        } else {
            format!("{min}..{max}")
        };
        Err(zeo_rt::dispatch::wrong_arity(argc.max(0) as usize, &want))
    }

    fn rb_error_frozen(what: *const c_char) -> () {
        Err(zeo_rt::builtins::frozen_error!("can't modify frozen {}", unsafe { cstr(what) }))
    }

    fn rb_error_frozen_object(obj: Value) -> () {
        let v = unsafe { value_of(obj) };
        let shown = send(&v, "inspect", &[])
            .ok()
            .map_or_else(String::new, |s| s.to_display_string());
        Err(zeo_rt::builtins::frozen_error!("can't modify frozen {}: {shown}",
                zeo_rt::dispatch::class_name(v.class_id()).unwrap_or("Object".into())))
    }

    /// `rb_invalid_str(str, type)`: what `Integer("0x")` raises.
    fn rb_invalid_str(text: *const c_char, kind: *const c_char) -> () {
        let (text, kind) = (unsafe { cstr(text) }, unsafe { cstr(kind) });
        Err(zeo_rt::builtins::arg_error!("invalid value for {kind}: \"{text}\""))
    }

    /// `rb_f_notimplement`: the body MRI installs for a method the platform
    /// does not have. It raises whatever it is called as.
    fn rb_f_notimplement(_argc: c_int, _argv: *const Value, _recv: Value, _marker: Value) -> Value {
        Err(zeo_rt::builtins::not_impl_error!("the platform does not support this method"))
    }

    /// `rb_eof_error()`: what a reader raises when it runs out of input. The
    /// message is ruby's own, so a `rescue EOFError => e` prints the same
    /// text whether the reader was Ruby or C.
    fn rb_eof_error() -> () {
        Err(zeo_rt::builtins::eof_error!("end of file reached"))
    }

    // ---- errno ---------------------------------------------------------

    fn rb_syserr_new(code: c_int, msg: *const c_char) -> Value {
        let what = unsafe { cstr(msg) };
        to_value(&syserr_value(code, Some(&what))?)
    }

    fn rb_syserr_new_str(code: c_int, msg: Value) -> Value {
        let what = unsafe { text_of(msg)? };
        to_value(&syserr_value(code, Some(&what))?)
    }

    fn rb_syserr_fail(code: c_int, msg: *const c_char) -> () {
        let what = if msg.is_null() { None } else { Some(unsafe { cstr(msg) }) };
        Err(syserr(code, what.as_deref()))
    }

    fn rb_syserr_fail_str(code: c_int, msg: Value) -> () {
        let what = unsafe { text_of(msg)? };
        Err(syserr(code, Some(&what)))
    }

    /// `rb_mod_syserr_fail(mod, e, mesg)`: the same raise, with the module
    /// mixed into the exception's singleton class. Gems use it to tag an
    /// error as theirs while keeping the `Errno::*` class.
    fn rb_mod_syserr_fail(module: Value, code: c_int, msg: *const c_char) -> () {
        let what = unsafe { cstr(msg) };
        Err(tagged(module, syserr(code, Some(&what)))?)
    }

    fn rb_mod_syserr_fail_str(module: Value, code: c_int, msg: Value) -> () {
        let what = unsafe { text_of(msg)? };
        Err(tagged(module, syserr(code, Some(&what)))?)
    }

    /// `rb_readwrite_syserr_fail(waiting, e, mesg)`: an `Errno::*` raise with
    /// `IO::WaitReadable` or `IO::WaitWritable` mixed in, which is how a
    /// non-blocking read reports "try again". The two are the values of
    /// `enum rb_io_wait_readwrite`: `RB_IO_WAIT_READABLE` is 0.
    fn rb_readwrite_syserr_fail(waiting: c_int, code: c_int, msg: *const c_char) -> () {
        let what = unsafe { cstr(msg) };
        let m = wait_module(waiting)?;
        Err(tagged_value(&m, syserr(code, Some(&what)))?)
    }

    // ---- warnings ------------------------------------------------------

    /// `rb_warn`'s worker, and `rb_warning`'s and `rb_category_warn`'s: the
    /// message was formatted in `csrc/cext_va.c` and the three differ only in
    /// what gates them.
    ///
    /// `verbose_only` is `rb_warning`, which needs `$VERBOSE` TRUE where
    /// `rb_warn` needs only that it is not nil. `category` is
    /// `rb_warning_category_t` as an integer, and `0` is
    /// `RB_WARN_CATEGORY_NONE`.
    fn zeo_cext_warn(msg: *const c_char, verbose_only: c_int, category: c_int) -> () {
        if verbose_only != 0
            && !matches!(zeo_rt::globals::global_get(0, "$VERBOSE"), RubyValue::Bool(true))
        {
            return Ok(());
        }
        if let Some(cat) = category_name(category)
            && !zeo_rt::builtins::warning::category_enabled(cat)
        {
            return Ok(());
        }
        zeo_rt::builtins::warning::rb_warn(&unsafe { cstr(msg) });
        Ok(())
    }

    /// `rb_sys_warning(fmt, ...)`: a warning with the current `errno`'s text
    /// appended, and `$VERBOSE`-gated as `rb_warning` is.
    fn zeo_cext_sys_warning(msg: *const c_char, code: c_int) -> () {
        if !matches!(zeo_rt::globals::global_get(0, "$VERBOSE"), RubyValue::Bool(true)) {
            return Ok(());
        }
        let (_, desc) = errno_row(code);
        zeo_rt::builtins::warning::rb_warn(&format!("{}: {desc}", unsafe { cstr(msg) }));
        Ok(())
    }

    /// `rb_name_error(id, fmt, ...)`: a `NameError` that carries the NAME,
    /// which is what `NameError#name` and `did_you_mean` read.
    fn zeo_cext_name_error(name: Value, msg: *const c_char) -> () {
        let text = unsafe { cstr(msg) };
        Err(name_error("NameError", &text, unsafe { value_of(name) })?)
    }

    fn zeo_cext_loaderror(msg: *const c_char, path: Value) -> () {
        let text = unsafe { cstr(msg) };
        Err(name_error("LoadError", &text, unsafe { value_of(path) })?)
    }

    fn zeo_cext_frozen_error(obj: Value, msg: *const c_char) -> () {
        let text = unsafe { cstr(msg) };
        let v = unsafe { value_of(obj) };
        let shown = send(&v, "inspect", &[])
            .ok()
            .map_or_else(String::new, |s| s.to_display_string());
        Err(zeo_rt::builtins::frozen_error!("{text}: {shown}"))
    }
}

/// `NameError#name` and `LoadError#path` are both a reader over an ivar the
/// raise site sets. Building the exception and writing it is the only way to
/// get one that answers.
fn name_error(class: &str, msg: &str, name: RubyValue) -> Result<Signal, Signal> {
    let Some(cls) = zeo_rt::constants::const_get(zeo_abi::OBJECT_CLASS.0, class) else {
        return Ok(zeo_rt::dispatch::raise_error(class, msg.to_string()));
    };
    let text = zeo_rt::builtins::string::str_value_in_enc(zeo_rt::encoding::UTF_8, msg);
    let exc = send(&cls, "new", &[text, name])?;
    Ok(Signal::Raise(exc))
}

/// Mix `module` into the exception's singleton class, so a `rescue` on the
/// module catches it. `Signal::Raise` is the only shape that can carry one.
fn tagged(module: Value, sig: Signal) -> Result<Signal, Signal> {
    let m = unsafe { value_of(module) };
    tagged_value(&m, sig)
}

fn tagged_value(module: &RubyValue, sig: Signal) -> Result<Signal, Signal> {
    let Signal::Raise(exc) = sig else {
        return Ok(sig);
    };
    zeo_rt::runtime_meta::runtime_extend(&exc, module)?;
    Ok(Signal::Raise(exc))
}

/// `enum rb_io_wait_readwrite`: `RB_IO_WAIT_READABLE = 0`, `WRITABLE = 1`.
fn wait_module(waiting: c_int) -> Result<RubyValue, Signal> {
    let name = if waiting == 0 {
        "WaitReadable"
    } else {
        "WaitWritable"
    };
    let io = zeo_rt::constants::const_get(zeo_abi::OBJECT_CLASS.0, "IO")
        .ok_or_else(|| zeo_rt::builtins::name_error!("uninitialized constant IO"))?;
    let RubyValue::Class(io) = io else {
        return Err(wrong_arg_type(&io, "Class"));
    };
    zeo_rt::constants::const_get(io.0, name)
        .ok_or_else(|| zeo_rt::builtins::name_error!("uninitialized constant IO::{name}"))
}

/// `rb_warning_category_t` as the Symbol `Warning[]` accepts.
/// `RB_WARN_CATEGORY_NONE` is 0 and gates on nothing.
fn category_name(cat: c_int) -> Option<&'static str> {
    match cat {
        1 => Some("deprecated"),
        2 => Some("experimental"),
        3 => Some("performance"),
        4 => Some("strict_unused_block"),
        _ => None,
    }
}

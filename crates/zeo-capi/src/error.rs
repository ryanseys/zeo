//! Raising: exceptions, `errno`, warnings and arity.
//!
//! Every entry here ends in a raise, and MRI marks most of them
//! `__attribute__((noreturn))` -- which an extension relies on. A C compiler
//! that believes a function returns emits the code after the call; one that
//! knows it does not, does not. So each of these leaves through
//! [`super::unwind::raise`], which unwinds and never comes back.
//!
//! The variadic spellings (`rb_raise(exc, fmt, ...)`, the `rb_warn` family)
//! format their message through [`super::fmt`], MRI's format with
//! `PRIsVALUE`, and then take the same road as the fixed-arity entries.
//!
//! # `errno`
//!
//! `errno` is a macro over a per-thread location libc names differently on
//! each platform; `errno_location` is that name. What is here is the mapping
//! from a code to a class. That has to agree with the one every other raise
//! site in the runtime uses, so it calls the same `zeo_abi::errno_class`, and
//! an unmapped code falls back to `SystemCallError` -- which is what CRuby
//! answers too.

use super::convert::{to_value, value_of};
use super::fmt::vformat;
use super::misc::{Encoding, encoding_of};
use super::object::{cstr, send};
use super::symbol::Id;
use super::value::Value;
use std::ffi::{VaList, c_char, c_int, c_long};
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

    /// `errno` is a macro over a per-thread location, and `rb_errno_ptr`
    /// hands out that location's address -- the libc's own answer.
    fn rb_errno() -> c_int {
        Ok(errno())
    }

    fn rb_errno_ptr() -> *mut c_int {
        Ok(errno_location())
    }

    fn rb_errno_set(e: c_int) -> () {
        unsafe { errno_location().write(e) };
        Ok(())
    }

    /// `rb_sys_fail(mesg)`: `rb_syserr_fail` for the current `errno`, read
    /// first thing so nothing here can move it.
    fn rb_sys_fail(msg: *const c_char) -> () {
        let code = errno();
        let what = if msg.is_null() { None } else { Some(unsafe { cstr(msg) }) };
        Err(syserr(code, what.as_deref()))
    }

    fn rb_sys_fail_str(msg: Value) -> () {
        let code = errno();
        let what = unsafe { text_of(msg)? };
        Err(syserr(code, Some(&what)))
    }

    fn rb_mod_sys_fail(module: Value, msg: *const c_char) -> () {
        let code = errno();
        let what = unsafe { cstr(msg) };
        Err(tagged(module, syserr(code, Some(&what)))?)
    }

    fn rb_mod_sys_fail_str(module: Value, msg: Value) -> () {
        let code = errno();
        let what = unsafe { text_of(msg)? };
        Err(tagged(module, syserr(code, Some(&what)))?)
    }

    fn rb_readwrite_sys_fail(waiting: c_int, msg: *const c_char) -> () {
        let code = errno();
        let what = unsafe { cstr(msg) };
        let m = wait_module(waiting)?;
        Err(tagged_value(&m, syserr(code, Some(&what)))?)
    }

}

/// A formatted message, as the variadic entries read theirs.
///
/// # Safety
///
/// `fmt` must be NUL-terminated and `ap` must hold what it names.
unsafe fn message(fmt: *const c_char, ap: &mut VaList<'_>) -> Result<String, Signal> {
    Ok(String::from_utf8_lossy(&unsafe { vformat(fmt, ap)? }).into_owned())
}

/// `rb_warn`, `rb_warning` and `rb_category_warn` differ only in what gates
/// them. `verbose_only` is `rb_warning`, which needs `$VERBOSE` TRUE where
/// `rb_warn` needs only that it is not nil. `category` is
/// `rb_warning_category_t` as an integer, and `0` is
/// `RB_WARN_CATEGORY_NONE`.
fn warn(msg: &str, verbose_only: bool, category: c_int) {
    if verbose_only
        && !matches!(
            zeo_rt::globals::global_get(0, "$VERBOSE"),
            RubyValue::Bool(true)
        )
    {
        return;
    }
    if let Some(cat) = category_name(category)
        && !zeo_rt::builtins::warning::category_enabled(cat)
    {
        return;
    }
    zeo_rt::builtins::warning::rb_warn(msg);
}

/// The compile-time warnings. MRI reports the file and line the PARSER was
/// at; an extension calling one is not parsing anything, so the pair it
/// passes is the only honest location and is written into the message.
///
/// # Safety
///
/// `file` must be NUL-terminated or null.
unsafe fn located(file: *const c_char, line: c_int, msg: &str) -> String {
    let file = if file.is_null() {
        "-".to_string()
    } else {
        unsafe { cstr(file) }
    };
    format!("{file}:{line}: {msg}")
}

crate::cext_va_fn! {
    fn rb_warn(fmt: *const c_char; ap) -> () {
        warn(&unsafe { message(fmt, ap)? }, false, 0);
        Ok(())
    }

    fn rb_warning(fmt: *const c_char; ap) -> () {
        warn(&unsafe { message(fmt, ap)? }, true, 0);
        Ok(())
    }

    fn rb_category_warn(cat: c_int, fmt: *const c_char; ap) -> () {
        warn(&unsafe { message(fmt, ap)? }, false, cat);
        Ok(())
    }

    fn rb_category_warning(cat: c_int, fmt: *const c_char; ap) -> () {
        warn(&unsafe { message(fmt, ap)? }, true, cat);
        Ok(())
    }

    /// A warning with the current `errno`'s text appended, and
    /// `$VERBOSE`-gated as `rb_warning` is. `errno` is read before the
    /// format runs, which may itself set it.
    fn rb_sys_warning(fmt: *const c_char; ap) -> () {
        let code = errno();
        let msg = unsafe { message(fmt, ap)? };
        if matches!(zeo_rt::globals::global_get(0, "$VERBOSE"), RubyValue::Bool(true)) {
            let (_, desc) = errno_row(code);
            zeo_rt::builtins::warning::rb_warn(&format!("{msg}: {desc}"));
        }
        Ok(())
    }

    fn rb_compile_warn(file: *const c_char, line: c_int, fmt: *const c_char; ap) -> () {
        let msg = unsafe { message(fmt, ap)? };
        warn(&unsafe { located(file, line, &msg) }, false, 0);
        Ok(())
    }

    fn rb_compile_warning(file: *const c_char, line: c_int, fmt: *const c_char; ap) -> () {
        let msg = unsafe { message(fmt, ap)? };
        warn(&unsafe { located(file, line, &msg) }, true, 0);
        Ok(())
    }

    fn rb_category_compile_warn(
        cat: c_int,
        file: *const c_char,
        line: c_int,
        fmt: *const c_char;
        ap
    ) -> () {
        let msg = unsafe { message(fmt, ap)? };
        warn(&unsafe { located(file, line, &msg) }, false, cat);
        Ok(())
    }

    // ---- the named raises ----------------------------------------------

    fn rb_raise(exc: Value, fmt: *const c_char; ap) -> () {
        let msg = unsafe { message(fmt, ap)? };
        Err(unsafe { super::call::raise_str(exc, &msg) })
    }

    /// `rb_raise` with the message string tagged in `enc` rather than in
    /// the default, which is MRI's own route through `rb_enc_vsprintf` and
    /// `rb_exc_new_str`.
    fn rb_enc_raise(enc: Encoding, exc: Value, fmt: *const c_char; ap) -> () {
        let bytes = unsafe { vformat(fmt, ap)? };
        let mesg = unsafe { value_of(super::fmt::new_str(bytes, encoding_of(enc))?) };
        let k = unsafe { value_of(exc) };
        Err(Signal::Raise(send(&k, "new", &[mesg])?))
    }

    fn rb_fatal(fmt: *const c_char; ap) -> () {
        let msg = unsafe { message(fmt, ap)? };
        Err(unsafe { super::call::raise_str(0, &msg) })
    }

    /// `rb_name_error(id, fmt, ...)`: a `NameError` that carries the NAME,
    /// which is what `NameError#name` and `did_you_mean` read.
    fn rb_name_error(id: Id, fmt: *const c_char; ap) -> () {
        let msg = unsafe { message(fmt, ap)? };
        let name = RubyValue::Symbol(super::symbol::symbol_of(id));
        Err(name_error("NameError", &msg, name)?)
    }

    fn rb_name_error_str(name: Value, fmt: *const c_char; ap) -> () {
        let msg = unsafe { message(fmt, ap)? };
        Err(name_error("NameError", &msg, unsafe { value_of(name) })?)
    }

    fn rb_loaderror(fmt: *const c_char; ap) -> () {
        let msg = unsafe { message(fmt, ap)? };
        Err(name_error("LoadError", &msg, RubyValue::Nil)?)
    }

    fn rb_loaderror_with_path(path: Value, fmt: *const c_char; ap) -> () {
        let msg = unsafe { message(fmt, ap)? };
        Err(name_error("LoadError", &msg, unsafe { value_of(path) })?)
    }

    fn rb_frozen_error_raise(obj: Value, fmt: *const c_char; ap) -> () {
        let msg = unsafe { message(fmt, ap)? };
        let v = unsafe { value_of(obj) };
        let shown = send(&v, "inspect", &[])
            .ok()
            .map_or_else(String::new, |s| s.to_display_string());
        Err(zeo_rt::builtins::frozen_error!("{msg}: {shown}"))
    }

    /// What a failed `RUBY_ASSERT` calls. It is a bug in the extension by
    /// construction, so it aborts rather than raising -- the same answer
    /// `rb_bug` gives.
    fn rb_assert_failure_detail(
        file: *const c_char,
        line: c_int,
        name: *const c_char,
        expr: *const c_char,
        fmt: *const c_char;
        ap
    ) -> () {
        let msg = unsafe { message(fmt, ap)? };
        let dash = |p: *const c_char| if p.is_null() { "-".to_string() } else { unsafe { cstr(p) } };
        let out = format!(
            "{}:{line}:{}: assertion failed: {}: {msg}",
            dash(file),
            dash(name),
            dash(expr)
        );
        let out = std::ffi::CString::new(out).unwrap_or_default();
        unsafe { super::call::rb_bug(out.as_ptr()) };
        Ok(())
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

/// The calling thread's `errno` slot.
fn errno_location() -> *mut c_int {
    #[cfg(target_vendor = "apple")]
    unsafe {
        libc::__error()
    }
    #[cfg(target_os = "linux")]
    unsafe {
        libc::__errno_location()
    }
}

fn errno() -> c_int {
    unsafe { *errno_location() }
}

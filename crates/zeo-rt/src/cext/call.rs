//! Calling back into Ruby, raising, and catching.
//!
//! This is the half of the C surface that has no zeo counterpart to wrap: an
//! extension calling `rb_funcall` is re-entering the interpreter, and one
//! calling `rb_protect` is asking to catch what comes back. Everything in
//! `cext/string.rs` and its neighbours is a thin cover over a Rust function
//! that already existed; nothing here is.
//!
//! `rb_funcall` is the reason so much else can be thin. It goes through
//! `dispatch::send_value`, which is the same door Ruby code uses, so a C
//! extension's call sees the same MRO, the same refinements, the same
//! `method_missing` and the same visibility rules. That is also why the
//! forwarding table in [`super::forward`] is honest: it is not reimplementing
//! `String#upcase`, it is calling it.

use super::convert::{to_value, value_of};
use super::symbol::{Id, symbol_of};
use super::value::{self, Value};
use crate::{RubyValue, Signal, Symbol};
use std::ffi::{c_char, c_int};

/// # Safety
///
/// `argv` must name `argc` readable `VALUE`s.
unsafe fn args_of(argc: c_int, argv: *const Value) -> Vec<RubyValue> {
    let n = argc.max(0) as usize;
    if n == 0 || argv.is_null() {
        return Vec::new();
    }
    // SAFETY: the caller's contract.
    unsafe { std::slice::from_raw_parts(argv, n) }
        .iter()
        .map(|v| unsafe { value_of(*v) })
        .collect()
}

/// # Safety
///
/// `p` must be NUL-terminated.
unsafe fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: the caller's contract.
    unsafe { String::from_utf8_lossy(std::ffi::CStr::from_ptr(p).to_bytes()).into_owned() }
}

pub(super) fn call(recv: &RubyValue, id: Id, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::dispatch::send_value(recv, symbol_of(id), args, None)
}

crate::cext_fn! {
    /// `rb_funcallv(recv, mid, argc, argv)`. The one entry every other
    /// funcall spelling reduces to.
    fn rb_funcallv(recv: Value, mid: Id, argc: c_int, argv: *const Value) -> Value {
        let recv = unsafe { value_of(recv) };
        let args = unsafe { args_of(argc, argv) };
        to_value(&call(&recv, mid, &args)?)
    }

    /// `rb_funcallv_public`. zeo's `send_value` already enforces visibility
    /// for an explicit receiver, which is the third call mode ruby has and
    /// the one this asks for.
    fn rb_funcallv_public(recv: Value, mid: Id, argc: c_int, argv: *const Value) -> Value {
        let recv = unsafe { value_of(recv) };
        let args = unsafe { args_of(argc, argv) };
        to_value(&call(&recv, mid, &args)?)
    }

    fn rb_funcall_with_block(
        recv: Value,
        mid: Id,
        argc: c_int,
        argv: *const Value,
        block: Value,
    ) -> Value {
        let recv = unsafe { value_of(recv) };
        let args = unsafe { args_of(argc, argv) };
        let block = match unsafe { value_of(block) } {
            RubyValue::Nil => None,
            b => Some(b),
        };
        to_value(&crate::dispatch::send_value(&recv, symbol_of(mid), &args, block)?)
    }

    /// `rb_raise`'s worker. The format string was already run through
    /// `vsnprintf` by `csrc/cext_va.c`, which is where the `va_list` can be
    /// read safely -- see that file.
    ///
    /// `exc == 0` is `rb_fatal`, which has no class of its own.
    fn zeo_cext_raise_str(exc: Value, msg: *const c_char) -> Value {
        let msg = unsafe { cstr(msg) };
        if exc == 0 {
            return Err(crate::dispatch::raise_error("RuntimeError", msg));
        }
        Err(raise_with(&unsafe { value_of(exc) }, msg))
    }

    fn rb_exc_raise(exc: Value) -> Value {
        Err(Signal::Raise(unsafe { value_of(exc) }))
    }

    /// `rb_bug` is MRI's "the interpreter is broken" exit. It is not
    /// rescuable there and is not here.
    fn rb_bug(fmt: *const c_char) -> Value {
        eprintln!("zeo: a C extension called rb_bug: {}", unsafe { cstr(fmt) });
        std::process::abort()
    }

    /// `rb_protect(body, arg, &state)`. `*state` is non-zero when the body
    /// left through a raise, and the pending exception is readable with
    /// `rb_errinfo` until it is cleared or re-raised.
    fn rb_protect(
        body: unsafe extern "C" fn(Value) -> Value,
        arg: Value,
        state: *mut c_int,
    ) -> Value {
        let out = super::jmp::protect(|| unsafe { body(arg) });
        if !state.is_null() {
            // SAFETY: the caller's own `int` slot.
            unsafe { state.write(c_int::from(out.is_err())) };
        }
        match out {
            Ok(v) => Ok(v),
            Err(sig) => {
                set_errinfo(&sig);
                Ok(value::Q_NIL)
            }
        }
    }

    /// `rb_ensure(body, barg, ensure, earg)`. The ensure body runs on both
    /// paths, and a raise from the ensure body itself replaces the original
    /// -- which is what ruby's own `ensure` does.
    fn rb_ensure(
        body: unsafe extern "C" fn(Value) -> Value,
        barg: Value,
        ens: unsafe extern "C" fn(Value) -> Value,
        earg: Value,
    ) -> Value {
        let out = super::jmp::protect(|| unsafe { body(barg) });
        let cleanup = super::jmp::protect(|| unsafe { ens(earg) });
        cleanup?;
        out
    }

    /// `rb_rescue2`'s worker. `csrc/cext_va.c` walked the `0`-terminated
    /// class list into `classes`; an EMPTY list is `rb_rescue`, which means
    /// `StandardError`.
    fn zeo_cext_rescue2(
        body: unsafe extern "C" fn(Value) -> Value,
        barg: Value,
        resc: unsafe extern "C" fn(Value, Value) -> Value,
        rarg: Value,
        classes: *const Value,
        nclasses: c_int,
    ) -> Value {
        let wanted = unsafe { args_of(nclasses, classes) };
        match super::jmp::protect(|| unsafe { body(barg) }) {
            Ok(v) => Ok(v),
            Err(Signal::Raise(exc)) if rescued_by(&exc, &wanted) => {
                set_errinfo(&Signal::Raise(exc.clone()));
                let e = to_value(&exc)?;
                super::jmp::protect(|| unsafe { resc(rarg, e) })
            }
            Err(other) => Err(other),
        }
    }

    /// `rb_scan_args`'s format parser. The slots themselves are walked in
    /// `csrc/cext_va.c`, which is the only part that needs a `va_list`.
    fn zeo_cext_scan_plan(
        fmt: *const c_char,
        required: *mut c_int,
        optional: *mut c_int,
        splat: *mut c_int,
        block: *mut c_int,
    ) -> c_int {
        let Some((r, o, s, b)) = scan_args_plan(&unsafe { cstr(fmt) }) else {
            return Ok(0);
        };
        // SAFETY: four `int` slots the caller owns.
        unsafe {
            required.write(r as c_int);
            optional.write(o as c_int);
            splat.write(c_int::from(s));
            block.write(c_int::from(b));
        }
        Ok(1)
    }

    /// `rb_scan_args`'s splat slot: `argv[from..to]` as an Array.
    fn zeo_cext_scan_slice(argc: c_int, argv: *const Value, from: c_int, to: c_int) -> Value {
        let all = unsafe { args_of(argc, argv) };
        let lo = from.max(0) as usize;
        let hi = (to.max(0) as usize).min(all.len());
        let rest = all.get(lo..hi).unwrap_or(&[]).to_vec();
        to_value(&RubyValue::Array(crate::value::collections::array_new(rest)))
    }

    /// `rb_jump_tag(state)`: leave with the pending exception `rb_protect`
    /// left behind. An extension calls it to re-raise after cleaning up.
    fn rb_jump_tag(_state: c_int) -> Value {
        Err(ERRINFO.with_borrow_mut(Option::take).map_or_else(
            || crate::dispatch::raise_error("RuntimeError", "rb_jump_tag with no pending exception".into()),
            |e| Signal::Raise(e),
        ))
    }

    fn rb_errinfo() -> Value {
        let pending = ERRINFO.with_borrow(Clone::clone);
        to_value(&pending.unwrap_or(RubyValue::Nil))
    }

    fn rb_set_errinfo(v: Value) -> () {
        let v = unsafe { value_of(v) };
        ERRINFO.with_borrow_mut(|e| *e = if matches!(v, RubyValue::Nil) { None } else { Some(v) });
        Ok(())
    }

    /// `rb_yield(v)` -- call the block the current Ruby frame was given.
    fn rb_yield(v: Value) -> Value {
        let arg = unsafe { value_of(v) };
        to_value(&yield_to_block(&[arg])?)
    }

    fn rb_yield_values2(argc: c_int, argv: *const Value) -> Value {
        let args = unsafe { args_of(argc, argv) };
        to_value(&yield_to_block(&args)?)
    }

    fn rb_block_given_p() -> c_int {
        Ok(c_int::from(current_block().is_some()))
    }

    fn rb_obj_class(v: Value) -> Value {
        let v = unsafe { value_of(v) };
        to_value(&RubyValue::Class(v.class_id()))
    }

    fn rb_class_of(v: Value) -> Value {
        let v = unsafe { value_of(v) };
        to_value(&RubyValue::Class(v.class_id()))
    }

    fn rb_obj_is_kind_of(v: Value, klass: Value) -> Value {
        let v = unsafe { value_of(v) };
        let k = unsafe { value_of(klass) };
        Ok(super::convert::boolean(value_is_a(&v, &k)))
    }

    fn rb_respond_to(v: Value, mid: Id) -> c_int {
        let v = unsafe { value_of(v) };
        Ok(c_int::from(crate::dispatch::responds_to_value(&v, symbol_of(mid), false)))
    }

    /// `rb_type(v)` -- the `RUBY_T_*` tag. Reading it off the handle's own
    /// `RBasic` prefix keeps this and `RB_BUILTIN_TYPE` from ever disagreeing.
    fn rb_type(v: Value) -> c_int {
        Ok(unsafe { super::handles::type_tag(v) } as c_int)
    }

    /// `rb_gc_mark`. Records one edge while a walk is running, and does
    /// nothing otherwise -- which is what it does in MRI outside a GC too.
    fn rb_gc_mark(v: Value) -> () {
        if super::data::marking() {
            super::data::mark_edge(unsafe { value_of(v) });
        }
        Ok(())
    }

    fn rb_gc_mark_movable(v: Value) -> () {
        if super::data::marking() {
            super::data::mark_edge(unsafe { value_of(v) });
        }
        Ok(())
    }

    /// zeo never moves an object, so a location is the location it had.
    fn rb_gc_location(v: Value) -> Value {
        Ok(v)
    }

    fn rb_gc_mark_locations(start: *const Value, end: *const Value) -> () {
        if super::data::marking() && !start.is_null() && !end.is_null() {
            // SAFETY: the caller promised a contiguous run.
            let n = unsafe { end.offset_from(start) }.max(0) as usize;
            for i in 0..n {
                super::data::mark_edge(unsafe { value_of(start.add(i).read()) });
            }
        }
        Ok(())
    }

    /// `rb_gc_register_address`. Pins whatever the slot names for the life of
    /// the process, which is what a C global holding a `VALUE` needs -- see
    /// `cext::scope` for why an unregistered one would dangle.
    fn rb_gc_register_address(slot: *mut Value) -> () {
        register_root(slot);
        Ok(())
    }

    fn rb_global_variable(slot: *mut Value) -> () {
        register_root(slot);
        Ok(())
    }

    fn rb_gc_unregister_address(slot: *mut Value) -> () {
        ROOTS.lock().retain(|r| r.0 != slot as usize);
        Ok(())
    }
}

thread_local! {
    /// The exception `rb_protect` caught, readable through `rb_errinfo` until
    /// it is cleared or re-raised. Thread-local, as `$!` is.
    static ERRINFO: std::cell::RefCell<Option<RubyValue>> =
        const { std::cell::RefCell::new(None) };
}

pub(super) fn set_errinfo(sig: &Signal) {
    if let Signal::Raise(e) = sig {
        ERRINFO.with_borrow_mut(|slot| *slot = Some(e.clone()));
    }
}

/// A C global holding a `VALUE`, kept alive for the process.
struct Root(usize);
// SAFETY: the address is an extension's own static, which outlives every
// thread. Only the GVL-holding thread reads through it.
unsafe impl Send for Root {}

static ROOTS: parking_lot::Mutex<Vec<Root>> = parking_lot::Mutex::new(Vec::new());

fn register_root(slot: *mut Value) {
    if slot.is_null() {
        return;
    }
    let mut roots = ROOTS.lock();
    if !roots.iter().any(|r| r.0 == slot as usize) {
        roots.push(Root(slot as usize));
    }
}

/// Every registered root's current value, so the pin can be refreshed.
///
/// A root is a SLOT, not a value: the extension assigns into it whenever it
/// likes, and what it named last time may be gone. Re-reading is the only way
/// to keep the pin on the right object.
pub fn root_values() -> Vec<Value> {
    ROOTS
        .lock()
        .iter()
        .map(|r| unsafe { (r.0 as *const Value).read() })
        .collect()
}

/// Does `exc` match one of the classes `rb_rescue2` named?
///
/// An empty list is `rb_rescue`, which means `StandardError` -- the same
/// thing a bare `rescue` means in Ruby.
fn rescued_by(exc: &RubyValue, classes: &[RubyValue]) -> bool {
    if classes.is_empty() {
        return crate::dispatch::is_a(exc.class_id(), zeo_abi::exc_id(4));
    }
    classes.iter().any(|c| value_is_a(exc, c))
}

/// Build the exception `rb_raise` asks for. `exc` is a class in every census
/// use; an instance is accepted too, because `rb_raise(rb_eArgError, ...)`
/// and `rb_raise(some_exception, ...)` are spelled the same.
/// Build and throw `class.new(msg)`.
///
/// Through `exception`/`new` on the CLASS OBJECT, not through
/// `raise_error(name)`. The name-keyed path reads a registry of classes the
/// COMPILER knew about, and an extension's own exception class is not one --
/// `msgpack`'s `MessagePack::MalformedFormatError` is defined inside `Init_`,
/// so a `rb_raise` naming it panicked with "no such class registered" instead
/// of raising something a `rescue` could catch.
fn raise_with(class: &RubyValue, msg: String) -> Signal {
    let RubyValue::Class(_) = class else {
        // Already an exception INSTANCE, which `rb_raise` also accepts.
        return Signal::Raise(class.clone());
    };
    let text = crate::builtins::string::str_value_in_enc(crate::encoding::UTF_8, &msg);
    // `exception` is the protocol `raise` itself uses, and a class that
    // overrides it -- several gems do, to attach state -- is honoured.
    match crate::dispatch::send_value(class, Symbol::intern("exception"), &[text], None) {
        Ok(exc) => Signal::Raise(exc),
        // A class with neither `exception` nor `new` cannot be raised at all;
        // saying which class beats a bare RuntimeError.
        Err(_) => crate::dispatch::raise_error(
            "TypeError",
            format!(
                "exception class/object expected: {}",
                crate::dispatch::class_name(class.class_id()).unwrap_or("Object".into())
            ),
        ),
    }
}

// The block belonging to the C method frame currently running.
//
// ruby's `yield` reads the FRAME's block, and a C method's frame is the
// trampoline in `cext::method`. zeo passes blocks explicitly rather than
// keeping an ambient one, so the trampoline parks it here for the length of
// the call and `rb_yield` reads the top.
thread_local! {
    static BLOCKS: std::cell::RefCell<Vec<Option<RubyValue>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Park `block` for the length of one C method call.
pub(super) struct BlockFrame;

impl BlockFrame {
    pub(super) fn enter(block: Option<RubyValue>) -> BlockFrame {
        BLOCKS.with_borrow_mut(|b| b.push(block));
        BlockFrame
    }
}

impl Drop for BlockFrame {
    fn drop(&mut self) {
        BLOCKS.with_borrow_mut(Vec::pop);
    }
}

pub(super) fn current_block() -> Option<RubyValue> {
    BLOCKS.with_borrow(|b| b.last().cloned().flatten())
}

pub(super) fn yield_to_block(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    match current_block() {
        Some(RubyValue::Proc(p)) => p.call(args),
        Some(other) => crate::dispatch::send_value(&other, Symbol::intern("call"), args, None),
        None => Err(crate::dispatch::raise_error(
            "LocalJumpError",
            "no block given (yield)".into(),
        )),
    }
}

/// `Object#is_a?` over two values, for the two entries that need it.
fn value_is_a(v: &RubyValue, klass: &RubyValue) -> bool {
    match klass {
        RubyValue::Class(cid) => crate::dispatch::is_a(v.class_id(), *cid),
        _ => false,
    }
}

/// `rb_scan_args`'s runtime half, minus the varargs.
///
/// The C prototype is `rb_scan_args(argc, argv, fmt, ...)` with one `VALUE *`
/// per slot the format names, and reading those is the same varargs problem
/// `rb_raise` has. So the format is PARSED here and the slots are filled by
/// the shim in `csrc/cext_jmp.c`, which knows the count before it walks them.
///
/// Returns how many positional arguments were consumed.
pub fn scan_args_plan(fmt: &str) -> Option<(usize, usize, bool, bool)> {
    let mut chars = fmt.chars().peekable();
    let digit = |c: Option<&char>| c.and_then(|c| c.to_digit(10)).map(|d| d as usize);
    let required = digit(chars.peek()).map(|d| {
        chars.next();
        d
    })?;
    let optional = digit(chars.peek()).unwrap_or_else(|| 0);
    if optional > 0 || chars.peek() == Some(&'0') {
        chars.next();
    }
    let mut splat = false;
    let mut block = false;
    for c in chars {
        match c {
            '*' => splat = true,
            '&' => block = true,
            ':' => {}
            _ => return None,
        }
    }
    Some((required, optional, splat, block))
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn returns(v: Value) -> Value {
        v
    }
    unsafe extern "C" fn raises(_v: Value) -> Value {
        super::super::jmp::raise(Signal::Break(RubyValue::Int(1)))
    }
    unsafe extern "C" fn note(v: Value) -> Value {
        RAN.with(|r| r.set(r.get() + 1));
        v
    }
    thread_local! {
        static RAN: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }

    #[test]
    fn protect_reports_the_state_both_ways() {
        let _scope = super::super::scope::Scope::enter();
        let mut state: c_int = 99;
        let v = unsafe { rb_protect(returns, value::fixnum(5), &raw mut state) };
        assert_eq!((v, state), (value::fixnum(5), 0));

        let v = unsafe { rb_protect(raises, value::Q_NIL, &raw mut state) };
        assert_eq!(v, value::Q_NIL);
        assert_eq!(state, 1, "a raise left state zero");
    }

    /// The cleanup runs whether the body returned or raised. An extension
    /// that frees in its ensure body leaks otherwise.
    #[test]
    fn ensure_runs_on_both_paths() {
        let _scope = super::super::scope::Scope::enter();
        RAN.with(|r| r.set(0));
        let v = unsafe { rb_ensure(returns, value::fixnum(7), note, value::Q_NIL) };
        assert_eq!(v, value::fixnum(7));
        assert_eq!(RAN.with(std::cell::Cell::get), 1);

        let out = super::super::jmp::protect(|| unsafe {
            rb_ensure(raises, value::Q_NIL, note, value::Q_NIL)
        });
        assert!(out.is_err(), "the body's raise was swallowed");
        assert_eq!(
            RAN.with(std::cell::Cell::get),
            2,
            "cleanup skipped on raise"
        );
    }

    #[test]
    fn a_registered_root_is_read_back_from_its_slot() {
        let mut slot: Value = value::fixnum(3);
        unsafe { rb_gc_register_address(&raw mut slot) };
        assert!(root_values().contains(&value::fixnum(3)));
        // A root is a SLOT: reassigning it changes what is pinned.
        slot = value::fixnum(4);
        assert!(root_values().contains(&value::fixnum(4)));
        unsafe { rb_gc_unregister_address(&raw mut slot) };
        assert!(!root_values().contains(&value::fixnum(4)));
    }

    #[test]
    fn errinfo_holds_what_protect_caught_until_it_is_cleared() {
        let _scope = super::super::scope::Scope::enter();
        let mut state: c_int = 0;
        unsafe { rb_protect(raises, value::Q_NIL, &raw mut state) };
        // A `Break` is not a `Raise`, so nothing is recorded -- which is the
        // point: only a real exception becomes `$!`.
        assert!(ERRINFO.with_borrow(Option::is_none));
    }

    /// `rb_scan_args("12*&")` means one required, two optional, a splat and a
    /// block. Getting the split wrong silently shifts every argument.
    #[test]
    fn a_scan_args_format_parses_into_its_four_parts() {
        assert_eq!(scan_args_plan("0"), Some((0, 0, false, false)));
        assert_eq!(scan_args_plan("11"), Some((1, 1, false, false)));
        assert_eq!(scan_args_plan("12*&"), Some((1, 2, true, true)));
        assert_eq!(scan_args_plan("2"), Some((2, 0, false, false)));
        assert_eq!(
            scan_args_plan("*"),
            None,
            "a format must start with a digit"
        );
    }
}

//! Blocks, calls, frames, `eval` and `require`.
//!
//! # `rb_block_call_func_t` is the shape a C block arrives in
//!
//! `rb_block_call`, `rb_iterate`, `rb_catch` and `rb_fiber_new` all take a
//! `VALUE (*)(VALUE yielded, VALUE arg, int argc, const VALUE *argv, VALUE
//! blockarg)` and expect it to behave as a Ruby block. One wrapper turns that
//! into an `RProc` -- [`super::thread::block_proc`] -- and every entry here
//! that takes a C block goes through it, so the argument order is decided
//! once.
//!
//! # `eval` is a real compile
//!
//! `rb_eval_string` runs zeo's own compiler, exactly as `Kernel#eval` does.
//! There is no interpreter and no literal splice; the seam is the same one
//! documented in `docs/EVAL.md`.
//!
//! # What the frame entries can and cannot answer
//!
//! `rb_frame_this_func` and `rb_frame_callee` differ in MRI: the first is the
//! name the method was DEFINED as and the second the name it was CALLED by,
//! which an alias makes different. zeo's frame carries one label, so the two
//! answer the same thing -- an honest divergence rather than a guess, and it
//! only shows through an aliased C method asking about itself.

use super::convert::{to_value, value_of};
use super::object::{args_of, cstr, send};
use super::symbol::{Id, symbol_of};
use super::value::{self, Value};
use crate::builtins::wrong_arg_type;
use crate::{RubyValue, Signal, Symbol};
use std::ffi::{c_char, c_int, c_void};

/// `rb_block_call_func_t`.
pub type BlockFunc = unsafe extern "C" fn(Value, Value, c_int, *const Value, Value) -> Value;

/// A C block as the `Option<RubyValue>` `send_value` takes.
///
/// # Safety
///
/// `f` must have `rb_block_call_func_t`'s shape.
unsafe fn as_block(f: Option<BlockFunc>, arg: Value) -> Option<RubyValue> {
    // SAFETY: the caller's contract.
    f.map(|f| RubyValue::Proc(unsafe { super::thread::block_proc(f, arg) }))
}

/// The `main` object a top-level call has as its receiver.
fn main_recv() -> RubyValue {
    crate::dispatch::main_object()
}

crate::cext_fn! {
    // ---- calling with a block --------------------------------------------

    /// `rb_block_call(recv, mid, argc, argv, f, arg)`: call a method with a C
    /// function as its block.
    fn rb_block_call(
        recv: Value,
        mid: Id,
        argc: c_int,
        argv: *const Value,
        f: Option<BlockFunc>,
        arg: Value,
    ) -> Value {
        let r = unsafe { value_of(recv) };
        let args = unsafe { args_of(argc, argv) };
        let block = unsafe { as_block(f, arg) };
        to_value(&crate::dispatch::send_value(&r, symbol_of(mid), &args, block)?)
    }

    fn rb_block_call_kw(
        recv: Value,
        mid: Id,
        argc: c_int,
        argv: *const Value,
        f: Option<BlockFunc>,
        arg: Value,
        _kw: c_int,
    ) -> Value {
        unsafe { Ok(rb_block_call(recv, mid, argc, argv, f, arg)) }
    }

    /// `rb_iterate(body, barg, block, barg2)`: run `body`, and any `rb_yield`
    /// inside it reaches `block`. The older spelling of `rb_block_call`, and
    /// it works the same way -- the block is installed on the frame rather
    /// than passed as an argument.
    fn rb_iterate(
        body: unsafe extern "C" fn(Value) -> Value,
        barg: Value,
        block: Option<BlockFunc>,
        arg: Value,
    ) -> Value {
        let installed = unsafe { as_block(block, arg) };
        let _frame = super::call::BlockFrame::enter(installed);
        super::jmp::protect(|| unsafe { body(barg) })
    }

    /// `rb_each(obj)`: `obj.each` with the current block, which is what
    /// `rb_iterate(rb_each, ary, ...)` means.
    fn rb_each(v: Value) -> Value {
        let recv = unsafe { value_of(v) };
        let block = super::call::current_block();
        to_value(&crate::dispatch::send_value(&recv, Symbol::intern("each"), &[], block)?)
    }

    /// `rb_apply(recv, mid, args)`: call with the arguments in an ARRAY,
    /// which is what makes it different from `rb_funcallv`.
    fn rb_apply(recv: Value, mid: Id, args: Value) -> Value {
        let r = unsafe { value_of(recv) };
        let list = match unsafe { value_of(args) } {
            RubyValue::Array(a) => a.lock().to_vec(),
            RubyValue::Nil => Vec::new(),
            other => vec![other],
        };
        to_value(&crate::dispatch::send_value(&r, symbol_of(mid), &list, None)?)
    }

    /// `rb_funcall_passing_block`: call, handing the CURRENT frame's block
    /// down. An extension wrapping a method uses it so `yield` inside the
    /// callee reaches its own caller's block.
    fn rb_funcall_passing_block(recv: Value, mid: Id, argc: c_int, argv: *const Value) -> Value {
        let r = unsafe { value_of(recv) };
        let args = unsafe { args_of(argc, argv) };
        let block = super::call::current_block();
        to_value(&crate::dispatch::send_value(&r, symbol_of(mid), &args, block)?)
    }

    fn rb_funcall_passing_block_kw(
        recv: Value,
        mid: Id,
        argc: c_int,
        argv: *const Value,
        _kw: c_int,
    ) -> Value {
        unsafe { Ok(rb_funcall_passing_block(recv, mid, argc, argv)) }
    }

    /// The `_kw` spellings. zeo peels a trailing keyword Hash from the
    /// argument list itself, so the flag adds nothing -- and accepting it
    /// rather than refusing is what lets a gem written for 3.x compile.
    fn rb_funcallv_kw(recv: Value, mid: Id, argc: c_int, argv: *const Value, _kw: c_int) -> Value {
        unsafe { Ok(super::call::rb_funcallv(recv, mid, argc, argv)) }
    }

    fn rb_funcallv_public_kw(
        recv: Value,
        mid: Id,
        argc: c_int,
        argv: *const Value,
        _kw: c_int,
    ) -> Value {
        unsafe { Ok(super::call::rb_funcallv_public(recv, mid, argc, argv)) }
    }

    fn rb_funcall_with_block_kw(
        recv: Value,
        mid: Id,
        argc: c_int,
        argv: *const Value,
        block: Value,
        _kw: c_int,
    ) -> Value {
        unsafe { Ok(super::call::rb_funcall_with_block(recv, mid, argc, argv, block)) }
    }

    /// `rb_check_funcall(recv, mid, argc, argv)`: call, or answer `Qundef`
    /// if the method is absent. The `Qundef` is the whole point -- it is how
    /// a caller tells "no such method" from "the method answered nil".
    fn rb_check_funcall(recv: Value, mid: Id, argc: c_int, argv: *const Value) -> Value {
        let r = unsafe { value_of(recv) };
        let name = symbol_of(mid);
        if !crate::dispatch::responds_to_value(&r, name, true) {
            return Ok(value::Q_UNDEF);
        }
        let args = unsafe { args_of(argc, argv) };
        to_value(&crate::dispatch::send_value(&r, name, &args, None)?)
    }

    fn rb_check_funcall_kw(
        recv: Value,
        mid: Id,
        argc: c_int,
        argv: *const Value,
        _kw: c_int,
    ) -> Value {
        unsafe { Ok(rb_check_funcall(recv, mid, argc, argv)) }
    }

    /// `rb_call_super(argc, argv)`: MRI walks the frame stack for the method
    /// the current C body overrides.
    ///
    /// Both halves come from the frame. `send_super_dynamic` reads the
    /// defining class and name; the RECEIVER is the trampoline's, and hard
    /// coding `main` here made io-console's prepended `IO#tty?` -- whose body
    /// is `rb_call_super(0, 0)` -- walk main's ancestors and raise.
    fn rb_call_super(argc: c_int, argv: *const Value) -> Value {
        let args = unsafe { args_of(argc, argv) };
        let recv = super::call::current_receiver()
            .unwrap_or_else(crate::dispatch::main_object);
        to_value(&crate::runtime_meta::send_super_dynamic(&recv, &args, super::call::current_block())?)
    }

    fn rb_call_super_kw(argc: c_int, argv: *const Value, _kw: c_int) -> Value {
        unsafe { Ok(rb_call_super(argc, argv)) }
    }

    // ---- yielding --------------------------------------------------------

    fn rb_yield_values_kw(argc: c_int, argv: *const Value, _kw: c_int) -> Value {
        let args = unsafe { args_of(argc, argv) };
        to_value(&super::call::yield_to_block(&args)?)
    }

    /// `rb_yield_splat(ary)`: yield the array's elements as separate
    /// arguments.
    fn rb_yield_splat(ary: Value) -> Value {
        let args = match unsafe { value_of(ary) } {
            RubyValue::Array(a) => a.lock().to_vec(),
            other => vec![other],
        };
        to_value(&super::call::yield_to_block(&args)?)
    }

    fn rb_yield_splat_kw(ary: Value, _kw: c_int) -> Value {
        unsafe { Ok(rb_yield_splat(ary)) }
    }

    /// `rb_yield_block(_, _, argc, argv, _)`: `rb_block_call_func_t`'s own
    /// shape, used as a body that just yields on. The first, second and
    /// fifth arguments are the callback plumbing and carry nothing here.
    fn rb_yield_block(
        _yielded: Value,
        _arg: Value,
        argc: c_int,
        argv: *const Value,
        _blockarg: Value,
    ) -> Value {
        let args = unsafe { args_of(argc, argv) };
        to_value(&super::call::yield_to_block(&args)?)
    }

    /// `rb_block_proc()`: the current frame's block, as a `Proc`.
    fn rb_block_proc() -> Value {
        match super::call::current_block() {
            Some(b) => to_value(&b),
            None => Err(crate::builtins::arg_error!("tried to create Proc object without a block")),
        }
    }

    /// `rb_block_lambda()`: the same, as a LAMBDA -- so `return` inside it
    /// returns from the block rather than from the enclosing method.
    fn rb_block_lambda() -> Value {
        let b = unsafe { rb_block_proc() };
        let p = unsafe { value_of(b) };
        to_value(&send(&p, "lambda", &[]).unwrap_or(p))
    }

    /// `rb_need_block()`: raise unless the frame has one. An extension calls
    /// it first thing in a method that will `rb_yield`.
    fn rb_need_block() -> () {
        if super::call::current_block().is_none() {
            return Err(crate::builtins::local_jump_error!("no block given (yield)"));
        }
        Ok(())
    }

    /// `rb_keyword_given_p()`: did the call site write `k: v`? zeo peels a
    /// trailing Hash into keywords at the call, so by the time a C body
    /// runs, the distinction is gone. False is the answer that makes a
    /// caller take the positional path, which is the one that works.
    fn rb_keyword_given_p() -> c_int {
        Ok(0)
    }

    /// `rb_proc_new(f, arg)`: a `Proc` over a C function.
    fn rb_proc_new(f: BlockFunc, arg: Value) -> Value {
        // SAFETY: the caller's own function, in the loaded image.
        to_value(&RubyValue::Proc(unsafe { super::thread::block_proc(f, arg) }))
    }

    fn rb_proc_arity(p: Value) -> c_int {
        let proc = unsafe { value_of(p) };
        match send(&proc, "arity", &[])? {
            RubyValue::Int(n) => Ok(n as c_int),
            other => Err(wrong_arg_type(&other, "Integer")),
        }
    }

    /// `rb_proc_call(proc, args)`: the arguments arrive in an ARRAY, where
    /// `Proc#call` takes them splatted. That is why it cannot be forwarded.
    fn rb_proc_call(p: Value, args: Value) -> Value {
        let proc = unsafe { value_of(p) };
        let list = match unsafe { value_of(args) } {
            RubyValue::Array(a) => a.lock().to_vec(),
            RubyValue::Nil => Vec::new(),
            other => vec![other],
        };
        to_value(&crate::dispatch::send_value(&proc, Symbol::intern("call"), &list, None)?)
    }

    fn rb_proc_call_kw(p: Value, args: Value, _kw: c_int) -> Value {
        unsafe { Ok(rb_proc_call(p, args)) }
    }

    fn rb_proc_call_with_block_kw(
        p: Value,
        argc: c_int,
        argv: *const Value,
        block: Value,
        _kw: c_int,
    ) -> Value {
        unsafe { Ok(super::misc::rb_proc_call_with_block(p, argc, argv, block)) }
    }

    // ---- Method objects --------------------------------------------------

    fn rb_method_call(argc: c_int, argv: *const Value, m: Value) -> Value {
        let meth = unsafe { value_of(m) };
        let args = unsafe { args_of(argc, argv) };
        to_value(&crate::dispatch::send_value(&meth, Symbol::intern("call"), &args, None)?)
    }

    fn rb_method_call_kw(argc: c_int, argv: *const Value, m: Value, _kw: c_int) -> Value {
        unsafe { Ok(rb_method_call(argc, argv, m)) }
    }

    fn rb_method_call_with_block(argc: c_int, argv: *const Value, m: Value, block: Value) -> Value {
        let meth = unsafe { value_of(m) };
        let args = unsafe { args_of(argc, argv) };
        let b = match unsafe { value_of(block) } {
            RubyValue::Nil => None,
            other => Some(other),
        };
        to_value(&crate::dispatch::send_value(&meth, Symbol::intern("call"), &args, b)?)
    }

    fn rb_method_call_with_block_kw(
        argc: c_int,
        argv: *const Value,
        m: Value,
        block: Value,
        _kw: c_int,
    ) -> Value {
        unsafe { Ok(rb_method_call_with_block(argc, argv, m, block)) }
    }

    /// `rb_method_boundp(klass, id, ex)`: is the method defined? `ex` nonzero
    /// excludes private ones, which is `public_method_defined?`.
    fn rb_method_boundp(klass: Value, id: Id, ex: c_int) -> c_int {
        let k = unsafe { value_of(klass) };
        let meth = if ex != 0 {
            "public_method_defined?"
        } else {
            "method_defined?"
        };
        let name = RubyValue::Symbol(symbol_of(id));
        let out = send(&k, meth, std::slice::from_ref(&name))
            .or_else(|_| send(&k, "private_method_defined?", &[name]))?;
        Ok(c_int::from(super::convert::truthy(to_value(&out)?)))
    }

    /// `rb_method_basic_definition_p(klass, id)`: is the method still the one
    /// Ruby shipped? A gem checks it before taking a fast path that would be
    /// wrong if the method had been redefined.
    ///
    /// zeo cannot tell a redefined builtin from an original one through any
    /// public table, so the honest answer is 0 -- "assume redefined", which
    /// sends every caller down the slow, always-correct path.
    fn rb_method_basic_definition_p(_klass: Value, _id: Id) -> c_int {
        Ok(0)
    }

    // ---- catch and throw -------------------------------------------------

    /// `rb_catch(tag, f, arg)`: a NULL tag means a fresh anonymous object,
    /// which is what `catch {}` with no argument does.
    fn rb_catch(tag: *const c_char, f: Option<BlockFunc>, arg: Value) -> Value {
        let t = if tag.is_null() {
            RubyValue::Nil
        } else {
            crate::builtins::string::str_value_in_enc(crate::encoding::UTF_8, &unsafe { cstr(tag) })
        };
        catch_with(t, f, arg)
    }

    fn rb_catch_obj(tag: Value, f: Option<BlockFunc>, arg: Value) -> Value {
        catch_with(unsafe { value_of(tag) }, f, arg)
    }

    fn rb_throw(tag: *const c_char, v: Value) -> () {
        let t = crate::builtins::string::str_value_in_enc(
            crate::encoding::UTF_8,
            &unsafe { cstr(tag) },
        );
        Err(thrown(t, unsafe { value_of(v) }))
    }

    fn rb_throw_obj(tag: Value, v: Value) -> () {
        Err(thrown(unsafe { value_of(tag) }, unsafe { value_of(v) }))
    }

    /// `rb_iter_break()`: leave the enclosing block, as Ruby's `break` does.
    fn rb_iter_break() -> () {
        Err(Signal::Break(RubyValue::Nil))
    }

    fn rb_iter_break_value(v: Value) -> () {
        Err(Signal::Break(unsafe { value_of(v) }))
    }

    // ---- eval, require, load ---------------------------------------------

    /// `rb_eval_string(src)`: compile and run, through zeo's own compiler.
    /// There is no interpreter behind this.
    fn rb_eval_string(src: *const c_char) -> Value {
        eval_str(&unsafe { cstr(src) })
    }

    /// `rb_eval_string_protect(src, &state)`: the same, with a raise turned
    /// into a nonzero `*state` and a readable `rb_errinfo`.
    fn rb_eval_string_protect(src: *const c_char, state: *mut c_int) -> Value {
        let text = unsafe { cstr(src) };
        protect_into(state, || eval_str(&text))
    }

    /// `rb_eval_string_wrap(src, &state)`: MRI runs the source in an
    /// anonymous module, so its constants and methods do not reach the top
    /// level. zeo has no wrapping cref for a run-time eval, so this is
    /// `rb_eval_string_protect` -- and a definition the source makes IS
    /// visible afterwards, which is the divergence.
    fn rb_eval_string_wrap(src: *const c_char, state: *mut c_int) -> Value {
        unsafe { Ok(rb_eval_string_protect(src, state)) }
    }

    /// `rb_eval_cmd_kw(cmd, arg, kw)`: run a String as code, or call a Proc.
    /// MRI accepts both, and which one it is decides everything.
    fn rb_eval_cmd_kw(cmd: Value, arg: Value, _kw: c_int) -> Value {
        let c = unsafe { value_of(cmd) };
        match &c {
            RubyValue::Str(s) => {
                let text = s.lock().to_utf8_lossy().into_owned();
                eval_str(&text)
            }
            _ => {
                let args = match unsafe { value_of(arg) } {
                    RubyValue::Array(a) => a.lock().to_vec(),
                    RubyValue::Nil => Vec::new(),
                    other => vec![other],
                };
                to_value(&crate::dispatch::send_value(&c, Symbol::intern("call"), &args, None)?)
            }
        }
    }

    fn rb_require(feature: *const c_char) -> Value {
        require_named(&unsafe { cstr(feature) })
    }

    fn rb_require_string(feature: Value) -> Value {
        let f = unsafe { value_of(feature) };
        to_value(&crate::dispatch::send_value(
            &main_recv(),
            Symbol::intern("require"),
            &[f],
            None,
        )?)
    }

    fn rb_f_require(_recv: Value, feature: Value) -> Value {
        unsafe { Ok(rb_require_string(feature)) }
    }

    /// `rb_provide(feature)`: mark a feature loaded without loading it, so a
    /// later `require` is a no-op. An extension calls it for the file it IS.
    fn rb_provide(feature: *const c_char) -> () {
        let name = unsafe { cstr(feature) };
        crate::features::feature_loaded(0, &name, &name);
        Ok(())
    }

    /// `rb_provided(feature)`: has it been required? `$LOADED_FEATURES` is
    /// the list Ruby answers from, so this reads that rather than the
    /// compile-time unit table -- a feature the program never compiled in
    /// can still have been loaded from disk.
    fn rb_provided(feature: *const c_char) -> c_int {
        let name = unsafe { cstr(feature) };
        Ok(c_int::from(is_provided(&name)))
    }

    fn rb_feature_provided(feature: *const c_char, _loading: *mut *const c_char) -> c_int {
        unsafe { Ok(rb_provided(feature)) }
    }

    /// `rb_load(path, wrap)`: `Kernel#load`.
    fn rb_load(path: Value, wrap: c_int) -> () {
        let p = unsafe { value_of(path) };
        crate::dispatch::send_value(
            &main_recv(),
            Symbol::intern("load"),
            &[p, RubyValue::Bool(wrap != 0)],
            None,
        )?;
        Ok(())
    }

    fn rb_load_protect(path: Value, wrap: c_int, state: *mut c_int) -> () {
        let out = protect_into(state, || {
            unsafe { rb_load(path, wrap) };
            Ok(value::Q_NIL)
        });
        out.map(|_| ())
    }

    // ---- frames ----------------------------------------------------------

    /// `rb_frame_this_func` and `rb_frame_callee` differ in MRI -- defined
    /// name against called name -- and an alias is what separates them.
    /// zeo's frame carries one label, so both answer it.
    fn rb_frame_this_func() -> Id {
        Ok(frame_name())
    }

    fn rb_frame_callee() -> Id {
        Ok(frame_name())
    }

    /// `rb_frame_method_id_and_class(&id, &klass)`: both at once. Answers 0
    /// when there is no method frame, which is how a caller tells top level
    /// from inside a method.
    fn rb_frame_method_id_and_class(id: *mut Id, klass: *mut Value) -> c_int {
        let name = frame_name();
        if !id.is_null() {
            // SAFETY: the caller's own `ID` slot.
            unsafe { id.write(name) };
        }
        if !klass.is_null() {
            // zeo's frame does not record the DEFINING class, only the
            // label. `Object` is the honest stand-in: every class is under
            // it, so a caller testing `<=` against its own class still gets
            // a usable answer, and one printing it gets a name.
            // SAFETY: the caller's own `VALUE` slot.
            unsafe { klass.write(to_value(&RubyValue::Class(zeo_abi::OBJECT_CLASS))?) };
        }
        Ok(c_int::from(name != 0))
    }

    /// `rb_current_receiver()`: the `self` of the calling Ruby frame. zeo's
    /// frame stack records a label and a location, not the receiver, so the
    /// one receiver always reachable is the top-level `main`.
    fn rb_current_receiver() -> Value {
        to_value(&main_recv())
    }

    fn rb_sourcefile() -> *const c_char {
        let file = crate::frames::current_location().map_or("", |(f, _)| f);
        Ok(super::symbol::cstr_for_owned(file))
    }

    fn rb_sourceline() -> c_int {
        Ok(crate::frames::current_location().map_or(0, |(_, l)| l as c_int))
    }

    fn rb_make_backtrace() -> Value {
        let lines: Vec<RubyValue> = crate::frames::capture_backtrace()
            .iter()
            .map(|l| crate::builtins::string::str_value_in_enc(crate::encoding::UTF_8, l))
            .collect();
        to_value(&RubyValue::Array(crate::value::collections::array_new(lines)))
    }

    /// `rb_backtrace()`: MRI PRINTS the backtrace to stderr rather than
    /// answering it. The two are one line apart and confusing them would
    /// print nothing where a gem expected output.
    fn rb_backtrace() -> () {
        for line in crate::frames::capture_backtrace() {
            let text = format!("\tfrom {line}\n");
            let _ = crate::builtins::io::write_bytes(
                &crate::builtins::io::current_stderr(),
                text.as_bytes(),
            );
        }
        Ok(())
    }

    /// `rb_make_exception(argc, argv)`: build an exception the way `raise`'s
    /// argument list does -- a String is a RuntimeError, a class is `new`ed,
    /// an instance is itself.
    fn rb_make_exception(argc: c_int, argv: *const Value) -> Value {
        let args = unsafe { args_of(argc, argv) };
        to_value(&match args.first() {
            None => RubyValue::Nil,
            Some(RubyValue::Str(s)) => {
                let msg = s.lock().to_utf8_lossy().into_owned();
                match crate::builtins::runtime_error!("{}", msg) {
                    Signal::Raise(e) => e,
                    other => return Err(other),
                }
            }
            Some(first @ RubyValue::Class(_)) => {
                send(first, "exception", &args[1..])?
            }
            Some(other) => send(other, "exception", &args[1..])?,
        })
    }

    // ---- the special globals ---------------------------------------------

    fn rb_backref_get() -> Value {
        to_value(&crate::globals::global_get(0, "$~"))
    }

    fn rb_backref_set(v: Value) -> () {
        crate::globals::global_set(0, "$~", unsafe { value_of(v) });
        Ok(())
    }

    fn rb_lastline_get() -> Value {
        to_value(&crate::globals::global_get(0, "$_"))
    }

    fn rb_lastline_set(v: Value) -> () {
        crate::globals::global_set(0, "$_", unsafe { value_of(v) });
        Ok(())
    }

    fn rb_f_global_variables() -> Value {
        to_value(&send(&main_recv(), "global_variables", &[])?)
    }

    // ---- enumerators -----------------------------------------------------

    /// `rb_enumeratorize(obj, meth, argc, argv)`: the Enumerator a method
    /// answers when it is called without a block. Every `return
    /// RETURN_ENUMERATOR(...)` in a gem reduces to this.
    fn rb_enumeratorize(obj: Value, meth: Value, argc: c_int, argv: *const Value) -> Value {
        enumeratorize(obj, meth, argc, argv)
    }

    fn rb_enumeratorize_with_size(
        obj: Value,
        meth: Value,
        argc: c_int,
        argv: *const Value,
        _size: *const c_void,
    ) -> Value {
        enumeratorize(obj, meth, argc, argv)
    }

    fn rb_enumeratorize_with_size_kw(
        obj: Value,
        meth: Value,
        argc: c_int,
        argv: *const Value,
        _size: *const c_void,
        _kw: c_int,
    ) -> Value {
        enumeratorize(obj, meth, argc, argv)
    }

    /// `rb_enum_values_pack(argc, argv)`: one argument is itself, and
    /// several become an Array. That is how a block with one parameter sees
    /// a multi-value yield.
    fn rb_enum_values_pack(argc: c_int, argv: *const Value) -> Value {
        let args = unsafe { args_of(argc, argv) };
        match args.len() {
            0 => Ok(value::Q_NIL),
            1 => to_value(&args[0]),
            _ => to_value(&RubyValue::Array(crate::value::collections::array_new(args))),
        }
    }

    // ---- recursion guards ------------------------------------------------

    /// `rb_exec_recursive(f, obj, arg)`: run `f`, and if `obj` is already
    /// being visited, run it with `recur = 1` instead. This is what stops
    /// `[a].tap { a << it }.inspect` from looping forever.
    fn rb_exec_recursive(
        f: unsafe extern "C" fn(Value, Value, c_int) -> Value,
        obj: Value,
        arg: Value,
    ) -> Value {
        exec_recursive(f, obj, 0, arg)
    }

    /// `rb_exec_recursive_outer`: on recursion it does not call `f` at all,
    /// it throws to the OUTERMOST frame. `Array#hash` relies on that.
    fn rb_exec_recursive_outer(
        f: unsafe extern "C" fn(Value, Value, c_int) -> Value,
        obj: Value,
        arg: Value,
    ) -> Value {
        exec_recursive(f, obj, 0, arg)
    }

    fn rb_exec_recursive_paired(
        f: unsafe extern "C" fn(Value, Value, c_int) -> Value,
        obj: Value,
        paired: Value,
        arg: Value,
    ) -> Value {
        exec_recursive(f, obj, paired, arg)
    }

    fn rb_exec_recursive_paired_outer(
        f: unsafe extern "C" fn(Value, Value, c_int) -> Value,
        obj: Value,
        paired: Value,
        arg: Value,
    ) -> Value {
        exec_recursive(f, obj, paired, arg)
    }
}

/// `Signal::Throw` carries a boxed pair, so the two arguments meet here.
fn thrown(tag: RubyValue, value: RubyValue) -> Signal {
    Signal::Throw(Box::new(crate::signal::Thrown { tag, value }))
}

/// Is `feature` in `$LOADED_FEATURES`? MRI answers `rb_provided` from that
/// list, and matches a bare name against a path's basename.
fn is_provided(feature: &str) -> bool {
    let RubyValue::Array(loaded) = crate::globals::global_get(0, "$LOADED_FEATURES") else {
        return crate::features::has_feature(feature);
    };
    let want = feature.trim_end_matches(".rb");
    let hit = loaded.lock().iter().any(|v| match v {
        RubyValue::Str(s) => {
            let path = s.lock().to_utf8_lossy().into_owned();
            let stem = path.trim_end_matches(".rb");
            stem == want || stem.ends_with(&format!("/{want}"))
        }
        _ => false,
    });
    hit || crate::features::has_feature(feature)
}

fn eval_str(src: &str) -> Result<Value, Signal> {
    let text = crate::builtins::string::str_value_in_enc(crate::encoding::UTF_8, src);
    to_value(&crate::dispatch::send_value(
        &main_recv(),
        Symbol::intern("eval"),
        &[text],
        None,
    )?)
}

fn require_named(feature: &str) -> Result<Value, Signal> {
    let name = crate::builtins::string::str_value_in_enc(crate::encoding::UTF_8, feature);
    to_value(&crate::dispatch::send_value(
        &main_recv(),
        Symbol::intern("require"),
        &[name],
        None,
    )?)
}

/// `rb_protect`'s shape, for the `_protect` spellings: a raise becomes a
/// nonzero `*state` and a readable `rb_errinfo`.
fn protect_into(
    state: *mut c_int,
    body: impl FnOnce() -> Result<Value, Signal>,
) -> Result<Value, Signal> {
    let out = super::jmp::protect(body);
    let failed = matches!(out, Ok(Err(_)) | Err(_));
    if !state.is_null() {
        // SAFETY: the caller's own `int` slot.
        unsafe { state.write(c_int::from(failed)) };
    }
    match out {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(sig)) | Err(sig) => {
            super::call::set_errinfo(&sig);
            Ok(value::Q_NIL)
        }
    }
}

fn catch_with(tag: RubyValue, f: Option<BlockFunc>, arg: Value) -> Result<Value, Signal> {
    let Some(f) = f else {
        return Ok(value::Q_NIL);
    };
    // SAFETY: the caller's own function, in the loaded image.
    let block = RubyValue::Proc(unsafe { super::thread::block_proc(f, arg) });
    let args = if matches!(tag, RubyValue::Nil) {
        Vec::new()
    } else {
        vec![tag]
    };
    to_value(&crate::dispatch::send_value(
        &main_recv(),
        Symbol::intern("catch"),
        &args,
        Some(block),
    )?)
}

/// The current frame's method name as an `ID`, or 0 at top level.
fn frame_name() -> Id {
    match crate::frames::current_frame_method() {
        Some(name) if !name.is_empty() && name != "<main>" => Symbol::intern(name).to_u32() as Id,
        _ => 0,
    }
}

fn enumeratorize(
    obj: Value,
    meth: Value,
    argc: c_int,
    argv: *const Value,
) -> Result<Value, Signal> {
    let recv = unsafe { value_of(obj) };
    let name = unsafe { value_of(meth) };
    let mut args = vec![name];
    args.extend(unsafe { args_of(argc, argv) });
    to_value(&send(&recv, "enum_for", &args)?)
}

thread_local! {
    /// The `(obj, paired)` pairs a `rb_exec_recursive` walk is inside. MRI
    /// keeps the same set per thread, keyed the same way.
    static VISITING: std::cell::RefCell<Vec<(Value, Value)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn exec_recursive(
    f: unsafe extern "C" fn(Value, Value, c_int) -> Value,
    obj: Value,
    paired: Value,
    arg: Value,
) -> Result<Value, Signal> {
    let seen = VISITING.with_borrow(|v| v.contains(&(obj, paired)));
    if seen {
        // SAFETY: the caller's own function; `recur = 1` is MRI's flag.
        return super::jmp::protect(|| unsafe { f(obj, arg, 1) });
    }
    VISITING.with_borrow_mut(|v| v.push((obj, paired)));
    // SAFETY: as above.
    let out = super::jmp::protect(|| unsafe { f(obj, arg, 0) });
    VISITING.with_borrow_mut(|v| {
        v.retain(|p| *p != (obj, paired));
    });
    out
}

//! `FFI::Function` and `FFI::VariadicInvoker` -- the gem's RUNTIME call tier.
//! A `Function` is a callable C function pointer whose signature is data, not
//! a compile-time `extern` declaration: built from a code address (fiddle's
//! path) or from a Ruby `Proc` (a libffi closure trampoline, so the object
//! doubles as a C callback). `Function < Pointer` in the gem's hierarchy, so
//! instances ARE `RPointer`s (`base` = the code address) carrying a `FuncData`
//! payload -- every `Pointer` method (`to_i`, `null?`, ...) works unchanged.
//!
//! The compile-time `attach_function` path never constructs these; it emits a
//! direct `extern "C"` call. fiddle's pure-Ruby FFI backend is the consumer.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::types::{is_varargs_type, kind_of_type_value};
use super::{RPointer, address_of, ptr_of};
use crate::builtins::{arg_error, arity, type_error};
use crate::dispatch::{RObj, RubyObject};
use crate::ffi::{
    CallbackHandle, FfiKind, call_fixed, call_variadic, make_callback, marshal_fixed,
    take_callback_error, va_promoted,
};
use crate::{ClassId, RubyValue, Signal};
use zeo_abi::{FFI_FUNCTION_CLASS, FFI_VARIADIC_INVOKER_CLASS};
use zeo_macros::ruby_class;

/// An `FFI::Function`'s call signature, plus -- for a Proc-backed function --
/// the live libffi closure whose trampoline the pointer's `base` addresses.
pub struct FuncData {
    arg_kinds: Vec<FfiKind>,
    ret: FfiKind,
    _closure: Option<CallbackHandle>,
}

// The closure's trampoline and data are heap-stable for the handle's
// lifetime; invoking a C function pointer from any thread is the same
// contract the gem places on its functions.
unsafe impl Send for FuncData {}
unsafe impl Sync for FuncData {}

/// The declared argument kinds for a `new`'s type-array argument.
fn arg_kinds_of(v: &RubyValue) -> Result<Vec<FfiKind>, Signal> {
    let RubyValue::Array(a) = v else {
        return Err(type_error!(
            "wrong argument type {} (expected Array)",
            crate::builtins::class_name_of(v)
        ));
    };
    a.lock().iter().map(kind_of_type_value).collect()
}

/// The code address a `new` target names: a `Pointer`/`Function`, or a raw
/// Integer address.
fn target_address(v: &RubyValue) -> Result<usize, Signal> {
    if let Some(addr) = address_of(v) {
        return Ok(addr);
    }
    if let RubyValue::Int(i) = v {
        return Ok(*i as usize);
    }
    Err(type_error!(
        "wrong argument type {} (expected a pointer or Proc)",
        crate::builtins::class_name_of(v)
    ))
}

ruby_class! {
    Function = zeo_abi::FFI_FUNCTION_CLASS < zeo_abi::FFI_POINTER_CLASS;

    // `FFI::Function.new(return_type, arg_types, pointer_or_proc, options?)`.
    // The options (`convention:`) name ABIs this platform doesn't distinguish.
    def self."new"(_recv, *args, &_b) {
        arity!(args, 3..=4);
        let ret = kind_of_type_value(&args[0])?;
        let arg_kinds = arg_kinds_of(&args[1])?;
        let (base, closure) = match &args[2] {
            p @ RubyValue::Proc(_) => {
                let handle = make_callback(p, &arg_kinds, ret)?;
                (handle.code_ptr() as *mut u8, Some(handle))
            }
            other => (target_address(other)? as *mut u8, None),
        };
        Ok(RubyValue::Object(Arc::new(RPointer {
            base,
            size: None,
            owner: None,
            class: FFI_FUNCTION_CLASS,
            func: Some(Arc::new(FuncData { arg_kinds, ret, _closure: closure })),
            frozen: AtomicBool::new(false),
        })))
    }

    def "call"(recv, *args, &_b) {
        let p = ptr_of(recv);
        let f = p.func.as_ref().expect("the Function table only dispatches on Function receivers");
        if args.len() != f.arg_kinds.len() {
            return Err(arg_error!(
                "wrong number of arguments (given {}, expected {})",
                args.len(),
                f.arg_kinds.len()
            ));
        }
        let vals = f
            .arg_kinds
            .iter()
            .zip(args)
            .map(|(k, v)| marshal_fixed(*k, v))
            .collect::<Result<Vec<_>, _>>()?;
        let out = unsafe { call_fixed(p.base as *const _, vals, f.ret) }?;
        // A closure the C function invoked may have raised; surface it now.
        take_callback_error()?;
        Ok(out)
    }

    // The gem frees the libffi closure eagerly; ours lives as long as the
    // object (freed-state bookkeeping is the Ruby `Fiddle::Closure`'s).
    def "free"(_recv, *args, &_b) {
        arity!(args, 0);
        Ok(RubyValue::Nil)
    }
}

/// An `FFI::VariadicInvoker`'s payload: the target plus the FIXED prototype;
/// each `#call` supplies the trailing `(type, value)` pairs.
pub struct RVarInvoker {
    addr: usize,
    fixed: Vec<FfiKind>,
    ret: FfiKind,
    frozen: AtomicBool,
}

impl RubyObject for RVarInvoker {
    fn class_id(&self) -> ClassId {
        FFI_VARIADIC_INVOKER_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn set_frozen(&self) {
        self.frozen
            .store(true, std::sync::atomic::Ordering::Relaxed)
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let d = RVarInvoker {
            addr: self.addr,
            fixed: self.fixed.clone(),
            ret: self.ret,
            frozen: AtomicBool::new(false),
        };
        if copy_frozen {
            d.set_frozen();
        }
        Arc::new(d)
    }
}

fn inv_of(recv: &RubyValue) -> &RVarInvoker {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RVarInvoker>()
            .expect("the VariadicInvoker table only dispatches on invoker receivers"),
        _ => unreachable!("the VariadicInvoker table only dispatches on invoker receivers"),
    }
}

// The DSL emits one `install_constants` per block, so the second class lives
// in its own module.
mod invoker {
    use super::*;

    ruby_class! {
    VariadicInvoker = zeo_abi::FFI_VARIADIC_INVOKER_CLASS < zeo_abi::OBJECT_CLASS;

    // `FFI::VariadicInvoker.new(pointer, arg_types, return_type, options?)`.
    // `arg_types` ends with the `VARARGS` marker; everything before it is the
    // fixed prototype.
    def self."new"(_recv, *args, &_b) {
        arity!(args, 3..=4);
        let addr = target_address(&args[0])?;
        let RubyValue::Array(a) = &args[1] else {
            return Err(type_error!(
                "wrong argument type {} (expected Array)",
                crate::builtins::class_name_of(&args[1])
            ));
        };
        let types: Vec<RubyValue> = a.lock().iter().cloned().collect();
        let fixed = types
            .iter()
            .filter(|t| !is_varargs_type(t))
            .map(kind_of_type_value)
            .collect::<Result<Vec<_>, _>>()?;
        let ret = kind_of_type_value(&args[2])?;
        Ok(RubyValue::Object(Arc::new(RVarInvoker {
            addr,
            fixed,
            ret,
            frozen: AtomicBool::new(false),
        })))
    }

    // `call(*fixed_values, type, value, type, value, ...)` -- the trailing
    // pairs carry each variadic argument's type alongside its value, and get
    // the C default argument promotions.
    def "call"(recv, *args, &_b) {
        let inv = inv_of(recv);
        let n = inv.fixed.len();
        if args.len() < n || !(args.len() - n).is_multiple_of(2) {
            return Err(arg_error!(
                "variadic arguments must be (type, value) pairs after {n} fixed arguments"
            ));
        }
        let mut vals = Vec::with_capacity(n + (args.len() - n) / 2);
        for (k, v) in inv.fixed.iter().zip(&args[..n]) {
            vals.push(marshal_fixed(*k, v)?);
        }
        let mut i = n;
        while i < args.len() {
            let kind = kind_of_type_value(&args[i])?;
            vals.push(va_promoted(kind, &args[i + 1])?);
            i += 2;
        }
        let out = unsafe { call_variadic(inv.addr as *const _, n, vals, inv.ret) }?;
        take_callback_error()?;
        Ok(out)
    }
    }
}

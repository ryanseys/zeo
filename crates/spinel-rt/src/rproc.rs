//! The real, escaping `Proc`/closure type (Phase 6) -- deliberately a plain
//! Rust closure type, not a hand-rolled trait + per-call-site env struct: a
//! `move |args| { ... }` closure generated at each escaping block's call
//! site already IS a concrete, heap-allocated environment (Rust's own
//! compiler builds it for us), so there is nothing left for a hand-rolled
//! `ProcBody` trait to add except ceremony. `redo` is handled entirely
//! inside the closure body (a plain `loop` around the block's own code, see
//! `codegen`'s Proc-construction docs) -- no runtime support needed beyond
//! `Signal::Redo` already existing.
//!
//! Must be `'static`: `RubyValue` (which stores this) has no lifetime
//! parameter anywhere in this codebase, so anything it holds has to be
//! independently owned, not borrowed -- this is exactly why an escaping
//! block captures OWNED `Arc<parking_lot::Mutex<RubyValue>>` cells (and an
//! owned `Arc<Self>` for `self`/ivar access) rather than references.
//!
//! `+ Send + Sync` (Part 9): a Rust closure is automatically `Send`/`Sync`
//! based purely on what it captures -- once every capture is `Arc`/`Mutex`-
//! based, the closures codegen generates satisfy this bound with no manual
//! annotation needed at the construction site.

use crate::{RubyValue, Signal};
use std::sync::Arc;

/// The closure a `Proc` value wraps, plus the two facts about its
/// PARAMETERS that a Rust closure can't answer for itself but Ruby exposes:
/// `Proc#arity` and `Proc#lambda?` (and `#curry`, which needs the arity).
/// Codegen fills them in from the block/lambda's static `Params` at every
/// literal construction site; runtime-internal procs (Enumerator shuttles,
/// `Symbol#to_proc`, ...) use `RProc::new`'s var-args default, which is
/// what CRuby reports for a comparable C-implemented proc anyway.
pub struct ProcData {
    /// Takes the `self` to run under as its FIRST parameter rather than
    /// capturing it, which is what makes `instance_exec` possible: a Ruby
    /// block's self is not fixed at creation: `obj.instance_exec { @x }`
    /// runs this same proc body under a DIFFERENT receiver. A captured
    /// `self` could only be rebound by mutating shared state (every clone
    /// of the proc shares one `Arc<ProcData>`, so that would race, and a
    /// save/restore around the call would corrupt any concurrent use). A
    /// parameter is immutable, reentrant, and thread-safe by construction.
    f: Box<dyn Fn(&RubyValue, &[RubyValue]) -> Result<RubyValue, Signal> + Send + Sync>,
    /// The block's LEXICAL self -- the receiver `#call` runs under, i.e.
    /// what `self` meant where the block was written. `instance_exec`
    /// bypasses it; everything else uses it.
    self_val: RubyValue,
    /// CRuby's encoding: a required-only signature is the positive count;
    /// any optional/rest param makes it `-(required + 1)`.
    pub arity: i32,
    pub is_lambda: bool,
}

/// A `Proc` value's payload. A newtype over `Arc<ProcData>` rather than the
/// bare `Arc<dyn Fn>` it started as -- `Deref` to the closure keeps every
/// existing `p(&args)` call site working unchanged (a call expression
/// auto-dereferences its callee), while giving the value somewhere to carry
/// `arity`/`is_lambda`.
#[derive(Clone)]
pub struct RProc(Arc<ProcData>);

impl RProc {
    /// A runtime-internal proc: var-args arity (`-1`), not a lambda. Its
    /// body has no Ruby `self` to speak of (Enumerator shuttles,
    /// `Symbol#to_proc`, ...), so it ignores the receiver and reports nil as
    /// its lexical self.
    pub fn new(
        f: impl Fn(&[RubyValue]) -> Result<RubyValue, Signal> + Send + Sync + 'static,
    ) -> RProc {
        RProc(Arc::new(ProcData {
            f: Box::new(move |_self, args| f(args)),
            self_val: RubyValue::Nil,
            arity: -1,
            is_lambda: false,
        }))
    }

    /// A proc built from Ruby source, whose `Params` codegen knows, and
    /// whose body never mentions `self` (no ivars, no implicit-self call) --
    /// so there is nothing for `instance_exec` to rebind.
    pub fn with_meta(
        f: impl Fn(&[RubyValue]) -> Result<RubyValue, Signal> + Send + Sync + 'static,
        arity: i32,
        is_lambda: bool,
    ) -> RProc {
        RProc(Arc::new(ProcData {
            f: Box::new(move |_self, args| f(args)),
            self_val: RubyValue::Nil,
            arity,
            is_lambda,
        }))
    }

    /// A proc built from Ruby source whose body DOES use `self` -- codegen
    /// emits this form, passing the block's lexical self as `self_val`. The
    /// closure reads its receiver from the parameter, so `instance_exec` can
    /// supply a different one (see `call_with_self`).
    pub fn with_self(
        f: impl Fn(&RubyValue, &[RubyValue]) -> Result<RubyValue, Signal> + Send + Sync + 'static,
        self_val: RubyValue,
        arity: i32,
        is_lambda: bool,
    ) -> RProc {
        RProc(Arc::new(ProcData {
            f: Box::new(f),
            self_val,
            arity,
            is_lambda,
        }))
    }

    /// Invoke under the block's own lexical self -- ordinary `#call`/`yield`.
    pub fn call(&self, args: &[RubyValue]) -> Result<RubyValue, Signal> {
        (self.0.f)(&self.0.self_val, args)
    }

    /// Invoke with `self` REBOUND to `recv` -- `instance_exec`/`instance_eval`.
    /// Leaves this proc untouched and is safe to call concurrently: the
    /// receiver is a parameter, never stored.
    pub fn call_with_self(
        &self,
        recv: &RubyValue,
        args: &[RubyValue],
    ) -> Result<RubyValue, Signal> {
        (self.0.f)(recv, args)
    }

    /// The block's lexical self -- `Proc#binding`-adjacent reflection, and
    /// what `instance_exec` restores nothing to (it simply doesn't consult it).
    pub fn self_val(&self) -> &RubyValue {
        &self.0.self_val
    }

    pub fn arity(&self) -> i32 {
        self.0.arity
    }

    pub fn is_lambda(&self) -> bool {
        self.0.is_lambda
    }

    /// Pointer identity -- `Proc#==`/`#equal?` and Ractor sharability
    /// checks compare the underlying allocation, not the closure's
    /// behavior.
    pub fn ptr_eq(&self, other: &RProc) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// This proc's allocation address, as `Hash`-key identity and
    /// `#object_id` need it -- the same notion `ptr_eq` compares, exposed as
    /// a value. (Previously spelled `&**p` at the call sites, via a `Deref`
    /// that no longer exists.)
    pub fn ptr_id(&self) -> usize {
        Arc::as_ptr(&self.0) as *const () as usize
    }
}

// No `Deref` to the inner closure (it existed to keep bare `p(&args)` call
// sites working): the closure now takes `self` first, so a call expression
// can't stand in for a decision about WHICH receiver to run under. Every
// invocation goes through `call` (lexical self) or `call_with_self`
// (rebound) and thereby states which one it means.

/// Pack a logical tuple into ONE `RubyValue::Array` before invoking `p` --
/// the uniform poly-array ABI every pair/tuple-yielding iterator uses (CRuby
/// `Hash#each`'s shape). A `{ |k, v| }` block auto-splats the array back to
/// `k, v`; a `{ |pair| }` block and any forwarded callable (`&method(:m)`,
/// `&:sym`) receive the whole array as one argument.
pub fn yield_tuple(p: &RProc, elems: Vec<RubyValue>) -> Result<RubyValue, Signal> {
    p.call(&[RubyValue::Array(crate::array_new(elems))])
}

/// A non-lambda block's AUTO-SPLAT (CRuby `setup_parameters_complex`'s
/// `arg_setup_block` path): a block yielded EXACTLY ONE argument that is an
/// Array (or `to_ary`-coercible) has that array spread across its
/// positional parameters -- `[[1, 2]].each { |a, b| }` binds `a=1, b=2`,
/// the idiom that makes `Hash#each { |k, v| }` and `each_with_index` read
/// naturally.
///
/// The DECISION (which param shapes splat at all) is codegen's, made
/// statically from the block's own `Params` -- see
/// `codegen::params::emit_proc_param_bindings`. This function is only
/// reached once that decision says yes, so it just performs the coercion.
/// A lambda never comes here (strict arity, no auto-splat).
pub fn block_auto_splat(args: Vec<RubyValue>) -> Result<Vec<RubyValue>, Signal> {
    if args.len() != 1 {
        return Ok(args);
    }
    match &args[0] {
        RubyValue::Array(a) => Ok(a.lock().clone()),
        v => {
            let to_ary = crate::Symbol::intern("to_ary");
            // `rb_check_array_type`: a `to_ary` answering a non-Array is
            // simply not a coercion (no TypeError here, unlike a splice's
            // explicit conversion) -- the value binds as one argument.
            if crate::dispatch::responds_to(v.class_id(), to_ary, false) {
                if let RubyValue::Array(a) = crate::dispatch::send_value(v, to_ary, &[], None)? {
                    return Ok(a.lock().clone());
                }
            }
            Ok(args)
        }
    }
}

/// The `**expr` double-splat conversion (CRuby's `rb_to_hash_type`): a Hash
/// passes through, anything else must define `to_hash` and answer a Hash
/// from it -- `take(**opts_object)` is the idiom. Codegen routes every
/// double-splat call-site argument through this.
pub fn to_hash_coerce(v: &RubyValue) -> Result<crate::RHash, Signal> {
    if let RubyValue::Hash(h) = v {
        return Ok(h.clone());
    }
    let to_hash = crate::Symbol::intern("to_hash");
    if crate::dispatch::responds_to(v.class_id(), to_hash, false) {
        if let RubyValue::Hash(h) = crate::dispatch::send_value(v, to_hash, &[], None)? {
            return Ok(h);
        }
    }
    Err(crate::dispatch::raise_error(
        "TypeError",
        format!(
            "no implicit conversion of {} into Hash",
            crate::builtins::class_name_of(v)
        ),
    ))
}

/// The `&expr` block-argument conversion (CRuby's `Proc()` coercion at a
/// call site): a Proc passes through, a Symbol converts via
/// `Symbol#to_proc` (`map(&:to_s)`), nil means "no block", anything else
/// is real Ruby's TypeError. Codegen's `emit_block_option` routes every
/// forwarded block argument through this.
pub fn block_arg_to_proc(
    v: crate::RubyValue,
) -> Result<Option<crate::RubyValue>, crate::Signal> {
    match v {
        crate::RubyValue::Proc(_) => Ok(Some(v)),
        crate::RubyValue::Symbol(s) => {
            Ok(Some(crate::builtins::symbol::symbol_to_proc(s)))
        }
        crate::RubyValue::Nil => Ok(None),
        // Anything else duck-types through `to_proc` (CRuby's
        // `rb_block_arg_to_proc`): a user object defining it (or a
        // `Method` -- its Path-2 row answers one) converts; a non-Proc
        // answer or no `to_proc` at all is the TypeError.
        other => {
            let to_proc = crate::Symbol::intern("to_proc");
            if crate::dispatch::responds_to(other.class_id(), to_proc, false) {
                return match crate::dispatch::send_value(&other, to_proc, &[], None)? {
                    p @ crate::RubyValue::Proc(_) => Ok(Some(p)),
                    bad => Err(crate::dispatch::raise_error(
                        "TypeError",
                        format!(
                            "can't convert {} to Proc ({}#to_proc gives {})",
                            crate::builtins::class_name_of(&other),
                            crate::builtins::class_name_of(&other),
                            crate::builtins::class_name_of(&bad)
                        ),
                    )),
                };
            }
            Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Proc",
                    crate::builtins::class_name_of(&other)
                ),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::{array_new, hash_new, string_new};

    fn proc_of(f: impl Fn(&[RubyValue]) -> Result<RubyValue, Signal> + Send + Sync + 'static) -> RubyValue {
        RubyValue::Proc(RProc::new(f))
    }

    #[test]
    fn a_runtime_proc_reports_var_args_arity_and_is_not_a_lambda() {
        let p = RProc::new(|_| Ok(RubyValue::Nil));
        assert_eq!(p.arity(), -1);
        assert!(!p.is_lambda());
    }

    #[test]
    fn with_meta_carries_the_arity_and_lambda_flag_codegen_computed() {
        let p = RProc::with_meta(|_| Ok(RubyValue::Nil), 2, true);
        assert_eq!(p.arity(), 2);
        assert!(p.is_lambda());
    }

    /// The newtype still CALLS like the bare `Arc<dyn Fn>` it replaced --
    /// a call expression auto-dereferences its callee, which is what keeps
    /// every existing `p(&args)` site working.
    #[test]
    fn a_proc_is_callable_through_the_newtype() {
        let p = RProc::new(|args: &[RubyValue]| Ok(args[0].clone()));
        let out = p.call(&[RubyValue::Int(7)]).unwrap();
        assert_eq!(out.to_display_string(), "7");
    }

    #[test]
    fn ptr_eq_is_allocation_identity_not_behavioral_equality() {
        let a = RProc::new(|_| Ok(RubyValue::Nil));
        let b = RProc::new(|_| Ok(RubyValue::Nil));
        assert!(a.ptr_eq(&a.clone()));
        assert!(!a.ptr_eq(&b));
    }

    // --- block_auto_splat ---------------------------------------------

    #[test]
    fn auto_splat_spreads_a_lone_array_argument() {
        let arg = RubyValue::Array(array_new(vec![RubyValue::Int(1), RubyValue::Int(2)]));
        let out = block_auto_splat(vec![arg]).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].to_display_string(), "1");
        assert_eq!(out[1].to_display_string(), "2");
    }

    #[test]
    fn auto_splat_leaves_a_lone_non_array_alone() {
        let out = block_auto_splat(vec![RubyValue::Int(5)]).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].to_display_string(), "5");
    }

    /// Only a SINGLE argument ever splats -- two yielded values are already
    /// separate arguments and must pass through untouched, even when the
    /// first happens to be an Array.
    #[test]
    fn auto_splat_leaves_multiple_arguments_alone() {
        let arr = RubyValue::Array(array_new(vec![RubyValue::Int(1)]));
        let out = block_auto_splat(vec![arr, RubyValue::Int(9)]).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].inspect_string(), "[1]");
    }

    #[test]
    fn auto_splat_of_no_arguments_is_a_no_op() {
        assert!(block_auto_splat(Vec::new()).unwrap().is_empty());
    }

    #[test]
    fn auto_splat_spreads_an_empty_array_to_nothing() {
        let out = block_auto_splat(vec![RubyValue::Array(array_new(Vec::new()))]).unwrap();
        assert!(out.is_empty());
    }

    // --- block_arg_to_proc --------------------------------------------

    #[test]
    fn a_proc_block_argument_passes_through_unchanged() {
        let p = proc_of(|_| Ok(RubyValue::Int(1)));
        let out = block_arg_to_proc(p).unwrap();
        assert!(matches!(out, Some(RubyValue::Proc(_))));
    }

    #[test]
    fn a_symbol_block_argument_converts_via_symbol_to_proc() {
        let out = block_arg_to_proc(RubyValue::Symbol(crate::Symbol::intern("upcase")))
            .unwrap()
            .expect("a Symbol converts");
        let RubyValue::Proc(p) = out else { panic!("expected a Proc") };
        let s = RubyValue::Str(string_new("hi".to_string()));
        assert_eq!(p.call(&[s]).unwrap().to_display_string(), "HI");
    }

    #[test]
    fn a_nil_block_argument_means_no_block() {
        assert!(block_arg_to_proc(RubyValue::Nil).unwrap().is_none());
    }

    /// A value with no `to_proc` raises TypeError -- surfacing as a panic
    /// here only because these unit tests run without a `ClassRegistry`
    /// installed (the same posture as `builtins::array`'s own tests); the
    /// `duck_conversions` example covers the real rescued message.
    #[test]
    fn a_non_convertible_block_argument_is_a_type_error() {
        let r = std::panic::catch_unwind(|| block_arg_to_proc(RubyValue::Int(1)));
        assert!(r.is_err());
    }

    // --- to_hash_coerce ------------------------------------------------

    #[test]
    fn a_hash_double_splat_passes_through_unchanged() {
        let h = hash_new(vec![(RubyValue::Int(1), RubyValue::Int(2))]);
        let out = to_hash_coerce(&RubyValue::Hash(h.clone())).unwrap();
        assert_eq!(out.lock().len(), 1);
    }

    #[test]
    fn a_non_convertible_double_splat_is_a_type_error() {
        let r = std::panic::catch_unwind(|| to_hash_coerce(&RubyValue::Int(1)));
        assert!(r.is_err());
    }
}

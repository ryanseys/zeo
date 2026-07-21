//! `BasicObject` -- the true root's 8 methods (CRuby object.c). Owns
//! `==`/`!=` (funneling to `rb_eq`, whose recursion guard lives in
//! `value.rs`), `!`, `equal?` (reference identity), `__send__`, and
//! `instance_eval`/`instance_exec`. `__id__` is Tier B.
//!
//! `instance_eval`/`instance_exec` were long documented here as "compile-time
//! rejections (dynamic self-rebinding is a permanent AOT exclusion)". That
//! was conflating two different things: rebinding self in a BLOCK needs no
//! eval and no runtime compilation -- the block is ordinary compiled code,
//! and the only question is which receiver it runs under. Once a proc takes
//! its self as a PARAMETER instead of capturing it (`RProc::with_self`),
//! answering that question is a function call. What stays excluded is the
//! STRING form (`instance_eval("@x + 1")`), which genuinely needs the eval
//! VM -- it raises NotImplementedError below, like every other eval path.

use crate::builtins::{arity, builtin_methods};
use crate::{RubyValue, Signal, Symbol};
use std::sync::Arc;

builtin_methods! {
    pub(crate) fn lookup;

    "==" => fn eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.rb_eq(&args[0])))
    }
    "!=" => fn neq(recv, args, _block) {
        arity!(args, 1);
        // CRuby's `!=` is `!(self == other)` -- it dispatches the receiver's
        // OWN `==` (a Struct's value equality, a user override), not the
        // low-level identity fallback `rb_eq` gives for a plain object.
        let eq = crate::dispatch::send_value(recv, crate::Symbol::intern("=="), args, None)?;
        Ok(RubyValue::Bool(!eq.truthy()))
    }
    "!" => fn not(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(!recv.truthy()))
    }
    "equal?" => fn equal(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(value_identity(recv, &args[0])))
    }
    // The root `initialize`: private, takes NO arguments, does nothing
    // (`rb_obj_dummy`, `object.c`; registered with arity 0 at
    // `object.c`'s BasicObject setup). It exists so that `super` from ANY
    // class's `initialize` has something to reach -- which is what makes the
    // ordinary `include SomeMixin` + `super` idiom work. Without it, a bare
    // `super` at the top of a chain raised "no superclass method".
    //
    // Arity 0 is load-bearing, not incidental: `def initialize(x); super; end`
    // forwards `x` and must raise ArgumentError exactly as CRuby does, so a
    // permissive signature here would silently accept programs CRuby rejects.
    // `super()` (explicit empty parens) is the way to reach it from a method
    // that takes parameters.
    "initialize" => fn initialize(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Nil)
    }
    // ONLY `__send__` belongs here. CRuby puts `send` and `public_send` on
    // KERNEL (`vm_eval.c:2961`, `:2963`), which a `BasicObject` subclass never
    // sees -- so `BO.new.send(:x)` must raise while `BO.new.__send__(:x)`
    // works. Ordinary objects still reach both through Kernel's own table,
    // which shares this implementation.
    "__send__" => fn bo_send(recv, args, block) {
        dynamic_send(recv, args, block)
    }
    // `instance_exec(*args) { |*a| ... }` -- run the block with `self`
    // rebound to the receiver, forwarding args to the block's params.
    // Arity is NOT checked against the block's params: a non-lambda block is
    // lenient (extra args dropped, missing ones nil), exactly as `yield` is.
    "instance_exec" => fn instance_exec(recv, args, block) {
        let blk = block_proc(block, "instance_exec")?;
        blk.call_with_self(recv, args)
    }
    // `instance_eval { ... }` -- the block form only. Real Ruby yields the
    // receiver to the block as well as rebinding self, which is what makes
    // `obj.instance_eval { |o| o == self }` true.
    "instance_eval" => fn instance_eval(recv, args, block) {
        if let Some(arg) = args.first() {
            // The string form (`obj.instance_eval("...")`) runs the source
            // through the eval VM (#97 stage 2) with `self` rebound to the
            // receiver, so `@ivar`/implicit-self calls resolve against `obj`.
            // A non-String argument keeps Ruby's own TypeError (handled by
            // `eval_value`'s coercion).
            return crate::eval_value(arg.clone(), recv.clone(), 0);
        }
        let blk = block_proc(block, "instance_eval")?;
        blk.call_with_self(recv, std::slice::from_ref(recv))
    }
}

/// The block argument `instance_exec`/`instance_eval` require, or real
/// Ruby's own no-block error.
pub(crate) fn block_proc(
    block: Option<RubyValue>,
    method: &str,
) -> Result<crate::RProc, crate::Signal> {
    match block {
        Some(RubyValue::Proc(p)) => Ok(p),
        _ => Err(crate::dispatch::raise_error(
            "ArgumentError",
            format!("tried to create Proc object without a block (in `{method}')"),
        )),
    }
}

/// Reference identity, CRuby's `equal?`: by-value for immediates (real Ruby
/// too -- `5.equal?(5)` is true, immediates have one identity per value),
/// allocation identity for everything heap-backed (`"a".equal?("a")` is
/// false).
pub(crate) fn value_identity(a: &RubyValue, b: &RubyValue) -> bool {
    match (a, b) {
        (RubyValue::Nil, RubyValue::Nil) => true,
        (RubyValue::Bool(x), RubyValue::Bool(y)) => x == y,
        (RubyValue::Int(x), RubyValue::Int(y)) => x == y,
        (RubyValue::Float(x), RubyValue::Float(y)) => x.to_bits() == y.to_bits(),
        (RubyValue::Symbol(x), RubyValue::Symbol(y)) => x == y,
        (RubyValue::Class(x), RubyValue::Class(y)) => x == y,
        (RubyValue::Str(x), RubyValue::Str(y)) => Arc::ptr_eq(x, y),
        (RubyValue::Array(x), RubyValue::Array(y)) => Arc::ptr_eq(x, y),
        (RubyValue::Hash(x), RubyValue::Hash(y)) => Arc::ptr_eq(x, y),
        (RubyValue::Proc(x), RubyValue::Proc(y)) => x.ptr_eq(y),
        (RubyValue::Enumerator(x), RubyValue::Enumerator(y)) => Arc::ptr_eq(x, y),
        (RubyValue::Yielder(x), RubyValue::Yielder(y)) => x.ptr_eq(y),
        (RubyValue::Regexp(x), RubyValue::Regexp(y)) => Arc::ptr_eq(x, y),
        (RubyValue::MatchData(x), RubyValue::MatchData(y)) => Arc::ptr_eq(x, y),
        (RubyValue::Object(x), RubyValue::Object(y)) => {
            // The same fat-pointer identity `container_identity` uses.
            std::ptr::addr_eq(Arc::as_ptr(x), Arc::as_ptr(y))
        }
        _ => false,
    }
}

/// The shared `send`/`__send__` body: coerce the first argument to a method
/// name and dispatch the rest to it.
///
/// Lives here because `__send__` is BasicObject's, but Kernel's `send` is the
/// same operation on a receiver that also has the Object surface -- CRuby
/// likewise gives both the one `rb_f_send` implementation (vm_eval.c:2960).
pub(crate) fn dynamic_send(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let Some((name_arg, rest)) = args.split_first() else {
        return Err(crate::dispatch::raise_error(
            "ArgumentError",
            "no method name given".to_string(),
        ));
    };
    let sym = match name_arg {
        RubyValue::Symbol(s) => *s,
        RubyValue::Str(s) => Symbol::intern(&s.lock().to_utf8_lossy()),
        other => {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                format!("{} is not a symbol nor a string", other.inspect_string()),
            ))
        }
    };
    crate::dispatch::send_value(recv, sym, rest, block)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_is_reference_identity_with_immediate_value_identity() {
        let five = RubyValue::Int(5);
        assert!(value_identity(&five, &RubyValue::Int(5)));
        assert!(!value_identity(&five, &RubyValue::Float(5.0)));

        let s1 = RubyValue::Str(crate::string_new("a".to_string()));
        let s2 = RubyValue::Str(crate::string_new("a".to_string()));
        assert!(value_identity(&s1, &s1.clone()));
        assert!(!value_identity(&s1, &s2));
    }

    #[test]
    fn bang_negates_truthiness() {
        assert!(matches!(
            not(&RubyValue::Int(5), &[], None).unwrap(),
            RubyValue::Bool(false)
        ));
        assert!(matches!(
            not(&RubyValue::Nil, &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
    }

    #[test]
    fn eq_row_rejects_wrong_arity_registryless_by_panicking() {
        // No exception factory in unit tests: raise_error panics loudly.
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| eq(&RubyValue::Int(1), &[], None)));
        assert!(r.is_err());
    }
}

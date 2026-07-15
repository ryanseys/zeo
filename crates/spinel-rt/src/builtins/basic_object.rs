//! `BasicObject` -- the true root's 8 methods (CRuby object.c). Owns
//! `==`/`!=` (funneling to `rb_eq`, whose recursion guard lives in
//! `value.rs`), `!`, `equal?` (reference identity), and `__send__`.
//! `instance_eval`/`instance_exec` are compile-time rejections (dynamic
//! self-rebinding is a permanent AOT exclusion); `__id__` is Tier B.

use crate::builtins::{arity, builtin_methods};
use crate::{RubyValue, Symbol};
use std::sync::Arc;

builtin_methods! {
    pub(crate) fn lookup;

    "==" => fn eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.rb_eq(&args[0])))
    }
    "!=" => fn neq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(!recv.rb_eq(&args[0])))
    }
    "!" => fn not(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(!recv.truthy()))
    }
    "equal?" => fn equal(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(value_identity(recv, &args[0])))
    }
    "send" | "__send__" | "public_send" => fn bo_send(recv, args, block) {
        let Some((name_arg, rest)) = args.split_first() else {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                "no method name given".to_string(),
            ));
        };
        let sym = match name_arg {
            RubyValue::Symbol(s) => *s,
            RubyValue::Str(s) => Symbol::intern(&s.lock()),
            other => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "{} is not a symbol nor a string",
                        other.inspect_string()
                    ),
                ))
            }
        };
        crate::dispatch::send_value(recv, sym, rest, block)
    }
}

/// Reference identity, CRuby's `equal?`: by-value for immediates (real Ruby
/// too -- `5.equal?(5)` is true, immediates have one identity per value),
/// allocation identity for everything heap-backed (`"a".equal?("a")` is
/// false). `send`/`public_send` are listed here rather than in `kernel.rs`
/// because `__send__` is BasicObject's and the three share one
/// implementation; the walk finds them regardless.
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

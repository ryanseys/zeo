//! `NilClass`/`TrueClass`/`FalseClass` (CRuby object.c's home for all
//! three) -- the boolean algebra rows and nil's conversion family.
//! `to_s`/`inspect` resolve through Kernel's rows (nil renders ""/"nil").

use crate::RubyValue;
use crate::builtins::{arity, builtin_methods};

builtin_methods! {
    pub(crate) fn lookup_nil;

    "&"[1] => fn nil_and(_recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(false))
    }
    "|"[1] => fn nil_or(_recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(args[0].truthy()))
    }
    "^"[1] => fn nil_xor(_recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(args[0].truthy()))
    }
    "to_a"[0] => fn nil_to_a(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Array(crate::array_new(Vec::new())))
    }
    "to_h"[0] => fn nil_to_h(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Hash(crate::hash_new(Vec::new())))
    }
    "to_i"[0] => fn nil_to_i(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(0))
    }
    "to_f"[0] => fn nil_to_f(_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Float(0.0))
    }
    // `nil` converts to the zero of each numeric tower.
    "to_r"[0] | "rationalize" => fn nil_to_r(_recv, args, _block) {
        arity!(args, 0..=1);
        crate::builtins::rational::rational_new(0.into(), 1.into())
    }
    "to_c"[0] => fn nil_to_c(_recv, args, _block) {
        arity!(args, 0);
        crate::builtins::complex::complex_new(RubyValue::Int(0), RubyValue::Int(0))
    }
    // `nil =~ anything` is always nil (nil matches no pattern).
    "=~"[1] => fn nil_match(_recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Nil)
    }
}

builtin_methods! {
    pub(crate) fn lookup_bool;

    "&"[1] => fn bool_and(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.truthy() && args[0].truthy()))
    }
    "|"[1] => fn bool_or(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.truthy() || args[0].truthy()))
    }
    "^"[1] => fn bool_xor(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.truthy() != args[0].truthy()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nil_algebra_and_conversions_match_the_oracle() {
        assert!(matches!(
            nil_and(&RubyValue::Nil, &[RubyValue::Bool(true)], None).unwrap(),
            RubyValue::Bool(false)
        ));
        assert!(matches!(
            nil_or(&RubyValue::Nil, &[RubyValue::Int(1)], None).unwrap(),
            RubyValue::Bool(true)
        ));
        assert_eq!(
            nil_to_a(&RubyValue::Nil, &[], None)
                .unwrap()
                .inspect_string(),
            "[]"
        );
        assert!(matches!(
            nil_to_i(&RubyValue::Nil, &[], None).unwrap(),
            RubyValue::Int(0)
        ));
    }

    #[test]
    fn bool_algebra_is_truthiness_logic() {
        assert!(matches!(
            bool_xor(&RubyValue::Bool(true), &[RubyValue::Bool(true)], None).unwrap(),
            RubyValue::Bool(false)
        ));
        assert!(matches!(
            bool_and(&RubyValue::Bool(true), &[RubyValue::Int(1)], None).unwrap(),
            RubyValue::Bool(true)
        ));
    }
}

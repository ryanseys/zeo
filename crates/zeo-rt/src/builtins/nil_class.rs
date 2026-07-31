//! `NilClass` -- nil's boolean algebra (`nil` is falsy) and its conversion
//! family (`nil` is the zero of every tower). CRuby's home for this is
//! object.c.

use crate::RubyValue;
use crate::builtins::arity;
use zeo_macros::ruby_class;

ruby_class! {
    NilClass = zeo_abi::NIL_CLASS < zeo_abi::OBJECT_CLASS;

    def "&" arity 1 (_recv, *args, &_block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(false))
    }
    def "|" arity 1 (_recv, *args, &_block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(args[0].truthy()))
    }
    def "^" arity 1 (_recv, *args, &_block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(args[0].truthy()))
    }
    def "to_a" arity 0 (_recv, *args, &_block) {
        arity!(args, 0);
        Ok(RubyValue::Array(crate::array_new(Vec::new())))
    }
    def "to_h" arity 0 (_recv, *args, &_block) {
        arity!(args, 0);
        Ok(RubyValue::Hash(crate::hash_new(Vec::new())))
    }
    def "to_i" arity 0 (_recv, *args, &_block) {
        arity!(args, 0);
        Ok(RubyValue::Int(0))
    }
    def "to_f" arity 0 (_recv, *args, &_block) {
        arity!(args, 0);
        Ok(RubyValue::Float(0.0))
    }
    // `nil` converts to the zero of each numeric tower.
    def "to_r" arity 0 | "rationalize" (_recv, *args, &_block) {
        arity!(args, 0..=1);
        crate::builtins::rational::rational_new(0.into(), 1.into())
    }
    def "to_c" arity 0 (_recv, *args, &_block) {
        arity!(args, 0);
        crate::builtins::complex::complex_new(RubyValue::Int(0), RubyValue::Int(0))
    }
    // Renderings Kernel's own rows already produce -- but declared HERE, where
    // CRuby declares them, because a caller can ask WHERE a method comes from:
    // pp's fallback reads `obj.method(:inspect).owner != Kernel` to decide
    // whether an object has its own rendering, and answered `Kernel` for nil.
    def "to_s" arity 0 (_recv, *args, &_block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_new(String::new())))
    }
    def "inspect" arity 0 (_recv, *args, &_block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_new("nil".to_string())))
    }
    // `nil =~ anything` is always nil (nil matches no pattern).
    def "=~" arity 1 (_recv, *args, &_block) {
        arity!(args, 1);
        Ok(RubyValue::Nil)
    }
}

#[cfg(test)]
mod tests {
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::NIL_CLASS)
            .expect("NilClass is a registered builtin table")
            .instance
            .as_ref()
            .expect("NilClass has instance methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("NilClass#{name} is defined"))
    }

    #[test]
    fn nil_algebra_and_conversions_match_the_oracle() {
        use crate::RubyValue;
        assert_eq!(
            imethod("&")(&RubyValue::Nil, &[RubyValue::Bool(true)], None)
                .unwrap()
                .inspect_string(),
            "false"
        );
        assert_eq!(
            imethod("|")(&RubyValue::Nil, &[RubyValue::Int(1)], None)
                .unwrap()
                .inspect_string(),
            "true"
        );
        assert_eq!(
            imethod("to_a")(&RubyValue::Nil, &[], None)
                .unwrap()
                .inspect_string(),
            "[]"
        );
        assert_eq!(
            imethod("to_i")(&RubyValue::Nil, &[], None)
                .unwrap()
                .inspect_string(),
            "0"
        );
    }
}

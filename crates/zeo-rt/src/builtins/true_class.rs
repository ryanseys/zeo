//! `TrueClass` -- the boolean algebra of `true`. Each row mirrors CRuby
//! object.c's dedicated `true_and`/`true_or`/`true_xor`: `true & obj` is truthy
//! iff `obj` is, `true | obj` is always true, `true ^ obj` is `obj`'s negation.

use crate::RubyValue;
use crate::builtins::arity;
use zeo_macros::ruby_class;

ruby_class! {
    TrueClass = zeo_abi::TRUE_CLASS < zeo_abi::OBJECT_CLASS;

    def "&" arity 1 (_recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(args[0].truthy()))
    }
    def "|" arity 1 (_recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(true))
    }
    def "^" arity 1 (_recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(!args[0].truthy()))
    }
    // Declared here rather than left to Kernel's row for the same reason
    // `NilClass#inspect` is: the OWNER is observable, and pp reads it.
    def "to_s" arity 0 | "inspect" (_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_new("true".to_string())))
    }
}

#[cfg(test)]
mod tests {
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::TRUE_CLASS)
            .expect("TrueClass is a registered builtin table")
            .instance
            .as_ref()
            .expect("TrueClass has instance methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("TrueClass#{name} is defined"))
    }

    #[test]
    fn true_algebra_is_truthiness_logic() {
        use crate::RubyValue;
        let t = RubyValue::Bool(true);
        assert_eq!(
            imethod("^")(&t, &[RubyValue::Bool(true)], None)
                .unwrap()
                .inspect_string(),
            "false"
        );
        assert_eq!(
            imethod("&")(&t, &[RubyValue::Int(1)], None)
                .unwrap()
                .inspect_string(),
            "true"
        );
        assert_eq!(
            imethod("|")(&t, &[RubyValue::Nil], None)
                .unwrap()
                .inspect_string(),
            "true"
        );
    }
}

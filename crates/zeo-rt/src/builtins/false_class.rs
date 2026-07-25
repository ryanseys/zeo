//! `FalseClass` -- the boolean algebra of `false`. Each row mirrors CRuby
//! object.c's dedicated `false_and`/`false_or`/`false_xor`: `false & obj` is
//! always false, `false | obj` and `false ^ obj` are both `obj`'s truthiness.

use crate::RubyValue;
use crate::builtins::arity;
use zeo_macros::ruby_class;

ruby_class! {
    FalseClass = zeo_abi::FALSE_CLASS < zeo_abi::OBJECT_CLASS;

    def "&"(_recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(false))
    }
    def "|"(_recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(args[0].truthy()))
    }
    def "^"(_recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(args[0].truthy()))
    }
}

#[cfg(test)]
mod tests {
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::FALSE_CLASS)
            .expect("FalseClass is a registered builtin table")
            .instance
            .as_ref()
            .expect("FalseClass has instance methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("FalseClass#{name} is defined"))
    }

    #[test]
    fn false_algebra_is_truthiness_logic() {
        use crate::RubyValue;
        let f = RubyValue::Bool(false);
        assert_eq!(imethod("&")(&f, &[RubyValue::Bool(true)], None).unwrap().inspect_string(), "false");
        assert_eq!(imethod("|")(&f, &[RubyValue::Int(1)], None).unwrap().inspect_string(), "true");
        assert_eq!(imethod("^")(&f, &[RubyValue::Nil], None).unwrap().inspect_string(), "false");
    }
}

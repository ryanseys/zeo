//! `Regexp` (CRuby re.c) -- stage B carries only `===` (pattern-match
//! case equality; Kernel's equality default would silently never match).
//! The dynamic-path breadth (`match`/`=~`/`source`/...) rides stage D with
//! String's, sharing `crate::regexp`'s helpers with the static paths.

use crate::builtins::{arity, builtin_methods};
use crate::RubyValue;

builtin_methods! {
    pub(crate) fn lookup;

    "===" => fn case_eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.rb_case_eq(&args[0])))
    }
    "encoding" => fn encoding_m(recv, args, _block) {
        arity!(args, 0);
        let RubyValue::Regexp(re) = recv else {
            unreachable!("the Regexp table only dispatches on Regexp receivers")
        };
        let id = crate::builtins::encoding::computed_encoding_of(&re.source);
        Ok(crate::builtins::encoding::encoding_value(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_eq_matches_a_string_subject() {
        let re = RubyValue::Regexp(crate::regexp_new("ab", false, false, false).expect("valid pattern"));
        let s = RubyValue::Str(crate::string_new("cabs".to_string()));
        assert!(matches!(case_eq(&re, &[s], None).unwrap(), RubyValue::Bool(true)));
        let miss = RubyValue::Str(crate::string_new("xyz".to_string()));
        assert!(matches!(case_eq(&re, &[miss], None).unwrap(), RubyValue::Bool(false)));
    }
}

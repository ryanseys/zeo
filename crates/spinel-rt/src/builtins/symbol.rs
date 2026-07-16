//! `Symbol` (CRuby symbol.c/string.c) -- the Tier A surface. The
//! case/succ rows delegate to `string.rs`'s shared helpers and re-intern;
//! `to_proc` builds the `&:name` block (one dynamic dispatch per call).

use crate::builtins::{arity, builtin_methods};
use crate::{RProc, RubyValue, Symbol};

fn recv_sym(recv: &RubyValue) -> Symbol {
    match recv {
        RubyValue::Symbol(s) => *s,
        _ => unreachable!("Symbol table row dispatched on a non-Symbol receiver"),
    }
}

/// `Symbol#to_proc`'s conversion -- also the `&:name` block-argument path
/// (`emit_block_option` routes every `&expr` through
/// `block_arg_to_proc`).
pub(crate) fn symbol_to_proc(name: Symbol) -> RubyValue {
    let p: RProc = RProc::new(move |args: &[RubyValue]| {
        let Some((recv, rest)) = args.split_first() else {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                "no receiver given".to_string(),
            ));
        };
        crate::dispatch::send_value(recv, name, rest, None)
    });
    RubyValue::Proc(p)
}

builtin_methods! {
    pub(crate) fn lookup;

    "to_s" | "id2name" | "name" => fn to_s(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_new(recv_sym(recv).name())))
    }
    "to_sym" | "intern" => fn to_sym(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    "encoding" => fn encoding_m(recv, args, _block) {
        arity!(args, 0);
        let id = crate::builtins::encoding::computed_encoding_of(&recv_sym(recv).name());
        Ok(crate::builtins::encoding::encoding_value(id))
    }
    "inspect" => fn inspect(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_new(format!(":{}", recv_sym(recv).name()))))
    }
    "length" | "size" => fn length(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_sym(recv).name().chars().count() as i64))
    }
    "empty?" => fn empty_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv_sym(recv).name().is_empty()))
    }
    "<=>" => fn spaceship(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Symbol(other) = &args[0] else {
            return Ok(RubyValue::Nil);
        };
        Ok(RubyValue::Int(
            recv_sym(recv).name().cmp(&other.name()) as i64
        ))
    }
    "==" => fn eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.rb_eq(&args[0])))
    }
    "upcase" => fn upcase(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Symbol(Symbol::intern(
            &recv_sym(recv).name().to_uppercase(),
        )))
    }
    "downcase" => fn downcase(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Symbol(Symbol::intern(
            &recv_sym(recv).name().to_lowercase(),
        )))
    }
    "capitalize" => fn capitalize(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Symbol(Symbol::intern(
            &crate::builtins::string::capitalize_str(&recv_sym(recv).name()),
        )))
    }
    "swapcase" => fn swapcase(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Symbol(Symbol::intern(
            &crate::builtins::string::swapcase_str(&recv_sym(recv).name()),
        )))
    }
    "succ" | "next" => fn succ(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Symbol(Symbol::intern(
            &crate::builtins::string::succ_str(&recv_sym(recv).name()),
        )))
    }
    "to_proc" => fn to_proc(recv, args, _block) {
        arity!(args, 0);
        Ok(symbol_to_proc(recv_sym(recv)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(s: &str) -> RubyValue {
        RubyValue::Symbol(Symbol::intern(s))
    }

    #[test]
    fn reflection_rows_match_the_oracle() {
        let r = inspect(&sym("he"), &[], None).unwrap();
        assert_eq!(r.to_display_string(), ":he");
        let r = length(&sym("hello"), &[], None).unwrap();
        assert!(matches!(r, RubyValue::Int(5)));
        let r = spaceship(&sym("b"), &[sym("a")], None).unwrap();
        assert!(matches!(r, RubyValue::Int(1)));
        let r = upcase(&sym("he"), &[], None).unwrap();
        assert_eq!(r.inspect_string(), ":HE");
        let r = succ(&sym("a"), &[], None).unwrap();
        assert_eq!(r.inspect_string(), ":b");
    }

    #[test]
    fn to_proc_dispatches_the_named_method() {
        let p = symbol_to_proc(Symbol::intern("length"));
        let RubyValue::Proc(p) = p else { panic!() };
        let s = RubyValue::Str(crate::string_new("abc".to_string()));
        let r = p.call(&[s]).unwrap();
        assert!(matches!(r, RubyValue::Int(3)));
    }
}

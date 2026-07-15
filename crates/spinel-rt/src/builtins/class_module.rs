//! `Class` + `Module` receiver methods (CRuby class.c/object.c), found via
//! the walk on a `RubyValue::Class` receiver (whose `class_id()` is
//! `CLASS_CLASS` or `MODULE_CLASS`; `Class`'s chain passes through
//! `Module`, so class values see both tables -- real Ruby's own layout:
//! `Module` owns `name`/`ancestors`/`===`, `Class` owns `new`).
//! `struct`/`class`/`module` being Rust keywords is why the two share this
//! one file.

use crate::builtins::{arity, builtin_methods};
use crate::RubyValue;

fn recv_cid(recv: &RubyValue) -> crate::ClassId {
    match recv {
        RubyValue::Class(cid) => *cid,
        _ => unreachable!("Class/Module table row dispatched on a non-Class receiver"),
    }
}

builtin_methods! {
    pub(crate) fn lookup_module;

    "name" | "to_s" | "inspect" => fn name(recv, args, _block) {
        arity!(args, 0);
        let cid = recv_cid(recv);
        let n = crate::dispatch::class_name(cid).unwrap_or_else(|| format!("#<Class:{}>", cid.0));
        Ok(RubyValue::Str(crate::string_new(n)))
    }
    // `Class#superclass` -- the first non-module entry after self in the
    // linearized ancestors (prepends/includes are modules, so this lands on
    // the real parent class); `nil` at the root (`BasicObject`).
    "superclass" => fn superclass(recv, args, _block) {
        arity!(args, 0);
        let cid = recv_cid(recv);
        let ancestors = crate::dispatch::ancestors_of_value(cid);
        for &anc in ancestors.iter().skip_while(|&&a| a != cid).skip(1) {
            if !crate::dispatch::class_is_module(anc).unwrap_or(false) {
                return Ok(RubyValue::Class(anc));
            }
        }
        Ok(RubyValue::Nil)
    }
    "ancestors" => fn ancestors(recv, args, _block) {
        arity!(args, 0);
        let chain = crate::dispatch::ancestors_of_value(recv_cid(recv))
            .iter()
            .map(|&a| RubyValue::Class(a))
            .collect();
        Ok(RubyValue::Array(crate::array_new(chain)))
    }
    // `Module#===`: instance-of-ancestry, the check `case`/`when` class
    // candidates desugar to.
    "===" => fn case_eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(crate::dispatch::is_a(
            args[0].class_id(),
            recv_cid(recv),
        )))
    }
}

builtin_methods! {
    pub(crate) fn lookup_class;

    // `Class#new` -- the registry's dynamic constructor (`x = Widget;
    // x.new(...)`). A class with no allocator (builtins, exception-less
    // edge cases) raises real Ruby's NoMethodError shape for its kind.
    "new" => fn new_m(recv, args, block) {
        let cid = recv_cid(recv);
        // `Enumerator.new([size]) { |y| ... }` is the ONE builtin with a
        // runtime allocator (Phase 17.2); parse deliberately skips the
        // static `New` node for it so the block arrives here.
        if cid == spinel_abi::ENUMERATOR_CLASS {
            return crate::builtins::enumerator::enumerator_new(args, block);
        }
        match crate::dispatch::constructor_of(cid) {
            Some(ctor) => ctor(args, block),
            None => {
                let is_module = crate::dispatch::class_is_module(cid).unwrap_or(false);
                let kind = if is_module { "module" } else { "class" };
                let n = crate::dispatch::class_name(cid)
                    .unwrap_or_else(|| format!("#<Class:{}>", cid.0));
                Err(crate::dispatch::raise_error(
                    "NoMethodError",
                    format!("undefined method 'new' for {kind} {n}"),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spinel_abi::*;

    #[test]
    fn module_case_eq_checks_ancestry_registry_free() {
        // 5.class == Integer; Integer's fallback chain contains Numeric.
        let r = case_eq(
            &RubyValue::Class(NUMERIC_CLASS),
            &[RubyValue::Int(5)],
            None,
        )
        .unwrap();
        assert!(matches!(r, RubyValue::Bool(true)));
        let r = case_eq(
            &RubyValue::Class(STRING_CLASS),
            &[RubyValue::Int(5)],
            None,
        )
        .unwrap();
        assert!(matches!(r, RubyValue::Bool(false)));
    }

    #[test]
    fn ancestors_row_reflects_the_fallback_chain() {
        let RubyValue::Array(a) = ancestors(&RubyValue::Class(INTEGER_CLASS), &[], None).unwrap()
        else {
            panic!()
        };
        assert_eq!(a.lock().len(), 6); // [Integer, Numeric, Comparable, Object, Kernel, BasicObject]
    }
}

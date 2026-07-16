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
    // Reflection over a CLASS OBJECT's own ivars -- the `@x` a `def self.x`
    // or a class body writes (see `civars`' docs). Really `Object`'s
    // methods, which a class inherits; they live on the Module table
    // because that is the one a `RubyValue::Class` receiver reaches.
    "instance_variable_get" => fn ivar_get(recv, args, _block) {
        arity!(args, 1);
        let name = ivar_name_arg(&args[0])?;
        Ok(crate::civars::class_ivar_get(recv_cid(recv).0, &name))
    }
    "instance_variable_set" => fn ivar_set(recv, args, _block) {
        arity!(args, 2);
        let name = ivar_name_arg(&args[0])?;
        crate::civars::class_ivar_set(recv_cid(recv).0, &name, args[1].clone());
        // Answers the VALUE, not the receiver -- oracle-checked.
        Ok(args[1].clone())
    }
    "instance_variable_defined?" => fn ivar_defined(recv, args, _block) {
        arity!(args, 1);
        let name = ivar_name_arg(&args[0])?;
        Ok(RubyValue::Bool(
            crate::civars::class_ivar_names(recv_cid(recv).0).contains(&name),
        ))
    }
    "instance_variables" => fn ivars(recv, args, _block) {
        arity!(args, 0);
        let names = crate::civars::class_ivar_names(recv_cid(recv).0)
            .into_iter()
            .map(|n| RubyValue::Symbol(crate::Symbol::intern(&format!("@{n}"))))
            .collect();
        Ok(RubyValue::Array(crate::array_new(names)))
    }
}

/// The `:@x`/`"@x"` argument of the `instance_variable_*` family, as the
/// BARE name (`x`) the `civars` table is keyed on -- matching what codegen
/// keys a static class-ivar access on, which is `safe_ident`'s output over
/// an already-`@`-less HIR name.
///
/// Both a Symbol and a String are accepted (real Ruby takes either), and a
/// name without the leading `@` is a NameError rather than a silent miss --
/// oracle-verified, message shape included:
/// `K.instance_variable_get(:a)` => `'a' is not allowed as an instance
/// variable name`.
fn ivar_name_arg(v: &RubyValue) -> Result<String, crate::Signal> {
    let raw = match v {
        RubyValue::Symbol(s) => s.name().to_string(),
        RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
        _ => {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                format!("{} is not a symbol nor a string", v.inspect_string()),
            ))
        }
    };
    match raw.strip_prefix('@') {
        Some(name) => Ok(name.to_string()),
        None => Err(crate::dispatch::raise_error(
            "NameError",
            format!("'{raw}' is not allowed as an instance variable name"),
        )),
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

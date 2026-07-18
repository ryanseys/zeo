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

/// The optional `inherit` boolean of `instance_methods`/`methods` (default
/// true) -- only an explicit `false`/`nil` narrows to own methods.
fn inherit_flag(args: &[RubyValue]) -> bool {
    !matches!(args.first(), Some(RubyValue::Bool(false)) | Some(RubyValue::Nil))
}

/// A `Vec<Symbol>` as a Ruby Array of Symbols -- reflection's return shape.
fn syms_to_array(names: Vec<crate::Symbol>) -> RubyValue {
    RubyValue::Array(crate::array_new(names.into_iter().map(RubyValue::Symbol).collect()))
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
    // `Module#include?(mod)`: true when `mod` is a MODULE mixed into `recv` or
    // one of its ancestors (never `recv` itself, and never a superclass --
    // only included/prepended modules count). A non-class/module argument is a
    // TypeError.
    "include?" => fn include_p(recv, args, _block) {
        arity!(args, 1);
        // The argument must be a MODULE. A class (or any non-module) is a
        // TypeError whose type name CRuby reports as `Class` for a class value.
        let type_err = || {
            let name = match &args[0] {
                RubyValue::Class(cid) if !crate::dispatch::class_is_module(*cid).unwrap_or(false) => {
                    "Class".to_string()
                }
                other => crate::builtins::class_name_of(other),
            };
            crate::dispatch::raise_error(
                "TypeError",
                format!("wrong argument type {name} (expected Module)"),
            )
        };
        let RubyValue::Class(other) = args[0] else {
            return Err(type_err());
        };
        if !crate::dispatch::class_is_module(other).unwrap_or(false) {
            return Err(type_err());
        }
        let me = recv_cid(recv);
        let in_chain =
            other != me && crate::dispatch::ancestors_of_value(me).contains(&other);
        Ok(RubyValue::Bool(in_chain))
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
    // `Module`'s ancestry ordering (`rb_class_cmp`): `<`/`<=`/`>`/`>=` answer
    // the subclass relation and `nil` when the two are UNRELATED (neither is
    // an ancestor of the other), while a non-class/module argument is a
    // TypeError. `<=>` is `nil` for both the unrelated and the non-module
    // cases. All five share the one `module_cmp` ordering.
    "<" => fn mod_lt(recv, args, _block) {
        arity!(args, 1);
        module_ordering_op(recv, &args[0], |o| matches!(o, std::cmp::Ordering::Less))
    }
    "<=" => fn mod_le(recv, args, _block) {
        arity!(args, 1);
        module_ordering_op(recv, &args[0], |o| matches!(o, std::cmp::Ordering::Less | std::cmp::Ordering::Equal))
    }
    ">" => fn mod_gt(recv, args, _block) {
        arity!(args, 1);
        module_ordering_op(recv, &args[0], |o| matches!(o, std::cmp::Ordering::Greater))
    }
    ">=" => fn mod_ge(recv, args, _block) {
        arity!(args, 1);
        module_ordering_op(recv, &args[0], |o| matches!(o, std::cmp::Ordering::Greater | std::cmp::Ordering::Equal))
    }
    "<=>" => fn mod_cmp(recv, args, _block) {
        arity!(args, 1);
        match args[0] {
            RubyValue::Class(other) => Ok(
                crate::dispatch::module_cmp(recv_cid(recv), other)
                    .map_or(RubyValue::Nil, |o| RubyValue::Int(o as i64)),
            ),
            _ => Ok(RubyValue::Nil),
        }
    }
    // `Class#subclasses`: the DIRECT, currently-registered subclasses. Order
    // is unspecified in CRuby (a hash-set walk), so this returns them in the
    // registry's iteration order -- tests that assert a listing sort it.
    "subclasses" => fn subclasses(recv, args, _block) {
        arity!(args, 0);
        let kids = crate::dispatch::direct_subclasses(recv_cid(recv))
            .into_iter()
            .map(RubyValue::Class)
            .collect();
        Ok(RubyValue::Array(crate::array_new(kids)))
    }
    // Named classes/modules are never singleton (metaclass) classes; spinel
    // doesn't model per-object singleton classes as first-class ids, so this
    // is `false` for every reachable `RubyValue::Class` receiver.
    "singleton_class?" => fn singleton_class_p(recv, args, _block) {
        arity!(args, 0);
        let _ = recv_cid(recv);
        Ok(RubyValue::Bool(false))
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
    // `Module#method_defined?(:name)` -- does the class (or an ancestor)
    // provide `name` as a public/protected INSTANCE method? Reuses the same
    // MRO walk `respond_to?` does (`responds_to` with `include_all=false`,
    // which skips private but keeps protected -- exactly method_defined?'s
    // rule), so an inherited `object_id`/`frozen?` answers true too.
    "method_defined?" => fn method_defined(recv, args, _block) {
        arity!(args, 1);
        let name = name_arg(&args[0])?;
        Ok(RubyValue::Bool(crate::dispatch::responds_to(
            recv_cid(recv),
            crate::Symbol::intern(&name),
            false,
        )))
    }
    // `instance_methods(inherit=true)` -- public+protected names of the
    // module/class (and its ancestors unless `inherit` is false). A builtin's
    // list is a subset of CRuby's (this runtime implements a subset), so
    // callers assert membership; a user class's own list is exact.
    "instance_methods" | "public_instance_methods" => fn instance_methods(recv, args, _block) {
        arity!(args, 0..=1);
        let names = crate::dispatch::instance_method_names(
            recv_cid(recv),
            crate::dispatch::MethodVisibility::Public,
            inherit_flag(args),
        );
        Ok(syms_to_array(names))
    }
    "private_instance_methods" => fn private_instance_methods(recv, args, _block) {
        arity!(args, 0..=1);
        let names = crate::dispatch::instance_method_names(
            recv_cid(recv),
            crate::dispatch::MethodVisibility::Private,
            inherit_flag(args),
        );
        Ok(syms_to_array(names))
    }
    // This runtime tracks no separate `protected` visibility, so the protected
    // set is always empty (documented divergence; protected methods surface as
    // public in `instance_methods`).
    "protected_instance_methods" => fn protected_instance_methods(_recv, args, _block) {
        arity!(args, 0..=1);
        Ok(syms_to_array(Vec::new()))
    }
    // `Module#instance_method(:name)` -> an UnboundMethod for the module/class.
    "instance_method" => fn instance_method(recv, args, _block) {
        arity!(args, 1);
        crate::builtins::method_obj::unbound_method_new(recv_cid(recv), &args[0])
    }
    // `Module#define_method(name) { body }` (#97) -- install/override an
    // instance method AT RUNTIME (a computed name, or inside an `each` loop).
    // The literal `define_method(:sym) { ... }` form is desugared to a `def` at
    // compile time in spinelc; this row serves everything that isn't literal.
    "define_method" => fn define_method(recv, args, block) {
        arity!(args, 1..=2);
        let name = crate::runtime_meta::coerce_method_name(args.first())?;
        let body = crate::runtime_meta::coerce_method_body(args, &block)?;
        Ok(crate::runtime_define_method(recv_cid(recv), name, body))
    }
    // `Module#class_variable_get/set/defined?` over the linearized ancestry
    // (a `@@x` is owned by the nearest ancestor that first assigned it --
    // see `cvars`' docs). `get` on a never-assigned name is a `NameError`,
    // unlike a plain `@@x` read's nil-on-miss.
    "class_variable_get" => fn cvar_get_m(recv, args, _block) {
        arity!(args, 1);
        let name = cvar_name_arg(&args[0])?;
        let cid = recv_cid(recv);
        for &anc in crate::dispatch::ancestors_of_value(cid) {
            if crate::cvar_defined(anc.0, &name) {
                return Ok(crate::cvar_get(anc.0, &name));
            }
        }
        Err(crate::dispatch::raise_error(
            "NameError",
            format!(
                "uninitialized class variable @@{name} in {}",
                crate::dispatch::class_name(cid).unwrap_or_default()
            ),
        ))
    }
    "class_variable_set" => fn cvar_set_m(recv, args, _block) {
        arity!(args, 2);
        let name = cvar_name_arg(&args[0])?;
        let cid = recv_cid(recv);
        // Assign on the owning ancestor if one already exists, else on the
        // receiver itself (real Ruby's own rule).
        let owner = crate::dispatch::ancestors_of_value(cid)
            .iter()
            .find(|&&anc| crate::cvar_defined(anc.0, &name))
            .map_or(cid, |&anc| anc);
        crate::cvar_set(owner.0, &name, args[1].clone());
        Ok(args[1].clone())
    }
    "class_variable_defined?" => fn cvar_defined_m(recv, args, _block) {
        arity!(args, 1);
        let name = cvar_name_arg(&args[0])?;
        Ok(RubyValue::Bool(
            crate::dispatch::ancestors_of_value(recv_cid(recv))
                .iter()
                .any(|&anc| crate::cvar_defined(anc.0, &name)),
        ))
    }
}

/// Shared body of `Module`'s `<`/`<=`/`>`/`>=`: a non-class/module argument
/// is a TypeError (`compared with non class/module`), an unrelated class is
/// `nil`, and a related one runs `pred` over the `module_cmp` ordering.
fn module_ordering_op(
    recv: &RubyValue,
    arg: &RubyValue,
    pred: impl Fn(std::cmp::Ordering) -> bool,
) -> Result<RubyValue, crate::Signal> {
    let RubyValue::Class(other) = arg else {
        return Err(crate::dispatch::raise_error(
            "TypeError",
            "compared with non class/module".to_string(),
        ));
    };
    Ok(match crate::dispatch::module_cmp(recv_cid(recv), *other) {
        Some(o) => RubyValue::Bool(pred(o)),
        None => RubyValue::Nil,
    })
}

/// A `:name`/`"name"` method-name argument as a bare `String`. Accepts a
/// Symbol or String (real Ruby takes either); anything else is a TypeError.
fn name_arg(v: &RubyValue) -> Result<String, crate::Signal> {
    match v {
        RubyValue::Symbol(s) => Ok(s.name().to_string()),
        RubyValue::Str(s) => Ok(s.lock().to_utf8_lossy().into_owned()),
        _ => Err(crate::dispatch::raise_error(
            "TypeError",
            format!("{} is not a symbol nor a string", v.inspect_string()),
        )),
    }
}

/// The `:@@x`/`"@@x"` argument of the `class_variable_*` family, as the bare
/// name (`x`) the `cvars` table is keyed on. A name without the leading
/// `@@` is a NameError, matching real Ruby's shape.
fn cvar_name_arg(v: &RubyValue) -> Result<String, crate::Signal> {
    let raw = name_arg(v)?;
    match raw.strip_prefix("@@") {
        Some(name) => Ok(name.to_string()),
        None => Err(crate::dispatch::raise_error(
            "NameError",
            format!("'{raw}' is not allowed as a class variable name"),
        )),
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
        // `Class.new(superclass) { body }` (#97 F4) -- `recv` is `Class`
        // itself, so its `.new` mints a fresh ANONYMOUS class rather than an
        // instance. The block is the class body, run with `self` bound to the
        // new class (so `define_method`/`include`/const-assign inside populate
        // it). A literal `def` inside the block is a documented fast-follow
        // (use `define_method`).
        if cid == spinel_abi::CLASS_CLASS {
            let superclass = args.first().cloned();
            let body = match &block {
                Some(RubyValue::Proc(p)) => Some(p.clone()),
                _ => None,
            };
            return crate::runtime_class_new(superclass, body);
        }
        // `Enumerator.new([size]) { |y| ... }` is the ONE builtin with a
        // runtime allocator (Phase 17.2); parse deliberately skips the
        // static `New` node for it so the block arrives here.
        if cid == spinel_abi::ENUMERATOR_CLASS {
            return crate::builtins::enumerator::enumerator_new(args, block);
        }
        match crate::dispatch::constructor_of(cid) {
            Some(ctor) => ctor(cid, args, block),
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

//! `Class` + `Module` receiver methods (CRuby class.c/object.c), found via
//! the walk on a `RubyValue::Class` receiver (whose `class_id()` is
//! `CLASS_CLASS` or `MODULE_CLASS`; `Class`'s chain passes through
//! `Module`, so class values see both tables -- real Ruby's own layout:
//! `Module` owns `name`/`ancestors`/`===`, `Class` owns `new`).
//! `struct`/`class`/`module` being Rust keywords is why the two share this
//! one file.

use crate::RubyValue;
use crate::builtins::{arity, builtin_methods, name_error, type_error};

fn recv_cid(recv: &RubyValue) -> crate::ClassId {
    match recv {
        RubyValue::Class(cid) => *cid,
        _ => unreachable!("Class/Module table row dispatched on a non-Class receiver"),
    }
}

/// The shared body of `private_constant`/`public_constant` (see their table
/// rows): validate every name against the receiver's OWN constants -- a value
/// constant in the runtime map, or a nested class/module registered under the
/// receiver's namespace -- and answer the module. The visibility flag itself
/// is a documented no-op.
fn constant_visibility_no_op(
    recv: &RubyValue,
    args: &[RubyValue],
) -> Result<RubyValue, crate::Signal> {
    let cid = recv_cid(recv);
    let owner = crate::dispatch::class_name(cid).unwrap_or_else(|| "Object".to_string());
    for arg in args {
        let name = match arg {
            RubyValue::Symbol(s) => s.name().to_string(),
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => {
                return Err(type_error!(
                    "{} is not a symbol nor a string",
                    other.inspect_string()
                ));
            }
        };
        let defined = crate::constants::const_get(cid.0, &name).is_some()
            || crate::dispatch::class_id_by_name(&format!("{owner}::{name}")).is_some();
        if !defined {
            return Err(name_error!("constant {owner}::{name} not defined"));
        }
    }
    Ok(recv.clone())
}

/// Whether `name_arg` names an instance method of `recv` with exactly `want`
/// visibility -- shared by `public/private/protected_method_defined?`.
fn method_defined_with_vis(
    recv: &RubyValue,
    arg: &RubyValue,
    want: crate::dispatch::MethodVisibility,
) -> Result<bool, crate::Signal> {
    let name = name_arg(arg)?;
    let vis =
        crate::dispatch::instance_method_visibility(recv_cid(recv), crate::Symbol::intern(&name));
    Ok(vis == Some(want))
}

/// The optional `inherit` boolean of `instance_methods`/`methods` (default
/// true) -- only an explicit `false`/`nil` narrows to own methods.
fn inherit_flag(args: &[RubyValue]) -> bool {
    !matches!(
        args.first(),
        Some(RubyValue::Bool(false)) | Some(RubyValue::Nil)
    )
}

/// A `Vec<Symbol>` as a Ruby Array of Symbols -- reflection's return shape.
fn syms_to_array(names: Vec<crate::Symbol>) -> RubyValue {
    RubyValue::Array(crate::array_new(
        names.into_iter().map(RubyValue::Symbol).collect(),
    ))
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
    // `Module#included_modules`: the modules in `recv`'s ancestor chain, in MRO
    // order (the classes filtered out). Kernel and any mixed-in module appear;
    // `Object`/`BasicObject` (classes) do not.
    "included_modules" => fn included_modules(recv, args, _block) {
        arity!(args, 0);
        let mods = crate::dispatch::ancestors_of_value(recv_cid(recv))
            .iter()
            .filter(|&&a| crate::dispatch::class_is_module(a).unwrap_or(false))
            .map(|&a| RubyValue::Class(a))
            .collect();
        Ok(RubyValue::Array(crate::array_new(mods)))
    }
    // `Module#constants([inherit=true])`: this module's own constant names as
    // Symbols, then -- unless `inherit` is false -- its ancestors' (except
    // `Object`'s, CRuby's rule), own group first. Order within one class is
    // unspecified (an id table in CRuby, a HashMap here).
    // `Module#const_set(name, value)` -- define a constant on this module,
    // answering the value (as CRuby does).
    "const_set" => fn const_set_m(recv, args, _block) {
        arity!(args, 2);
        let cid = recv_cid(recv);
        let name = match &args[0] {
            RubyValue::Symbol(s) => s.name(),
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => {
                return Err(type_error!("{} is not a symbol nor a string", other.inspect_string()))
            }
        };
        crate::constants::const_set(cid.0, &name, args[1].clone());
        Ok(args[1].clone())
    }
    // `Module#private_constant(:A, ...)` / `Module#public_constant(:A, ...)` --
    // argument-validated like CRuby (each name must be an OWN constant of the
    // receiver, else "constant M::A not defined"), answering the module.
    // DIVERGENCE: the visibility itself is not enforced -- a privatized
    // constant stays reachable (compile-time constant resolution binds
    // references statically, so a runtime-only flag could not be honored
    // consistently anyway). Gems call this to hide internals (timeout's
    // `private_constant :GET_TIME`); accepting-without-enforcing loads them.
    "private_constant" => fn private_constant(recv, args, _block) {
        constant_visibility_no_op(recv, args)
    }
    "public_constant" => fn public_constant(recv, args, _block) {
        constant_visibility_no_op(recv, args)
    }
    // `Module#const_get(name)` -- resolve a constant on this module (walking the
    // ancestry), a NameError "uninitialized constant <name>" on a miss.
    "const_get" => fn const_get_m(recv, args, _block) {
        arity!(args, 1..=2);
        let cid = recv_cid(recv);
        let name = match &args[0] {
            RubyValue::Symbol(s) => s.name(),
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => {
                return Err(type_error!("{} is not a symbol nor a string", other.inspect_string()))
            }
        };
        crate::constants::const_get(cid.0, &name)
            .or_else(|| crate::dispatch::ancestors_of_value(cid)
                .iter()
                .find_map(|anc| crate::constants::const_get(anc.0, &name)))
            .ok_or_else(|| name_error!("uninitialized constant {name}"))
    }
    "constants" => fn constants(recv, args, _block) {
        arity!(args, 0..=1);
        let cid = recv_cid(recv);
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        let mut push_owner = |owner: crate::ClassId, out: &mut Vec<RubyValue>| {
            for name in crate::constants::const_names_of(owner.0) {
                if seen.insert(name.clone()) {
                    out.push(RubyValue::Symbol(crate::Symbol::intern(&name)));
                }
            }
        };
        push_owner(cid, &mut out);
        if inherit_flag(args) {
            for anc in crate::dispatch::ancestors_of_value(cid) {
                // `Object`'s constants (every top-level constant) are excluded
                // from a non-Object module's `constants`, matching CRuby.
                if *anc == cid || *anc == zeo_abi::OBJECT_CLASS
                    || *anc == zeo_abi::BASIC_OBJECT_CLASS
                {
                    continue;
                }
                push_owner(*anc, &mut out);
            }
        }
        Ok(RubyValue::Array(crate::array_new(out)))
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
            type_error!("wrong argument type {name} (expected Module)")
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
    // Named classes/modules are never singleton (metaclass) classes; zeo
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
        crate::civars::class_ivar_set(recv_cid(recv).0, &name, args[1].clone())?;
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
    // provide `name` as a public OR protected INSTANCE method? (private and
    // nonexistent answer false), so an inherited `object_id`/`frozen?` answers
    // true too.
    "method_defined?" => fn method_defined(recv, args, _block) {
        // The optional second `inherit` flag (default true): false restricts the
        // lookup to the receiver's own methods (no ancestor walk).
        arity!(args, 1..=2);
        let name = name_arg(&args[0])?;
        let inherit = args.get(1).is_none_or(RubyValue::truthy);
        Ok(RubyValue::Bool(crate::dispatch::method_defined_inherit(
            recv_cid(recv),
            crate::Symbol::intern(&name),
            inherit,
        )))
    }
    // `instance_methods(inherit=true)` -- public+protected names of the
    // module/class (and its ancestors unless `inherit` is false). A builtin's
    // list is a subset of CRuby's (this runtime implements a subset), so
    // callers assert membership; a user class's own list is exact.
    "instance_methods" => fn instance_methods(recv, args, _block) {
        arity!(args, 0..=1);
        let names = crate::dispatch::instance_method_names(
            recv_cid(recv),
            crate::dispatch::VisFilter::NotPrivate,
            inherit_flag(args),
        );
        Ok(syms_to_array(names))
    }
    // `public_instance_methods` narrows to public ONLY (protected excluded).
    "public_instance_methods" => fn public_instance_methods(recv, args, _block) {
        arity!(args, 0..=1);
        let names = crate::dispatch::instance_method_names(
            recv_cid(recv),
            crate::dispatch::VisFilter::Public,
            inherit_flag(args),
        );
        Ok(syms_to_array(names))
    }
    "private_instance_methods" => fn private_instance_methods(recv, args, _block) {
        arity!(args, 0..=1);
        let names = crate::dispatch::instance_method_names(
            recv_cid(recv),
            crate::dispatch::VisFilter::Private,
            inherit_flag(args),
        );
        Ok(syms_to_array(names))
    }
    "protected_instance_methods" => fn protected_instance_methods(recv, args, _block) {
        arity!(args, 0..=1);
        let names = crate::dispatch::instance_method_names(
            recv_cid(recv),
            crate::dispatch::VisFilter::Protected,
            inherit_flag(args),
        );
        Ok(syms_to_array(names))
    }
    // `private_method_defined?`/`public_method_defined?`/
    // `protected_method_defined?` -- true when `name` is an instance method of
    // this exact visibility. A non-method (or a name of another visibility)
    // answers false.
    "public_method_defined?" => fn public_method_defined(recv, args, _block) {
        arity!(args, 1..=2);
        Ok(RubyValue::Bool(method_defined_with_vis(
            recv, &args[0], crate::dispatch::MethodVisibility::Public)?))
    }
    "private_method_defined?" => fn private_method_defined(recv, args, _block) {
        arity!(args, 1..=2);
        Ok(RubyValue::Bool(method_defined_with_vis(
            recv, &args[0], crate::dispatch::MethodVisibility::Private)?))
    }
    "protected_method_defined?" => fn protected_method_defined(recv, args, _block) {
        arity!(args, 1..=2);
        Ok(RubyValue::Bool(method_defined_with_vis(
            recv, &args[0], crate::dispatch::MethodVisibility::Protected)?))
    }
    // `Module#instance_method(:name)` -> an UnboundMethod for the module/class.
    "instance_method" => fn instance_method(recv, args, _block) {
        arity!(args, 1);
        crate::builtins::method_obj::unbound_method_new(recv_cid(recv), &args[0])
    }
    // `Module#define_method(name) { body }` (#97) -- install/override an
    // instance method AT RUNTIME (a computed name, or inside an `each` loop).
    // The literal `define_method(:sym) { ... }` form is desugared to a `def` at
    // compile time in zeo; this row serves everything that isn't literal.
    "define_method" => fn define_method(recv, args, block) {
        arity!(args, 1..=2);
        let name = crate::runtime_meta::coerce_method_name(args.first())?;
        let body = crate::runtime_meta::coerce_method_body(args, &block)?;
        crate::runtime_define_method(recv_cid(recv), name, body)
    }
    // `Module#alias_method(new_name, old_name)` -- the RUNTIME form (a
    // computed name, or inside an `each` loop: ostruct's bulk `!`-alias
    // loop). The literal class-body form resolves at compile time; this row
    // serves everything that isn't literal. Snapshot semantics -- the alias
    // keeps the method `old_name` resolves to NOW -- and returns the new
    // name's Symbol, both per CRuby.
    "alias_method" => fn alias_method(recv, args, _block) {
        arity!(args, 2);
        let new = crate::runtime_meta::coerce_method_name(args.first())?;
        let old = crate::runtime_meta::coerce_method_name(args.get(1))?;
        crate::runtime_meta::runtime_alias_method(recv_cid(recv), new, old)
    }
    // `Module#private`/`public`/`protected` reached at RUNTIME (inside a
    // `class_eval` block or a guarded class-body statement -- the plain
    // class-body form resolves at compile time): with names, validate and
    // mark visibility in the runtime overlay; the argument-less
    // default-visibility form is a documented nil no-op (see
    // `runtime_set_visibility`).
    "private" => fn private_m(recv, args, _block) {
        crate::runtime_meta::runtime_set_visibility(
            recv_cid(recv), args, crate::dispatch::MethodVisibility::Private)
    }
    "public" => fn public_m(recv, args, _block) {
        crate::runtime_meta::runtime_set_visibility(
            recv_cid(recv), args, crate::dispatch::MethodVisibility::Public)
    }
    "protected" => fn protected_m(recv, args, _block) {
        crate::runtime_meta::runtime_set_visibility(
            recv_cid(recv), args, crate::dispatch::MethodVisibility::Protected)
    }
    // `Module#class_eval`/`module_eval` -- run the block with `self` rebound
    // to the module/class value, returning the block's value. A
    // `def`/`define_method` inside installs on the receiver via the dynamic-
    // self path (self is a Class). `module_eval` is an alias.
    "class_eval" | "module_eval" => fn class_eval(recv, args, block) {
        if let Some(arg) = args.first() {
            // The string form, through the eval VM with `self` rebound to the
            // class -- the same routing `instance_eval` already uses, and the
            // compiler ALREADY links the eval runtime for it
            // (`Hir::uses_runtime_eval`). Ignoring `args` here meant a string
            // form fell through to the block path and reported the misleading
            // "tried to create Proc object without a block".
            return crate::eval_vm::eval_value_mode(
                arg.clone(),
                recv.clone(),
                0,
                crate::eval_vm::EvalMode::ClassEval,
            );
        }
        let blk = crate::builtins::basic_object::block_proc(block, "class_eval")?;
        blk.call_with_self(recv, &[])
    }
    // `Module#class_exec`/`module_exec(*args) { |*a| ... }` -- like class_eval
    // but forwards positional args to the block's params.
    "class_exec" | "module_exec" => fn class_exec(recv, args, block) {
        let blk = crate::builtins::basic_object::block_proc(block, "class_exec")?;
        blk.call_with_self(recv, args)
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
        Err(name_error!("uninitialized class variable @@{name} in {}",
                crate::dispatch::class_name(cid).unwrap_or_default()))
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
        crate::cvar_set(owner.0, &name, args[1].clone())?;
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
    // `Module#class_variables([inherit=true])` -- the `@@name` symbols owned by
    // this class and (unless `inherit` is false) its ancestors, own first.
    // Names store bare (`x`); the reflection re-adds the `@@` prefix.
    "class_variables" => fn class_variables(recv, args, _block) {
        arity!(args, 0..=1);
        let cid = recv_cid(recv);
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        let mut push_owner = |owner: crate::ClassId, out: &mut Vec<RubyValue>| {
            for name in crate::cvar_names_of(owner.0) {
                if seen.insert(name.clone()) {
                    out.push(RubyValue::Symbol(crate::Symbol::intern(&format!("@@{name}"))));
                }
            }
        };
        push_owner(cid, &mut out);
        if inherit_flag(args) {
            for anc in crate::dispatch::ancestors_of_value(cid) {
                if *anc == cid
                    || *anc == zeo_abi::OBJECT_CLASS
                    || *anc == zeo_abi::BASIC_OBJECT_CLASS
                {
                    continue;
                }
                push_owner(*anc, &mut out);
            }
        }
        Ok(RubyValue::Array(crate::array_new(out)))
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
        return Err(type_error!("compared with non class/module"));
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
        _ => Err(type_error!(
            "{} is not a symbol nor a string",
            v.inspect_string()
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
        None => Err(name_error!(
            "'{raw}' is not allowed as a class variable name"
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
            return Err(type_error!(
                "{} is not a symbol nor a string",
                v.inspect_string()
            ));
        }
    };
    match raw.strip_prefix('@') {
        Some(name) => Ok(name.to_string()),
        None => Err(name_error!(
            "'{raw}' is not allowed as an instance variable name"
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
        if cid == zeo_abi::CLASS_CLASS {
            let superclass = args.first().cloned();
            let body = match &block {
                Some(RubyValue::Proc(p)) => Some(p.clone()),
                _ => None,
            };
            return crate::runtime_class_new(superclass, body);
        }
        // `Module.new { body }` -- an anonymous module (no superclass, no
        // constructor); the block populates it just like a class body.
        if cid == zeo_abi::MODULE_CLASS {
            let body = match &block {
                Some(RubyValue::Proc(p)) => Some(p.clone()),
                _ => None,
            };
            return crate::runtime_meta::runtime_module_new(body);
        }
        // `Enumerator.new([size]) { |y| ... }` is the ONE builtin with a
        // runtime allocator; parse deliberately skips the
        // static `New` node for it so the block arrives here.
        if cid == zeo_abi::ENUMERATOR_CLASS {
            return crate::builtins::enumerator::enumerator_new(args, block);
        }
        // `BasicObject.new` -- instantiable in real Ruby: the same blank
        // instance `Object.new` builds, tagged with the root class's own id.
        // Its `initialize` (the true root's) takes no arguments.
        if cid == zeo_abi::BASIC_OBJECT_CLASS {
            crate::builtins::arity!(args, 0);
            return Ok(crate::runtime_meta::blank_instance(cid));
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

    // `Class#allocate` -- a fresh instance WITHOUT running `initialize`. A user
    // class allocates its zero-initialized struct via its registered allocator;
    // a builtin value class answers its empty value (`String.allocate` -> `""`,
    // like CRuby, whose `allocate` yields the class's default instance).
    "allocate" => fn allocate_m(recv, _args, _block) {
        let cid = recv_cid(recv);
        if let Some(v) = builtin_allocate(cid) {
            return Ok(v);
        }
        match crate::dispatch::allocate_of(cid) {
            Some(v) => Ok(v),
            None => {
                let n = crate::dispatch::class_name(cid)
                    .unwrap_or_else(|| format!("#<Class:{}>", cid.0));
                Err(type_error!("allocator undefined for {n}"))
            }
        }
    }
}

/// The empty/default value a builtin value class's `allocate` yields, matching
/// CRuby (`String.allocate == ""`, `Array.allocate == []`, `Hash.allocate ==
/// {}`). `None` for a user or non-value class, which routes to its registered
/// allocator instead.
fn builtin_allocate(cid: crate::ClassId) -> Option<RubyValue> {
    match cid {
        zeo_abi::STRING_CLASS => Some(RubyValue::Str(crate::string_new(String::new()))),
        zeo_abi::ARRAY_CLASS => Some(RubyValue::Array(crate::array_new(Vec::new()))),
        zeo_abi::HASH_CLASS => Some(RubyValue::Hash(crate::hash_new(Vec::new()))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeo_abi::*;

    #[test]
    fn module_case_eq_checks_ancestry_registry_free() {
        // 5.class == Integer; Integer's fallback chain contains Numeric.
        let r = case_eq(&RubyValue::Class(NUMERIC_CLASS), &[RubyValue::Int(5)], None).unwrap();
        assert!(matches!(r, RubyValue::Bool(true)));
        let r = case_eq(&RubyValue::Class(STRING_CLASS), &[RubyValue::Int(5)], None).unwrap();
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

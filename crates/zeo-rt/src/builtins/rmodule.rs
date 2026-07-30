//! `Module` receiver methods (CRuby module.c/object.c), found via the walk on
//! a `RubyValue::Class` receiver whose `class_id()` is `MODULE_CLASS` -- or
//! `CLASS_CLASS`, since `Class`'s ancestry passes through `Module`, so a class
//! value sees this table too. `Module` owns `name`/`ancestors`/`===`; its
//! sibling `Class` (which owns `new`/`allocate`) lives in `rclass.rs`. Both are
//! r-prefixed (like `rproc`/`rstruct`) because `class`/`module` are Rust
//! keywords. The shared `recv_cid` helper is `pub(crate)` for `rclass` to use.

use crate::RubyValue;
use crate::builtins::{arity, name_error, type_error};
use zeo_macros::ruby_class;

pub(crate) fn recv_cid(recv: &RubyValue) -> crate::ClassId {
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

/// A Symbol-or-String constant name; TypeError on anything else.
fn const_name_arg(v: &RubyValue) -> Result<String, crate::Signal> {
    match v {
        RubyValue::Symbol(s) => Ok(s.name()),
        RubyValue::Str(s) => Ok(s.lock().to_utf8_lossy().into_owned()),
        other => Err(type_error!(
            "{} is not a symbol nor a string",
            other.inspect_string()
        )),
    }
}

/// `cid`'s own constant table first, then its ancestry when `inherit`. Shared
/// by `const_get` and `const_defined?` so the two can't disagree.
fn const_lookup(cid: crate::ClassId, name: &str, inherit: bool) -> Option<RubyValue> {
    crate::constants::const_get(cid.0, name).or_else(|| {
        inherit.then(|| {
            crate::dispatch::ancestors_of_value(cid)
                .iter()
                .find_map(|anc| crate::constants::const_get(anc.0, name))
        })?
    })
}

/// `defined?(Scope::NAME)`'s membership test, for a scope codegen resolved but
/// a name it could not: the constant may not exist until a `const_set` runs.
/// Shares `const_lookup` with `const_defined?`, whose semantics these are.
pub fn const_defined_in(cid: crate::ClassId, name: &str) -> bool {
    const_lookup(cid, name, true).is_some()
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

ruby_class! {
    Module = zeo_abi::MODULE_CLASS < zeo_abi::OBJECT_CLASS;

    // `#name` answers only for a class REACHABLE by a constant path.
    // `Class.new`, `Module.new`, an unassigned `Struct.new`, and every
    // singleton class are nameless, so they answer nil -- while `#to_s`
    // still renders each of them, which is the whole distinction.
    def "name" (recv, args, _block) {
        arity!(args, 0);
        Ok(match crate::dispatch::class_real_name(recv_cid(recv)) {
            Some(n) => RubyValue::Str(crate::string_new(n)),
            None => RubyValue::Nil,
        })
    }
    def "to_s" | "inspect" (recv, args, _block) {
        arity!(args, 0);
        let cid = recv_cid(recv);
        let n = crate::dispatch::class_name(cid).unwrap_or_else(|| format!("#<Class:{}>", cid.0));
        Ok(RubyValue::Str(crate::string_new(n)))
    }
    // The mixin hooks' DEFAULTS. Ruby fires each one on every mixin whether
    // or not the module defines it, so these exist to be the no-op that
    // answers -- and, more to the point, to be what a `def self.included`
    // that ends in `super` reaches.
    def "included" | "extended" | "prepended" (_recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Nil)
    }
    // `Class#inherited`'s default, for the same reason. It lives on Module
    // rather than Class because that is where zeo's class-method `super`
    // chain looks, and no module is ever inherited from.
    def "inherited" (_recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Nil)
    }
    // `Class#superclass` -- the first non-module entry after self in the
    // linearized ancestors (prepends/includes are modules, so this lands on
    // the real parent class); `nil` at the root (`BasicObject`).
    def "superclass" (recv, args, _block) {
        arity!(args, 0);
        let cid = recv_cid(recv);
        // `superclass` is a `Class` method; a Module receiver has none
        // (`Enumerable.superclass` raises NoMethodError, not nil).
        if crate::dispatch::class_is_module(cid).unwrap_or(false) {
            return Err(crate::dispatch::raise_method_missing(
                recv,
                "superclass",
                args,
                crate::dispatch::MissingReason::NoEntry,
            ));
        }
        let ancestors = crate::dispatch::ancestors_of_value(cid);
        for &anc in ancestors.iter().skip_while(|&&a| a != cid).skip(1) {
            if !crate::dispatch::class_is_module(anc).unwrap_or(false) {
                return Ok(RubyValue::Class(anc));
            }
        }
        Ok(RubyValue::Nil)
    }
    def "ancestors" (recv, args, _block) {
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
    def "included_modules" (recv, args, _block) {
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
    def "const_set" (recv, args, _block) {
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
    def "private_constant" (recv, args, _block) {
        constant_visibility_no_op(recv, args)
    }
    def "public_constant" (recv, args, _block) {
        constant_visibility_no_op(recv, args)
    }
    // `Module#deprecate_constant(:A, ...)` -- same argument validation, same
    // reason for not enforcing: CRuby warns on ACCESS, and zeo binds constant
    // references at compile time, so there is no runtime read to hook. (CRuby's
    // warning is itself off unless `Warning[:deprecated]` is on, which it isn't
    // by default -- so the common case agrees exactly.) net/http deprecates its
    // legacy response-class aliases at load time.
    def "deprecate_constant" (recv, args, _block) {
        constant_visibility_no_op(recv, args)
    }
    def "const_get" (recv, args, _block) {
        arity!(args, 1..=2);
        let name = const_name_arg(&args[0])?;
        const_lookup(recv_cid(recv), &name, inherit_flag(&args[1..]))
            .ok_or_else(|| name_error!("uninitialized constant {name}"))
    }
    def "const_defined?" (recv, args, _block) {
        arity!(args, 1..=2);
        let name = const_name_arg(&args[0])?;
        let found = const_lookup(recv_cid(recv), &name, inherit_flag(&args[1..])).is_some();
        Ok(RubyValue::Bool(found))
    }
    // Returns the removed value; NameError when the constant isn't this
    // module's own (an inherited one doesn't count).
    def "remove_const" (recv, args, _block) {
        arity!(args, 1);
        let cid = recv_cid(recv);
        let name = const_name_arg(&args[0])?;
        crate::constants::const_remove(cid.0, &name)
            .ok_or_else(|| name_error!("constant {name} not defined"))
    }
    def "constants" (recv, args, _block) {
        arity!(args, 0..=1);
        let cid = recv_cid(recv);
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        let mut push_owner = |owner: crate::ClassId, out: &mut Vec<RubyValue>| {
            // Two sources, because a module's constants are stored two ways: an
            // ordinary `FOO = 1` lands in the constant table, while a nested
            // `class Bar` is registered by its qualified NAME and never reaches
            // that table (codegen resolves `Foo::Bar` statically). Both are
            // constants of `Foo` as far as Ruby is concerned.
            let named = crate::constants::const_names_of(owner.0);
            for name in named.into_iter().chain(crate::dispatch::nested_class_names(owner)) {
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
    def "include?" (recv, args, _block) {
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
    def "===" (recv, args, _block) {
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
    def "<" (recv, args, _block) {
        arity!(args, 1);
        module_ordering_op(recv, &args[0], |o| matches!(o, std::cmp::Ordering::Less))
    }
    def "<=" (recv, args, _block) {
        arity!(args, 1);
        module_ordering_op(recv, &args[0], |o| matches!(o, std::cmp::Ordering::Less | std::cmp::Ordering::Equal))
    }
    def ">" (recv, args, _block) {
        arity!(args, 1);
        module_ordering_op(recv, &args[0], |o| matches!(o, std::cmp::Ordering::Greater))
    }
    def ">=" (recv, args, _block) {
        arity!(args, 1);
        module_ordering_op(recv, &args[0], |o| matches!(o, std::cmp::Ordering::Greater | std::cmp::Ordering::Equal))
    }
    def "<=>" (recv, args, _block) {
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
    def "subclasses" (recv, args, _block) {
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
    def "singleton_class?" (recv, args, _block) {
        arity!(args, 0);
        let _ = recv_cid(recv);
        Ok(RubyValue::Bool(false))
    }
    // Reflection over a CLASS OBJECT's own ivars -- the `@x` a `def self.x`
    // or a class body writes (see `civars`' docs). Really `Object`'s
    // methods, which a class inherits; they live on the Module table
    // because that is the one a `RubyValue::Class` receiver reaches.
    def "instance_variable_get" (recv, args, _block) {
        arity!(args, 1);
        let name = ivar_name_arg(&args[0])?;
        Ok(crate::civars::class_ivar_get(recv_cid(recv).0, &name))
    }
    def "instance_variable_set" (recv, args, _block) {
        arity!(args, 2);
        let name = ivar_name_arg(&args[0])?;
        crate::civars::class_ivar_set(recv_cid(recv).0, &name, args[1].clone())?;
        // Answers the VALUE, not the receiver -- oracle-checked.
        Ok(args[1].clone())
    }
    def "instance_variable_defined?" (recv, args, _block) {
        arity!(args, 1);
        let name = ivar_name_arg(&args[0])?;
        Ok(RubyValue::Bool(
            crate::civars::class_ivar_names(recv_cid(recv).0).contains(&name),
        ))
    }
    def "instance_variables" (recv, args, _block) {
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
    def "method_defined?" (recv, args, _block) {
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
    def "instance_methods" (recv, args, _block) {
        arity!(args, 0..=1);
        let names = crate::dispatch::instance_method_names(
            recv_cid(recv),
            crate::dispatch::VisFilter::NotPrivate,
            inherit_flag(args),
        );
        Ok(syms_to_array(names))
    }
    // `public_instance_methods` narrows to public ONLY (protected excluded).
    def "public_instance_methods" (recv, args, _block) {
        arity!(args, 0..=1);
        let names = crate::dispatch::instance_method_names(
            recv_cid(recv),
            crate::dispatch::VisFilter::Public,
            inherit_flag(args),
        );
        Ok(syms_to_array(names))
    }
    def "private_instance_methods" (recv, args, _block) {
        arity!(args, 0..=1);
        let names = crate::dispatch::instance_method_names(
            recv_cid(recv),
            crate::dispatch::VisFilter::Private,
            inherit_flag(args),
        );
        Ok(syms_to_array(names))
    }
    def "protected_instance_methods" (recv, args, _block) {
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
    def "public_method_defined?" (recv, args, _block) {
        arity!(args, 1..=2);
        Ok(RubyValue::Bool(method_defined_with_vis(
            recv, &args[0], crate::dispatch::MethodVisibility::Public)?))
    }
    def "private_method_defined?" (recv, args, _block) {
        arity!(args, 1..=2);
        Ok(RubyValue::Bool(method_defined_with_vis(
            recv, &args[0], crate::dispatch::MethodVisibility::Private)?))
    }
    def "protected_method_defined?" (recv, args, _block) {
        arity!(args, 1..=2);
        Ok(RubyValue::Bool(method_defined_with_vis(
            recv, &args[0], crate::dispatch::MethodVisibility::Protected)?))
    }
    // `Module#instance_method(:name)` -> an UnboundMethod for the module/class.
    def "instance_method" (recv, args, _block) {
        arity!(args, 1);
        crate::builtins::unbound_method::unbound_method_new(recv_cid(recv), &args[0])
    }
    // `Module#define_method(name) { body }` -- install/override an
    // instance method AT RUNTIME (a computed name, or inside an `each` loop).
    // The literal `define_method(:sym) { ... }` form is desugared to a `def` at
    // compile time in zeo; this row serves everything that isn't literal.
    def "define_method" (recv, args, block) {
        arity!(args, 1..=2);
        let name = crate::runtime_meta::coerce_method_name(args.first())?;
        // A `Method`/`UnboundMethod` second argument installs that method's
        // own definition under `name` (not a Proc body).
        if let Some(src) = args.get(1) {
            if let Some((owner, src_name)) = crate::builtins::method::method_source(src) {
                return crate::runtime_meta::runtime_define_method_from_method(
                    recv_cid(recv), name, owner, src_name);
            }
        }
        let body = crate::runtime_meta::coerce_method_body(args, &block)?;
        crate::runtime_define_method(recv_cid(recv), name, body)
    }
    // `Module#alias_method(new_name, old_name)` -- the RUNTIME form (a
    // computed name, or inside an `each` loop: ostruct's bulk `!`-alias
    // loop). The literal class-body form resolves at compile time; this row
    // serves everything that isn't literal. Snapshot semantics -- the alias
    // keeps the method `old_name` resolves to NOW -- and returns the new
    // name's Symbol, both per CRuby.
    def "alias_method" (recv, args, _block) {
        arity!(args, 2);
        let new = crate::runtime_meta::coerce_method_name(args.first())?;
        let old = crate::runtime_meta::coerce_method_name(args.get(1))?;
        crate::runtime_meta::runtime_alias_method(recv_cid(recv), new, old)
    }
    // `Module#include(M, ...)` reached at RUNTIME on a Class/Module receiver
    // (`Class.new { include M }`, `mod.class_eval { include Other }`): mix each
    // module's instance methods into the receiver's runtime ancestry. The plain
    // class-body form resolves statically; this serves the runtime shapes.
    def "include" (recv, args, _block) {
        crate::runtime_meta::runtime_include(recv, args)
    }
    def "prepend" (recv, args, _block) {
        crate::runtime_meta::runtime_prepend(recv, args)
    }
    // The literal class-body forms of these four expand to real `def`s at
    // compile time; these rows serve `Class.new { }` and `class_eval { }`.
    def "attr_reader" (recv, args, _block) {
        crate::runtime_meta::runtime_attr(recv_cid(recv), args, crate::runtime_meta::AttrKind::Reader)
    }
    def "attr_writer" (recv, args, _block) {
        crate::runtime_meta::runtime_attr(recv_cid(recv), args, crate::runtime_meta::AttrKind::Writer)
    }
    def "attr_accessor" (recv, args, _block) {
        crate::runtime_meta::runtime_attr(recv_cid(recv), args, crate::runtime_meta::AttrKind::Accessor)
    }
    // `attr :x` is a reader; the deprecated `attr :x, true` is an accessor.
    def "attr" (recv, args, _block) {
        let accessor = matches!(args.last(), Some(RubyValue::Bool(true)));
        let names = if accessor { &args[..args.len() - 1] } else { args };
        let kind = if accessor {
            crate::runtime_meta::AttrKind::Accessor
        } else {
            crate::runtime_meta::AttrKind::Reader
        };
        crate::runtime_meta::runtime_attr(recv_cid(recv), names, kind)
    }
    // A no-op by construction: it flags a method to pass a bare `*args`
    // trailing hash through as keywords, and zeo's keyword arguments are
    // already carried separately from the positionals.
    def "ruby2_keywords" (_recv, _args, _block) {
        Ok(RubyValue::Nil)
    }
    def "undef_method" (recv, args, _block) {
        crate::runtime_meta::runtime_undef_method(recv_cid(recv), args)
    }
    def "remove_method" (recv, args, _block) {
        crate::runtime_meta::runtime_remove_method(recv_cid(recv), args)
    }
    // `Module#private`/`public`/`protected` reached at RUNTIME (inside a
    // `class_eval` block or a guarded class-body statement -- the plain
    // class-body form resolves at compile time): with names, validate and
    // mark visibility in the runtime overlay; the argument-less
    // default-visibility form is a documented nil no-op (see
    // `runtime_set_visibility`).
    def "private" (recv, args, _block) {
        crate::runtime_meta::runtime_set_visibility(
            recv_cid(recv), args, crate::dispatch::MethodVisibility::Private)
    }
    def "public" (recv, args, _block) {
        crate::runtime_meta::runtime_set_visibility(
            recv_cid(recv), args, crate::dispatch::MethodVisibility::Public)
    }
    def "protected" (recv, args, _block) {
        crate::runtime_meta::runtime_set_visibility(
            recv_cid(recv), args, crate::dispatch::MethodVisibility::Protected)
    }
    // `Module#module_function(name)` reached at RUNTIME (a computed argument --
    // fileutils' `private_module_function` calls `module_function name`). The
    // literal form resolves at compile time in `lower/defs.rs`. Promotes the
    // named instance method to a module method (see `runtime_module_function`).
    def "module_function" (recv, args, _block) {
        crate::runtime_meta::runtime_module_function(recv_cid(recv), args)
    }
    // `private_class_method`/`public_class_method` at RUNTIME. The literal
    // form resolves at compile time in `lower/defs.rs`; this marks the
    // overlay, which outranks whatever the frozen registry baked in.
    def "private_class_method" (recv, args, _block) {
        crate::runtime_meta::runtime_class_method_visibility(recv_cid(recv), args, true)?;
        Ok(recv.clone())
    }
    def "public_class_method" (recv, args, _block) {
        crate::runtime_meta::runtime_class_method_visibility(recv_cid(recv), args, false)?;
        Ok(recv.clone())
    }
    // `Module#class_eval`/`module_eval` -- run the block with `self` rebound
    // to the module/class value, returning the block's value. A
    // `def`/`define_method` inside installs on the receiver via the dynamic-
    // self path (self is a Class). `module_eval` is an alias.
    def "class_eval" | "module_eval" (recv, args, block) {
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
        crate::runtime_meta::with_body_frame(recv_cid(recv), || blk.call_with_self(recv, &[]))
    }
    // `Module#class_exec`/`module_exec(*args) { |*a| ... }` -- like class_eval
    // but forwards positional args to the block's params.
    def "class_exec" | "module_exec" (recv, args, block) {
        let blk = crate::builtins::basic_object::block_proc(block, "class_exec")?;
        crate::runtime_meta::with_body_frame(recv_cid(recv), || blk.call_with_self(recv, args))
    }
    // `Module#class_variable_get/set/defined?` over the linearized ancestry
    // (a `@@x` is owned by the nearest ancestor that first assigned it --
    // see `cvars`' docs). `get` on a never-assigned name is a `NameError`,
    // unlike a plain `@@x` read's nil-on-miss.
    def "class_variable_get" (recv, args, _block) {
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
    def "class_variable_set" (recv, args, _block) {
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
    def "class_variable_defined?" (recv, args, _block) {
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
    def "class_variables" (recv, args, _block) {
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
            // No Object/BasicObject exclusion here, unlike `constants` -- a
            // `@@x` can only reach Object through an explicit `class Object`
            // body (a top-level one raises; see `Hir::cvar_is_toplevel`), and
            // CRuby does report that one from every descendant.
            for anc in crate::dispatch::ancestors_of_value(cid) {
                if *anc != cid {
                    push_owner(*anc, &mut out);
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use zeo_abi::*;

    /// The `Module` rows are `ruby_class!`-generated (their Rust fn names are
    /// mangled), so reach them the way dispatch does -- through the registered
    /// instance table keyed by `MODULE_CLASS`.
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(MODULE_CLASS)
            .expect("Module is a registered builtin table")
            .instance
            .as_ref()
            .expect("Module has instance methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Module#{name} is defined"))
    }

    #[test]
    fn module_case_eq_checks_ancestry_registry_free() {
        // 5.class == Integer; Integer's fallback chain contains Numeric.
        let case_eq = imethod("===");
        let r = case_eq(&RubyValue::Class(NUMERIC_CLASS), &[RubyValue::Int(5)], None).unwrap();
        assert!(matches!(r, RubyValue::Bool(true)));
        let r = case_eq(&RubyValue::Class(STRING_CLASS), &[RubyValue::Int(5)], None).unwrap();
        assert!(matches!(r, RubyValue::Bool(false)));
    }

    #[test]
    fn ancestors_row_reflects_the_fallback_chain() {
        let RubyValue::Array(a) =
            imethod("ancestors")(&RubyValue::Class(INTEGER_CLASS), &[], None).unwrap()
        else {
            panic!()
        };
        assert_eq!(a.lock().len(), 6); // [Integer, Numeric, Comparable, Object, Kernel, BasicObject]
    }
}

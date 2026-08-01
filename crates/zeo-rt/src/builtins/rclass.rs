//! `Class` receiver methods (CRuby class.c) -- the two things a class value
//! does that a plain `Module` does not: `new` (allocate + `initialize`) and
//! `allocate` (a blank instance). Every other class-value method
//! (`name`/`ancestors`/`===`/...) is inherited from `Module` through the
//! ancestry walk, since `Class < Module`; that table lives in `rmodule.rs`,
//! from which this file borrows the shared `recv_cid`. Both are r-prefixed
//! (like `rproc`/`rstruct`) because `class`/`module` are Rust keywords.

use crate::RubyValue;
use crate::builtins::rmodule::recv_cid;
use crate::builtins::type_error;
use zeo_macros::ruby_class;

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

ruby_class! {
    Class = zeo_abi::CLASS_CLASS < zeo_abi::MODULE_CLASS;

    // `Class#new` -- the registry's dynamic constructor (`x = Widget;
    // x.new(...)`). A class with no allocator (builtins, exception-less
    // edge cases) raises real Ruby's NoMethodError shape for its kind.
    def "new"(recv, *args, &block) {
        let cid = recv_cid(recv);
        // `Class.new(superclass) { body }` -- `recv` is `Class`
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
            crate::builtins::check_arity(args.len(), 0, Some(0))?;
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
    def "allocate"(recv) {
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

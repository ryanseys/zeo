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

/// The empty/default value a builtin class's `allocate` yields, matching CRuby
/// (`String.allocate == ""`, `Array.allocate == []`, `Hash.allocate == {}`,
/// `Range.allocate` a beginless-and-endless range). `None` for a user or
/// non-value class, which routes to its registered allocator instead.
///
/// `Object`/`BasicObject` land here rather than on a registered allocator
/// because their instances have no payload to allocate -- CRuby's is a blank
/// object, and so is this.
pub(crate) fn builtin_allocate(cid: crate::ClassId) -> Option<RubyValue> {
    match cid {
        zeo_abi::STRING_CLASS => Some(RubyValue::Str(crate::string_new(String::new()))),
        zeo_abi::ARRAY_CLASS => Some(RubyValue::Array(crate::array_new(Vec::new()))),
        zeo_abi::HASH_CLASS => Some(RubyValue::Hash(crate::hash_new(Vec::new()))),
        zeo_abi::RANGE_CLASS => Some(crate::builtins::range::range_value(None, None, false)),
        zeo_abi::OBJECT_CLASS | zeo_abi::BASIC_OBJECT_CLASS => {
            Some(crate::runtime_meta::blank_instance(cid))
        }
        _ => None,
    }
}

ruby_class! {
    Class = zeo_abi::CLASS_CLASS < zeo_abi::MODULE_CLASS;

    // `Class#attached_object` -- the object a SINGLETON class belongs to.
    // Anything else is CRuby's TypeError, quoting the receiver's own inspect.
    def "attached_object"(recv) {
        let cid = crate::builtins::rmodule::recv_cid(recv);
        match crate::runtime_meta::is_live()
            .then(|| crate::runtime_meta::singleton_class_owner(cid))
            .flatten()
        {
            Some(owner) => Ok(RubyValue::Class(owner)),
            None => Err(crate::builtins::type_error!(
                "'{}' is not a singleton class",
                recv.inspect_string()
            )),
        }
    }

    // `Class#new` -- the registry's dynamic constructor (`x = Widget;
    // x.new(...)`). A class with no allocator (builtins, exception-less
    // edge cases) raises real Ruby's NoMethodError shape for its kind.
    def "new"(recv, *args, &block) {
        let cid = recv_cid(recv);
        if crate::runtime_meta::class_is_uninitialized(cid) {
            return Err(type_error!("can't instantiate uninitialized class"));
        }
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
        // `Range.new(begin, end, exclude_end = false)` -- the literal `a..b`
        // under another name, so the endpoints normalize the same way and the
        // two endpoints must be comparable (`Range.new(1, "a")` is the
        // ArgumentError a literal could never reach).
        if cid == zeo_abi::RANGE_CLASS {
            crate::builtins::check_arity(args.len(), 2, Some(3))?;
            let excl = args.get(2).is_some_and(|v| v.truthy());
            return crate::range_checked(
                crate::range_endpoint(args[0].clone()),
                crate::range_endpoint(args[1].clone()),
                excl,
            );
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
        if crate::runtime_meta::class_is_uninitialized(cid) {
            return Err(type_error!("can't instantiate uninitialized class"));
        }
        if let Some(v) = builtin_allocate(cid) {
            return Ok(v);
        }
        // An exception carries a native payload every `Exception` method reads,
        // so a blank one is the native constructor with no arguments -- which
        // is what makes `RuntimeError.allocate.message` the class name.
        if crate::dispatch::ancestors_of_value(cid).contains(&zeo_abi::EXCEPTION_CLASS) {
            return crate::builtins::exception::exception_construct(cid, &[], None);
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

    // `Class.allocate` -- ruby declares it on `Class`'s OWN singleton, where it
    // answers a class with no superclass at all rather than an instance of
    // `Class`. Every other receiver falls through to the `Class#allocate` row
    // below.
    def self."allocate"(_recv) {
        Ok(crate::runtime_meta::runtime_class_allocate())
    }

    // `Class#superclass` -- the first non-module entry after self in the
    // linearized ancestors (prepends/includes are modules, so this lands on
    // the real parent class); `nil` at the root (`BasicObject`). Declaring it
    // HERE is what makes `Enumerable.superclass` the NoMethodError CRuby
    // raises: a module receiver never reaches this table.
    // Any Class a program can hold is initialized (CRuby raises even for
    // `Class.allocate` + first initialize) -- the row IS the refusal.
    private def "initialize" cfunc (_recv, *_args, &_block) {
        Err(type_error!("already initialized class"))
    }
    def "superclass" (recv) {
        let cid = recv_cid(recv);
        // "no superclass yet" is not "no superclass": `Class.allocate`'s
        // result raises here where `BasicObject` answers nil.
        if crate::runtime_meta::class_is_uninitialized(cid) {
            return Err(type_error!("uninitialized class"));
        }
        let ancestors = crate::dispatch::ancestors_of_value(cid);
        for &anc in ancestors.iter().skip_while(|&&a| a != cid).skip(1) {
            if !crate::dispatch::class_is_module(anc).unwrap_or(false) {
                return Ok(RubyValue::Class(anc));
            }
        }
        Ok(RubyValue::Nil)
    }

    // `Class#subclasses`: the DIRECT, currently-registered subclasses. Order
    // is unspecified in CRuby (a hash-set walk), so this returns them in the
    // registry's iteration order -- tests that assert a listing sort it.
    def "subclasses" (recv) {
        let kids = crate::dispatch::direct_subclasses(recv_cid(recv))
            .into_iter()
            .map(RubyValue::Class)
            .collect();
        Ok(RubyValue::Array(crate::array_new(kids)))
    }

    // `Class#inherited`'s default -- the no-op hook a user override's `super`
    // reaches, the `Module#included`/`extended`/`prepended` trio's sibling.
    private def "inherited" (_recv, _arg) {
        Ok(RubyValue::Nil)
    }
}

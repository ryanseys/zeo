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
/// Whether [`builtin_allocate`] has a blank instance for this class, asked
/// without building one. A class with no blank form keeps its fused `new`
/// (`tests/gaps/allocate_on_a_value_class`): diverting to `Class#new` there
/// would trade a working construction for `allocator undefined`.
/// Whether `allocate` is `undef`'d on this class rather than merely
/// unallocatable -- see [`zeo_abi::ALLOCATE_UNDEFINED`]. Asked over the
/// ancestors because CRuby's undef sits on the singleton chain, so a subclass
/// of `MatchData` refuses too.
pub(crate) fn allocate_is_undefined(cid: crate::ClassId) -> bool {
    zeo_abi::ALLOCATE_UNDEFINED.contains(&cid)
        || crate::dispatch::ancestors_of_value(cid)
            .iter()
            .any(|a| zeo_abi::ALLOCATE_UNDEFINED.contains(a))
}

pub(crate) fn can_builtin_allocate(cid: crate::ClassId) -> bool {
    crate::builtins::allocator_of(cid).is_some()
        || matches!(
            cid,
            zeo_abi::STRING_CLASS
                | zeo_abi::ARRAY_CLASS
                | zeo_abi::HASH_CLASS
                | zeo_abi::RANGE_CLASS
                | zeo_abi::OBJECT_CLASS
                | zeo_abi::BASIC_OBJECT_CLASS
        )
}

pub(crate) fn builtin_allocate(cid: crate::ClassId) -> Option<RubyValue> {
    // A class that declares `allocate <fn>;` in its `ruby_class!` header
    // answers through its own table. Asked FIRST so a class can state its own
    // blank value rather than being added to the match below.
    if let Some(f) = crate::builtins::allocator_of(cid) {
        return Some(f());
    }
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

/// `Class#new`'s allocate-then-`initialize` half, for a BUILTIN whose
/// `initialize` a program has reopened.
///
/// `Some(v)` means a user row won and `v` is the finished instance; `None`
/// means the class's own native constructor still owns construction.
///
/// **Why the predicate can be exact.** A compile-time reopen of a builtin
/// registers on the VALUE channel (`ClassEntry::value_methods`), and the
/// native row lives in the class's static `MethodTable`. They are different
/// tables. `method_owner` cannot tell them apart -- which the gap file
/// recorded as "indistinguishable", naming the wrong table rather than a
/// missing fact.
///
/// **Cost.** Asked only when the chain HAS a reopened `initialize`, so a
/// program that never reopens a builtin pays one set test per `new`.
/// `bm_object_new` and `bm_object_new_init` measure it.
fn user_initialize_construct(
    cid: crate::ClassId,
    args: &[RubyValue],
    block: &Option<RubyValue>,
) -> Result<Option<RubyValue>, crate::Signal> {
    // A RUST class only. A user or runtime-minted class already constructs
    // correctly -- its `ConstructorFn` allocates and then runs `initialize`,
    // and `value_subclass_construct` does the same for a subclass of a
    // builtin. Unfusing those raised `allocator undefined` for every
    // `Class.new`-minted namespace.
    if crate::builtins::registered_table(cid).is_none() {
        return Ok(None);
    }
    // A C extension's registered allocator REPLACES whatever the class had
    // -- CRuby's rule. The official strscan over a retired builtin is the
    // case: its C `initialize` reads the TypedData only its own allocator
    // makes, so the unfused blank-native road here handed it the wrong
    // receiver. `constructor_of` takes the C road on the fall-through.
    if crate::capi_hooks::has_alloc_func(cid) {
        return Ok(None);
    }
    let init = crate::symbol::wk::initialize();
    if !crate::dispatch::reopened_initialize_in_chain(cid, init) {
        return Ok(None);
    }
    let Some(recv) = builtin_allocate(cid) else {
        // A reopened `initialize` on a class with no way to make a blank
        // instance. Refuse LOUDLY: answering the constructor's value would be
        // the very bug this fixes, silently.
        let n = crate::dispatch::class_name(cid).unwrap_or_else(|| format!("#<Class:{}>", cid.0));
        return Err(type_error!("allocator undefined for {n}"));
    };
    match &recv {
        RubyValue::Object(o) => crate::dispatch::run_initialize(cid, o, args, block.clone())?,
        // A VALUE class -- String, Hash, Time -- allocates a blank value
        // rather than a struct, so its `initialize` is sent like any other
        // method. The user's body replaces the native construction whole,
        // which is why `String.new("x")` answers `""` once one exists.
        _ => {
            crate::dispatch::send_value(
                &recv,
                crate::symbol::wk::initialize(),
                args,
                block.clone(),
            )?;
        }
    }
    Ok(Some(recv))
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
            crate::builtins::check_arity(args.len(), 0, Some(1))?;
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
            // A module has no superclass to take, so `Module.new` takes
            // nothing at all -- the block is its whole body.
            crate::builtins::check_arity(args.len(), 0, Some(0))?;
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
        // Ruby's `new` is `allocate` plus `initialize`, and a builtin's
        // registered constructor FUSES the two. That is right until a program
        // reopens the class with its own `initialize`: the constructor cannot
        // run a body it does not know about, so `Pathname.new` left `@path`
        // nil and every method read it.
        //
        // Unfuse only when a USER row actually wins the lookup. A reopen with
        // no `initialize` keeps the constructor, which is what CRuby does too.
        if let Some(v) = user_initialize_construct(cid, args, &block)? {
            return Ok(v);
        }
        match crate::dispatch::constructor_of(cid) {
            Some(ctor) => ctor(cid, args, block),
            None => {
                let is_module = crate::dispatch::class_is_module(cid).unwrap_or(false);
                let kind = if is_module { "module" } else { "class" };
                let n = crate::dispatch::class_name(cid)
                    .unwrap_or_else(|| format!("#<Class:{}>", cid.0));
                Err(crate::builtins::no_method_error!("undefined method 'new' for {kind} {n}"))
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
        // Ruby UNDEFINES the name on these, so the refusal is a missing METHOD
        // rather than a missing allocator -- a different class and a different
        // message, and `rescue TypeError` around `MatchData.allocate` catches
        // nothing in ruby.
        if allocate_is_undefined(cid) {
            return Err(crate::dispatch::raise_method_missing(
                recv,
                "allocate",
                &[],
                crate::dispatch::MissingReason::NoEntry,
            ));
        }
        // A C extension's registered allocator replaces the builtin's own
        // blank -- same rule as `Class#new`'s unfused road above.
        if crate::capi_hooks::has_alloc_func(cid) {
            return match crate::dispatch::allocate_of(cid) {
                Some(v) => Ok(v),
                None => Err(crate::signal::take_pending().unwrap_or_else(|| {
                    let n = crate::dispatch::class_name(cid)
                        .unwrap_or_else(|| format!("#<Class:{}>", cid.0));
                    type_error!("allocator undefined for {n}")
                })),
            };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Signal;
    use crate::Symbol;

    // Each test runs in its own nextest process -- see the crate README for
    // the with_core() bootstrap pattern.
    fn install_core() {
        crate::dispatch::install_class_registry(crate::dispatch::ClassRegistry::with_core());
    }

    fn call(recv: &RubyValue, name: &str, args: &[RubyValue]) -> Result<RubyValue, Signal> {
        crate::dispatch::send_value(recv, Symbol::intern(name), args, None)
    }

    #[test]
    fn a_builtin_allocate_answers_the_empty_value() {
        install_core();
        let s = call(&RubyValue::Class(zeo_abi::STRING_CLASS), "allocate", &[]).unwrap();
        assert!(matches!(&s, RubyValue::Str(s) if s.lock().bytesize() == 0));
        let a = call(&RubyValue::Class(zeo_abi::ARRAY_CLASS), "allocate", &[]).unwrap();
        assert!(matches!(&a, RubyValue::Array(a) if a.lock().to_vec().is_empty()));
    }

    /// Every class that DECLARES a blank answers one, and the value reports
    /// the declaring class.
    ///
    /// The class id is the trap this guards. Several blanks share a payload
    /// whose own `class_id` names a different class -- a blank `File` and a
    /// blank socket are both `RIo`, which says `IO` unless the allocator
    /// stamps the override -- so a slot that compiles can still hand back an
    /// instance of the wrong class.
    #[test]
    fn every_declared_blank_reports_its_own_class() {
        install_core();
        let mut checked = 0;
        for b in zeo_abi::BUILTINS.iter() {
            let cid = crate::ClassId(b.id.0);
            let Some(f) = crate::builtins::allocator_of(cid) else {
                continue;
            };
            let v = f();
            assert_eq!(
                v.class_id(),
                cid,
                "{}.allocate answers an instance of {:?}",
                b.name,
                crate::dispatch::class_name(v.class_id())
            );
            checked += 1;
        }
        // The count is a floor, not a pin: a new `allocate` slot should join
        // this sweep without editing it. Zero would mean the sweep found
        // nothing and proved nothing.
        assert!(
            checked >= 20,
            "only {checked} classes declare a blank; the sweep is not reaching them"
        );
    }

    /// The five classes ruby `undef`s `allocate` on refuse with a
    /// NoMethodError rather than the TypeError a missing allocator gives, and
    /// the refusal is INHERITED -- ruby's undef sits on the singleton chain.
    #[test]
    fn an_undefined_allocate_refuses_as_a_missing_method() {
        install_core();
        for &cid in zeo_abi::ALLOCATE_UNDEFINED {
            assert!(
                allocate_is_undefined(cid),
                "{:?} is listed but not recognised",
                crate::dispatch::class_name(cid)
            );
            let err = call(&RubyValue::Class(cid), "allocate", &[]).unwrap_err();
            let Signal::Raise(exc) = err else {
                panic!("allocate raises");
            };
            assert_eq!(
                crate::dispatch::class_name(exc.class_id()).as_deref(),
                Some("NoMethodError")
            );
        }
        // A class NOT on the list keeps the other refusal.
        assert!(!allocate_is_undefined(zeo_abi::STRING_CLASS));
    }

    #[test]
    fn superclass_walks_past_modules_and_ends_nil_at_the_root() {
        install_core();
        // String's chain runs through Comparable (a module); the superclass
        // is the first non-module entry: Object.
        assert!(matches!(
            call(&RubyValue::Class(zeo_abi::STRING_CLASS), "superclass", &[]),
            Ok(RubyValue::Class(c)) if c == zeo_abi::OBJECT_CLASS
        ));
        assert!(matches!(
            call(
                &RubyValue::Class(zeo_abi::BASIC_OBJECT_CLASS),
                "superclass",
                &[]
            ),
            Ok(RubyValue::Nil)
        ));
    }

    #[test]
    fn a_module_has_no_new() {
        install_core();
        let err = call(&RubyValue::Class(zeo_abi::ENUMERABLE_CLASS), "new", &[]);
        let Err(Signal::Raise(exc)) = err else {
            panic!("expected a NoMethodError raise");
        };
        assert_eq!(
            exc.as_object_unchecked().class_id(),
            zeo_abi::NO_METHOD_ERROR_CLASS
        );
    }

    #[test]
    fn an_uninitialized_class_refuses_use() {
        install_core();
        // `Class.allocate` hands out a class with NO superclass at all.
        let bare = call(&RubyValue::Class(zeo_abi::CLASS_CLASS), "allocate", &[]).unwrap();
        let sup = call(&bare, "superclass", &[]);
        let Err(Signal::Raise(exc)) = sup else {
            panic!("expected a TypeError raise");
        };
        assert_eq!(
            exc.as_object_unchecked().class_id(),
            zeo_abi::TYPE_ERROR_CLASS
        );
        assert!(call(&bare, "new", &[]).is_err());
    }
}

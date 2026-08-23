//! The `ObjectSpace` module -- CRuby splits this surface across two files and
//! zeo doesn't: `gc.c` always defines `define_finalizer`/`each_object`/
//! `count_objects`, while `ext/objspace` bolts the introspection half
//! (`memsize_of`, `reachable_objects_from`, the allocation tracer, ...) onto
//! the same module when `require "objspace"` runs. zeo has no way to make a
//! subset of a module's methods appear on require -- the ABI's `feature` field
//! gates a whole class -- so every row here is always present and
//! `require "objspace"` is recognized ceremony, the same shape `io/wait` and
//! `io/console` already have (`docs/EXTENSIONS.md`).
//!
//! What each answer is worth:
//!
//! - **Real** -- `memsize_of` (computed from the value's own representation),
//!   `reachable_objects_from` (walks the value's direct references),
//!   `count_symbols` (the interner's own count).
//! - **Genuinely empty** -- `count_nodes`/`count_tdata_objects`/
//!   `count_imemo_objects` census VM object kinds zeo simply doesn't have, so
//!   an empty Hash is the true answer rather than a missing one.
//! - **`NotImplementedError`** -- anything needing heap enumeration, a GC root
//!   set, an allocation hook, or CRuby's object header. Never a hollow zero:
//!   `tests/gaps/README.md` is explicit that a stub is worse than an error.
//!
//! The finalizer and `WeakMap` machinery this module's `define_finalizer` rows
//! stand on lives next door in `weak.rs`.

use crate::collections::array_new;
use crate::{RubyValue, Symbol};

use super::weak::{FINALIZERS, Finalizer, WeakTarget, object_id_i64, same_object};
use super::{arg_error, not_impl_error};
use zeo_macros::ruby_module;

/// The footprint of one `RubyValue` -- zeo's counterpart to CRuby's 40-byte
/// heap slot, and the unit `memsize_of` counts in.
const SLOT: i64 = std::mem::size_of::<RubyValue>() as i64;

/// Whether `v` is stored IN the value word rather than behind a handle. These
/// are the shapes CRuby also calls immediate, and they're what `memsize_of`
/// answers 0 for and `reachable_objects_from` leaves out of its list.
fn is_immediate(v: &RubyValue) -> bool {
    matches!(
        v,
        RubyValue::Nil
            | RubyValue::Bool(_)
            | RubyValue::Int(_)
            | RubyValue::Float(_)
            | RubyValue::Symbol(_)
    )
}

/// `ObjectSpace.memsize_of(obj)` -- the bytes `obj` itself occupies.
///
/// CRuby documents this as implementation-defined ("the return value is
/// dependent on the version and build of the interpreter"), and it is: the
/// same call answers 40 for a short String on CRuby 4.0 and something else on
/// the next release. So a figure computed from zeo's own representation is a
/// legitimate answer, not an approximation of CRuby's -- one `SLOT` for the
/// value plus whatever it owns on the heap. Only the shape of the answer is
/// portable: 0 for an immediate, and bigger for a bigger payload.
pub(crate) fn memsize_of(v: &RubyValue) -> i64 {
    if is_immediate(v) {
        return 0;
    }
    let owned = match v {
        RubyValue::Str(s) => s.lock().bytesize() as i64,
        RubyValue::Array(a) => a.lock().len() as i64 * SLOT,
        RubyValue::Hash(h) => h.lock().len() as i64 * 2 * SLOT,
        RubyValue::BigInt(b) => (b.bits() as i64 + 7) / 8,
        RubyValue::Object(o) => o.ivar_pairs().len() as i64 * SLOT,
        _ => 0,
    };
    SLOT + owned
}

/// `ObjectSpace.reachable_objects_from(obj)` -- the objects `obj` refers to
/// DIRECTLY, led by its class, or nil for an immediate.
///
/// Matches CRuby element for element on the shapes a Ruby program can
/// actually see: an Array yields its elements, a Hash its keys and values in
/// insertion order, an object its instance-variable values, all with
/// immediates dropped. What it can't reproduce is CRuby's internal tier -- the
/// `T_ICLASS`/`T_IMEMO`/method-entry objects it lists for a Class or a Proc
/// have no zeo counterpart, so those answer with their class alone.
pub(crate) fn reachable_objects_from(v: &RubyValue) -> RubyValue {
    // An immediate isn't a heap object, so nothing is reachable "from" it.
    if is_immediate(v) {
        return RubyValue::Nil;
    }
    // A bignum is the one heap shape CRuby doesn't lead with its class (its
    // klass slot isn't a traversed reference), so it answers an empty list.
    if matches!(v, RubyValue::BigInt(_)) {
        return RubyValue::Array(array_new(Vec::new()));
    }

    let mut out = vec![RubyValue::Class(v.class_id())];
    match v {
        RubyValue::Array(a) => {
            for e in a.lock().iter() {
                push_ref(&mut out, e);
            }
        }
        RubyValue::Hash(h) => {
            for (k, val) in crate::hash_pairs(h) {
                push_ref(&mut out, &k);
                push_ref(&mut out, &val);
            }
        }
        RubyValue::Object(o) => {
            for (_, val) in o.ivar_pairs() {
                push_ref(&mut out, &val);
            }
        }
        RubyValue::Range(__rg) => {
            let (begin, end, _) = __rg.parts();
            for r in [begin, end].into_iter().flatten() {
                push_ref(&mut out, r);
            }
        }
        _ => {}
    }
    RubyValue::Array(array_new(out))
}

fn push_ref(out: &mut Vec<RubyValue>, r: &RubyValue) {
    if !is_immediate(r) {
        out.push(r.clone());
    }
}

/// An empty per-kind census -- `count_objects`' posture, and the shape every
/// `count_*` row answers with.
/// `ObjectSpace.each_object`, split by what zeo can answer COMPLETELY.
///
/// A partial enumeration presented as a whole one is the worst answer
/// available: a caller counting instances would silently get the wrong
/// number. So each argument form is either complete or refused.
///
/// * `Class` / `Module`: complete, and needs nothing armed. A class is not an
///   `Arc` a program allocates -- it is a row registered at startup, plus
///   whatever `Class.new` has minted since.
/// * A class whose instances the allocation registry records (an Object and
///   its subclasses, Array, Hash, Proc, Range): complete while `ZEO_GC=1` is
///   recording, refused otherwise.
/// * Anything else, and the no-argument form: refused. A String, a Symbol and
///   an Integer are never registered, so "every object" is not a set zeo can
///   produce.
fn each_object_of(
    arg: Option<&RubyValue>,
    block: Option<&RubyValue>,
) -> Result<RubyValue, crate::Signal> {
    let Some(RubyValue::Class(cid)) = arg else {
        return Err(not_impl_error!(
            "ObjectSpace.each_object needs a class argument: zeo can enumerate \
             every Class, every Module, and the object kinds the allocation \
             registry records, but not the whole heap"
        ));
    };
    let cid = *cid;
    let values: Vec<RubyValue> = if cid == zeo_abi::CLASS_CLASS || cid == zeo_abi::MODULE_CLASS {
        crate::dispatch::class_ids(cid == zeo_abi::MODULE_CLASS)
            .into_iter()
            .map(RubyValue::Class)
            .collect()
    } else {
        if !crate::gc::recording() {
            return Err(not_impl_error!(
                "ObjectSpace.each_object over instances needs the allocation \
                 registry, which only ZEO_GC=1 arms"
            ));
        }
        crate::gc::live_values()
            .into_iter()
            .filter(|v| crate::dispatch::is_a_value(v, cid))
            .collect()
    };
    let Some(RubyValue::Proc(block)) = block else {
        // CRuby answers an Enumerator; zeo has no enumerator over a walk it
        // cannot resume, so it answers the count -- which is what the
        // block form answers too.
        return Ok(RubyValue::Int(values.len() as i64));
    };
    let n = values.len() as i64;
    for v in values {
        block.call(&[v])?;
    }
    Ok(RubyValue::Int(n))
}

fn empty_census() -> RubyValue {
    RubyValue::Hash(crate::collections::hash_new(Vec::new()))
}

ruby_module! {
    ObjectSpace = zeo_abi::OBJECTSPACE_MODULE;

    // `define_finalizer(obj, callable)` or `define_finalizer(obj) { |id| }` --
    // best-effort: the callback runs when `obj` is seen collected (`GC.start`)
    // and unconditionally at program exit, receiving obj's id.
    module_function def "define_finalizer" cfunc (_recv, arg1, arg2?, &block) {
        let obj = arg1;
        let callback = match (arg2, block) {
            (Some(cb), _) => {
                if !crate::dispatch::responds_to(cb.class_id(), Symbol::intern("call"), false) {
                    return Err(arg_error!(
                        "wrong type argument {} (should be callable)",
                        crate::builtins::class_name_of(cb)
                    ));
                }
                cb.clone()
            }
            (None, Some(b)) => b,
            (None, None) => {
                return Err(arg_error!("tried to create Proc object without a block"));
            }
        };
        FINALIZERS.lock().push(Finalizer {
            target: WeakTarget::downgrade(obj),
            object_id: object_id_i64(obj),
            callback,
        });
        // CRuby returns `[0, callable]` (arity + the finalizer); the arity slot
        // is an internal detail callers don't read.
        Ok(RubyValue::Array(array_new(vec![RubyValue::Int(0), obj.clone()])))
    }
    // Remove every finalizer registered for `obj` (by identity). Returns obj.
    module_function def "undefine_finalizer"(_recv, arg) {
        let obj = arg;
        FINALIZERS.lock().retain(|f| match f.target.upgrade() {
            Some(t) => !same_object(&t, obj),
            None => true,
        });
        Ok(obj.clone())
    }
    // Every live object of a class. See [`each_object_of`] for which
    // arguments zeo can answer COMPLETELY and why the rest still refuse.
    module_function def "each_object"(_recv, *_args, &_block) {
        each_object_of(__args.first(), _block.as_ref())
    }
    // No id->object table exists under Arc refcounting.
    module_function def "_id2ref"(_recv, _object_id) {
        Err(not_impl_error!("ObjectSpace._id2ref is not available (zeo has no id-to-object table)"))
    }
    // `garbage_collect` IS `GC.start` in CRuby, and now here too: it runs the
    // cycle pass and the finalizer sweep through the same entry, so the two
    // spellings cannot drift.
    module_function def "garbage_collect" params "full_mark: true, immediate_mark: true, immediate_sweep: true"(_recv, *_args, &_block) {
        crate::builtins::gc::run_collection();
        Ok(RubyValue::Nil)
    }
    // An empty per-class census: no fabricated counts, matching `GC.stat`'s
    // empty-Hash posture.
    module_function def "count_objects"(_recv, *_args, &_block) {
        Ok(empty_census())
    }

    // -- ext/objspace's introspection half ------------------------------

    module_function def "memsize_of"(_recv, arg) {
        Ok(RubyValue::Int(memsize_of(arg)))
    }
    // The same walk as `each_object`, folded through the per-value size
    // `memsize_of` already computes. It needs the allocation registry for the
    // same reason and refuses for the same reason without it.
    module_function def "memsize_of_all"(_recv, *_args, &_block) {
        if !crate::gc::recording() {
            return Err(not_impl_error!(
                "ObjectSpace.memsize_of_all needs the allocation registry, which \
                 only ZEO_GC=1 arms"
            ));
        }
        let total: i64 = crate::gc::live_values().iter().map(memsize_of).sum();
        Ok(RubyValue::Int(total))
    }
    module_function def "reachable_objects_from"(_recv, arg) {
        Ok(reachable_objects_from(arg))
    }
    // The root set is CRuby's own VM state (machine stack, global table,
    // frame chain); zeo's roots are Rust locals a program can't enumerate.
    module_function def "reachable_objects_from_root"(_recv) {
        Err(not_impl_error!("ObjectSpace.reachable_objects_from_root is not available (zeo has no GC root table)"))
    }
    // Every zeo symbol is interned once and never freed, so CRuby's
    // mortal/dynamic/static split has no counterpart -- `immortal_symbol`
    // (its total) is the one key that can be answered truthfully.
    module_function def "count_symbols"(_recv, *_args, &_block) {
        let total = RubyValue::Int(Symbol::count() as i64);
        Ok(RubyValue::Hash(crate::collections::hash_new(vec![
            (RubyValue::Symbol(Symbol::intern("immortal_symbol")), total),
        ])))
    }
    // Truly empty rather than unanswerable: an AST node, a T_DATA wrapper and
    // an imemo are CRuby heap shapes with no zeo equivalent, so zero of each
    // is live by construction.
    module_function def "count_nodes"(_recv, *_args, &_block) {
        Ok(empty_census())
    }
    module_function def "count_tdata_objects"(_recv, *_args, &_block) {
        Ok(empty_census())
    }
    module_function def "count_imemo_objects"(_recv, *_args, &_block) {
        Ok(empty_census())
    }
    // `count_objects` by bytes -- empty for the same reason it is.
    module_function def "count_objects_size"(_recv, *_args, &_block) {
        Ok(empty_census())
    }
    // Allocation tracing needs a hook on every allocation site. zeo allocates
    // through Rust's own `Arc::new`, with no such seam, so the tracer says so
    // instead of quietly recording nothing.
    module_function def "trace_object_allocations"(_recv, &_block) {
        Err(not_impl_error!("ObjectSpace.trace_object_allocations is not available (zeo has no allocation hook)"))
    }
    module_function def "trace_object_allocations_start"(_recv) {
        Err(not_impl_error!("ObjectSpace.trace_object_allocations_start is not available (zeo has no allocation hook)"))
    }
    module_function def "trace_object_allocations_stop"(_recv) {
        Err(not_impl_error!("ObjectSpace.trace_object_allocations_stop is not available (zeo has no allocation hook)"))
    }
    module_function def "trace_object_allocations_clear"(_recv) {
        Err(not_impl_error!("ObjectSpace.trace_object_allocations_clear is not available (zeo has no allocation hook)"))
    }
    module_function def "trace_object_allocations_debug_start"(_recv) {
        Err(not_impl_error!("ObjectSpace.trace_object_allocations_debug_start is not available (zeo has no allocation hook)"))
    }
    // The allocation GETTERS answer nil, which is exactly what CRuby answers
    // for an object allocated outside a trace -- and since tracing can never
    // be on here, that is the whole truth rather than a stub. A program that
    // tries to turn tracing on hits the errors above first.
    module_function def "allocation_sourcefile"(_recv, _arg) {
        Ok(RubyValue::Nil)
    }
    module_function def "allocation_sourceline"(_recv, _arg) {
        Ok(RubyValue::Nil)
    }
    module_function def "allocation_class_path"(_recv, _arg) {
        Ok(RubyValue::Nil)
    }
    module_function def "allocation_method_id"(_recv, _arg) {
        Ok(RubyValue::Nil)
    }
    module_function def "allocation_generation"(_recv, _arg) {
        Ok(RubyValue::Nil)
    }
    // `dump`'s whole output is a serialization of CRuby's object header --
    // flags, shape id, embedded/chilled bits, slot size, write-barrier state.
    // None of that exists here, and a JSON line carrying only the handful of
    // fields zeo could fill would break every tool that reads dumps in a far
    // more confusing way than an error does.
    module_function def "dump"(_recv, _obj, **_opts) {
        Err(not_impl_error!("ObjectSpace.dump is not available (zeo objects carry no VM header to serialize)"))
    }
    module_function def "dump_all"(_recv, **_opts) {
        Err(not_impl_error!("ObjectSpace.dump_all is not available (zeo has no heap enumeration)"))
    }
    module_function def "dump_shapes"(_recv, **_opts) {
        Err(not_impl_error!("ObjectSpace.dump_shapes is not available (zeo has no shape tree)"))
    }
    // A singleton class, an iclass, a shape -- `internal_class_of` reports
    // whichever of those CRuby really dispatches through. zeo dispatches
    // through a `ClassId` and an overlay, so there is no internal answer to
    // give that `Object#class` doesn't already give honestly.
    module_function def "internal_class_of"(_recv, _arg) {
        Err(not_impl_error!("ObjectSpace.internal_class_of is not available (zeo has no internal classes)"))
    }
    module_function def "internal_super_of"(_recv, _arg) {
        Err(not_impl_error!("ObjectSpace.internal_super_of is not available (zeo has no internal classes)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::{hash_new, string_new};

    fn a_string(s: &str) -> RubyValue {
        RubyValue::Str(string_new(s.to_string()))
    }

    /// `ObjectSpace`'s `ruby_module!`-generated class methods are reachable only
    /// through the dispatch table (their Rust fn names are mangled), so the
    /// tests call them the way real dispatch does -- through the registered
    /// class-method `lookup`.
    fn os_cmethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::OBJECTSPACE_MODULE)
            .expect("ObjectSpace is a registered builtin table")
            .class
            .as_ref()
            .expect("ObjectSpace has class methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("ObjectSpace.{name} is defined"))
    }

    /// Call an ObjectSpace class method with one argument.
    fn call1(name: &str, arg: RubyValue) -> Result<RubyValue, crate::Signal> {
        os_cmethod(name)(&RubyValue::Nil, &[arg], None)
    }

    #[test]
    fn garbage_collect_is_a_nil_no_op_and_count_objects_is_empty() {
        let nil = RubyValue::Nil;
        assert!(matches!(
            os_cmethod("garbage_collect")(&nil, &[], None).unwrap(),
            RubyValue::Nil
        ));
        let RubyValue::Hash(h) = os_cmethod("count_objects")(&nil, &[], None).unwrap() else {
            panic!("count_objects is a Hash")
        };
        assert_eq!(h.lock().len(), 0);
    }

    #[test]
    fn heap_enumeration_methods_are_honest_not_implemented_errors() {
        for name in [
            "each_object",
            "_id2ref",
            "memsize_of_all",
            "reachable_objects_from_root",
            "trace_object_allocations_start",
            "dump",
            "dump_all",
            "internal_class_of",
        ] {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                call1(name, RubyValue::Int(0))
            }));
            // NotImplementedError raised; registry-less in a bare unit test, so
            // it either panics or returns Err.
            assert!(
                r.is_err() || r.unwrap().is_err(),
                "{name} should not succeed"
            );
        }
    }

    #[test]
    fn memsize_of_is_zero_for_an_immediate_and_grows_with_a_payload() {
        for immediate in [
            RubyValue::Nil,
            RubyValue::Bool(true),
            RubyValue::Int(7),
            RubyValue::Float(1.5),
            RubyValue::Symbol(Symbol::intern("s")),
        ] {
            assert_eq!(memsize_of(&immediate), 0, "{immediate:?} is immediate");
        }
        assert!(memsize_of(&a_string("hi")) > 0);
        assert!(memsize_of(&a_string(&"x".repeat(100))) > memsize_of(&a_string("hi")));

        let empty = RubyValue::Array(array_new(Vec::new()));
        let three = RubyValue::Array(array_new(vec![RubyValue::Int(1); 3]));
        assert!(memsize_of(&three) > memsize_of(&empty));
    }

    #[test]
    fn reachable_objects_from_leads_with_the_class_and_drops_immediates() {
        assert!(matches!(
            reachable_objects_from(&RubyValue::Int(1)),
            RubyValue::Nil
        ));

        let s = a_string("v");
        let arr = RubyValue::Array(array_new(vec![RubyValue::Int(1), s.clone()]));
        let RubyValue::Array(got) = reachable_objects_from(&arr) else {
            panic!("an Array's answer is an Array")
        };
        let got = got.lock().clone();
        assert_eq!(got.len(), 2, "the class plus the one non-immediate element");
        assert!(matches!(got[0], RubyValue::Class(c) if c == zeo_abi::ARRAY_CLASS));
        assert!(same_object(&got[1], &s));
    }

    #[test]
    fn reachable_objects_from_walks_a_hash_key_then_value() {
        let k = a_string("k");
        let v = a_string("v");
        let h = RubyValue::Hash(hash_new(vec![
            (k.clone(), v.clone()),
            (RubyValue::Int(2), RubyValue::Int(3)),
        ]));
        let RubyValue::Array(got) = reachable_objects_from(&h) else {
            panic!("a Hash's answer is an Array")
        };
        let got = got.lock().clone();
        assert_eq!(got.len(), 3, "the class plus one string pair");
        // The key is compared by CONTENT: a String key is duped and frozen on
        // insert (Ruby's rule), so what the Hash holds isn't `k` itself.
        assert!(got[1].rb_eq(&k), "the key comes before its value");
        assert!(same_object(&got[2], &v));
    }

    #[test]
    fn count_symbols_reports_the_interner_total() {
        let before = Symbol::count();
        Symbol::intern("a_symbol_no_other_test_interns");
        let RubyValue::Hash(h) = os_cmethod("count_symbols")(&RubyValue::Nil, &[], None).unwrap()
        else {
            panic!("count_symbols is a Hash")
        };
        let key = RubyValue::Symbol(Symbol::intern("immortal_symbol"));
        let RubyValue::Int(total) = crate::hash_get(&h, &key) else {
            panic!("immortal_symbol is an Integer")
        };
        assert!(total > before as i64, "the new symbol is counted");
    }

    #[test]
    fn define_and_undefine_finalizer_round_trip() {
        let obj = a_string("finalizable");
        // A trivial callable for the block slot.
        let block = RubyValue::Proc(crate::RProc::new(|_args: &[RubyValue]| Ok(RubyValue::Nil)));
        // A block finalizer registers; the return is CRuby's [0, obj] pair.
        let r = os_cmethod("define_finalizer")(
            &RubyValue::Nil,
            std::slice::from_ref(&obj),
            Some(block),
        );
        assert!(r.is_ok(), "define_finalizer with a block succeeds");
        // undefine_finalizer answers the object it was given.
        let back =
            os_cmethod("undefine_finalizer")(&RubyValue::Nil, std::slice::from_ref(&obj), None)
                .unwrap();
        assert!(same_object(&back, &obj));
    }

    #[test]
    fn the_allocation_getters_answer_nil_like_an_untraced_object() {
        for name in [
            "allocation_sourcefile",
            "allocation_sourceline",
            "allocation_class_path",
            "allocation_method_id",
            "allocation_generation",
        ] {
            assert!(
                matches!(call1(name, a_string("x")), Ok(RubyValue::Nil)),
                "{name} answers nil"
            );
        }
    }
}

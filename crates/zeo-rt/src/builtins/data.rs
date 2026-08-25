//! Native `Data` -- the immutable sibling of `Struct` (`Data.define(:a, :b)`).
//! Its runtime model (the `StructInstance` payload, per-class member metadata,
//! constructor, and class minting) is shared with `Struct` and lives in
//! `rstruct.rs`; this file carries only `Data`'s own frozen, reader-only
//! instance surface (no Enumerable, no `[]`/writers) and its `define` class
//! method. Both classes self-register via linkme, so `Data` inherits nothing
//! from `Struct` -- they are independent `Object` subclasses.

use crate::RubyValue;
use crate::array_new;
use crate::builtins::arg_error;
use crate::builtins::rstruct::{
    StructInstance, bind_members, build_inspect, build_members, deconstruct_keys,
    define_value_class, frozen_error, meta_of, slots_of, struct_equal, struct_to_h,
};
use crate::dispatch::send_value;
use crate::symbol::Symbol;
use zeo_abi::DATA_CLASS;
use zeo_macros::ruby_class;

ruby_class! {
    Data = zeo_abi::DATA_CLASS < zeo_abi::OBJECT_CLASS;

    // `Data.define(:a, :b)` MINTS an immutable value class at runtime.
    def self."define"(_recv, *args, &block) {
        define_value_class(DATA_CLASS, true, args, block)
    }

    // The freeze happens HERE, not after the constructor returns, because
    // that is where CRuby puts it (`rb_data_initialize_m`). A subclass whose
    // own `initialize` writes an ivar AFTER `super` must raise `FrozenError`,
    // and it cannot if the instance is still mutable until `Data.new` is done
    // with it.
    def "initialize"(recv, *args, &_block) {
        bind_members(recv, args, true)?;
        if let RubyValue::Object(o) = recv {
            o.set_frozen();
        }
        Ok(RubyValue::Nil)
    }
    // A CONSTRUCTED Data instance is frozen, so a direct send can only
    // raise -- but the copy hooks run on the pre-freeze copy (`Kernel#dup`/
    // `#clone`'s ordering), where this copies slots exactly like Struct's.
    private def "initialize_copy"(recv, other) {
        if recv.is_frozen() {
            return Err(frozen_error(recv));
        }
        let same_class = other.class_id() == recv.class_id() && meta_of(recv.class_id()).is_some();
        if !same_class {
            return Err(crate::builtins::type_error!(
                "initialize_copy should take same class object"
            ));
        }
        let values = slots_of(other);
        for (i, v) in values.into_iter().enumerate() {
            crate::builtins::rstruct::slot_set(recv, i, v);
        }
        Ok(recv.clone())
    }
    def "members"(recv) {
        build_members(recv)
    }
    def "to_h"(recv, &block) {
        struct_to_h(recv, block)
    }
    def "deconstruct"(recv) {
        Ok(RubyValue::Array(array_new(slots_of(recv))))
    }
    def "deconstruct_keys"(recv, arg) {
        deconstruct_keys(recv, arg)
    }
    // `d.with(x: 1)` -- a copy with the named members replaced. Changes arrive
    // as a trailing keyword hash (the kwargs convention).
    def "with"(recv, *args, &_block) {
        let cid = recv.class_id();
        let meta = meta_of(cid).expect("data instance has meta");
        let mut slots = slots_of(recv);
        if let Some(RubyValue::Hash(h)) = args.last() {
            for (k, v) in h.lock().values() {
                let RubyValue::Symbol(s) = k else {
                    return Err(arg_error!("unknown keyword"));
                };
                match meta.index_of(*s) {
                    Some(i) => slots[i] = v.clone(),
                    None => return Err(arg_error!("unknown keyword: :{}", s.name())),
                }
            }
        }
        let copy = StructInstance::new_robj(cid, slots);
        copy.set_frozen();
        Ok(RubyValue::Object(copy))
    }
    def "=="(recv, other) {
        Ok(RubyValue::Bool(struct_equal(recv, other)))
    }
    def "eql?"(recv, arg) {
        Ok(RubyValue::Bool(struct_equal(recv, arg)))
    }
    def "hash"(recv) {
        // Hash the slots ARRAY (structural), NOT a fresh `to_h` Hash -- a Hash
        // keys by object identity here, so equal Data would otherwise hash
        // apart, breaking their use as Hash keys and their `Array#==`/`uniq`.
        // Matches `struct`'s own value-based `hash` and `struct_equal`.
        let arr = RubyValue::Array(array_new(slots_of(recv)));
        send_value(&arr, Symbol::intern("hash"), &[], None)
    }
    def "inspect" | "to_s" (recv) {
        build_inspect(recv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Signal;

    // Each test runs in its own nextest process -- see the crate README for
    // the with_core() bootstrap pattern.
    fn install_core() {
        crate::dispatch::install_class_registry(crate::dispatch::ClassRegistry::with_core());
    }

    fn call(recv: &RubyValue, name: &str, args: &[RubyValue]) -> Result<RubyValue, Signal> {
        send_value(recv, Symbol::intern(name), args, None)
    }

    fn point_class() -> RubyValue {
        call(
            &RubyValue::Class(DATA_CLASS),
            "define",
            &[
                RubyValue::Symbol(Symbol::intern("x")),
                RubyValue::Symbol(Symbol::intern("y")),
            ],
        )
        .expect("Data.define succeeds")
    }

    #[test]
    fn define_mints_a_class_whose_instances_read_their_members() {
        install_core();
        let point = point_class();
        let p = call(&point, "new", &[RubyValue::Int(1), RubyValue::Int(2)]).unwrap();

        assert!(matches!(call(&p, "x", &[]), Ok(RubyValue::Int(1))));
        assert!(matches!(call(&p, "y", &[]), Ok(RubyValue::Int(2))));
        let Ok(RubyValue::Array(members)) = call(&p, "members", &[]) else {
            panic!("members answers an Array");
        };
        let members = members.lock().to_vec();
        assert!(matches!(&members[0], RubyValue::Symbol(s) if *s == Symbol::intern("x")));
        assert!(matches!(&members[1], RubyValue::Symbol(s) if *s == Symbol::intern("y")));
    }

    #[test]
    fn a_constructed_instance_is_frozen() {
        install_core();
        let point = point_class();
        let p = call(&point, "new", &[RubyValue::Int(1), RubyValue::Int(2)]).unwrap();
        assert!(p.is_frozen());
    }

    #[test]
    fn equal_slots_make_equal_values() {
        install_core();
        let point = point_class();
        let a = call(&point, "new", &[RubyValue::Int(1), RubyValue::Int(2)]).unwrap();
        let b = call(&point, "new", &[RubyValue::Int(1), RubyValue::Int(2)]).unwrap();
        let c = call(&point, "new", &[RubyValue::Int(9), RubyValue::Int(2)]).unwrap();

        assert!(matches!(
            call(&a, "==", std::slice::from_ref(&b)),
            Ok(RubyValue::Bool(true))
        ));
        assert!(matches!(
            call(&a, "==", std::slice::from_ref(&c)),
            Ok(RubyValue::Bool(false))
        ));
    }

    #[test]
    fn with_replaces_named_members_on_a_copy() {
        install_core();
        let point = point_class();
        let p = call(&point, "new", &[RubyValue::Int(1), RubyValue::Int(2)]).unwrap();
        let kw = RubyValue::Hash(crate::hash_new(vec![(
            RubyValue::Symbol(Symbol::intern("y")),
            RubyValue::Int(3),
        )]));

        let q = call(&p, "with", &[kw]).expect("with succeeds");
        assert!(matches!(call(&q, "x", &[]), Ok(RubyValue::Int(1))));
        assert!(matches!(call(&q, "y", &[]), Ok(RubyValue::Int(3))));
        // The original is untouched.
        assert!(matches!(call(&p, "y", &[]), Ok(RubyValue::Int(2))));
        // An unknown keyword is refused.
        let bad = RubyValue::Hash(crate::hash_new(vec![(
            RubyValue::Symbol(Symbol::intern("z")),
            RubyValue::Int(9),
        )]));
        assert!(call(&p, "with", &[bad]).is_err());
    }
}

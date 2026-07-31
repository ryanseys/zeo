//! Native `Data` -- the immutable sibling of `Struct` (`Data.define(:a, :b)`).
//! Its runtime model (the `StructInstance` payload, per-class member metadata,
//! constructor, and class minting) is shared with `Struct` and lives in
//! `rstruct.rs`; this file carries only `Data`'s own frozen, reader-only
//! instance surface (no Enumerable, no `[]`/writers) and its `define` class
//! method. Both classes self-register via linkme, so `Data` inherits nothing
//! from `Struct` -- they are independent `Object` subclasses.

use crate::RubyValue;
use crate::array_new;
use crate::builtins::rstruct::{
    StructInstance, bind_members, build_inspect, build_members, deconstruct_keys,
    define_value_class, meta_of, slots_of, struct_equal, struct_to_h,
};
use crate::builtins::arg_error;
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

    def "initialize"(recv, *args, &_block) {
        bind_members(recv, args, true)?;
        Ok(RubyValue::Nil)
    }
    def "members"(recv, *_args, &_block) {
        build_members(recv)
    }
    def "to_h"(recv, &block) {
        struct_to_h(recv, block)
    }
    def "deconstruct"(recv, *_args, &_block) {
        Ok(RubyValue::Array(array_new(slots_of(recv))))
    }
    def "deconstruct_keys"(recv, arg) {
        deconstruct_keys(recv, arg)
    }
    // `d.with(x: 1)` -- a copy with the named members replaced. Changes arrive
    // as a trailing keyword hash (the G2 convention).
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
    def "hash"(recv, *_args, &_block) {
        // Hash the slots ARRAY (structural), NOT a fresh `to_h` Hash -- a Hash
        // keys by object identity here, so equal Data would otherwise hash
        // apart, breaking their use as Hash keys and their `Array#==`/`uniq`.
        // Matches `struct`'s own value-based `hash` and `struct_equal`.
        let arr = RubyValue::Array(array_new(slots_of(recv)));
        send_value(&arr, Symbol::intern("hash"), &[], None)
    }
    def "inspect" | "to_s" (recv, *_args, &_block) {
        build_inspect(recv)
    }
}

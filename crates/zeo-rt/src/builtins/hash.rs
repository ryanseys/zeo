//! `Hash` (CRuby hash.c) -- stage B carries the rows migrated from the old
//! curated table; the Tier A breadth (merge/fetch/dig/...) lands in stage E.

use crate::RubyValue;
use crate::builtins::enumerable::{self, own_row};
use crate::builtins::{
    arg_error, block_or_enum, convert, frozen_error, inherited_row, recv_hash, type_error,
};
use zeo_macros::ruby_class;

/// CRuby's `rb_hash_modify` guard: a frozen Hash raises before any in-place
/// mutation. Shared by every mutator so a frozen receiver can't slip through.
fn guard_hash_frozen(recv: &RubyValue) -> Result<(), crate::Signal> {
    if recv_hash!(recv).is_frozen() {
        return Err(crate::dispatch::raise_error_details(
            "FrozenError",
            format!("can't modify frozen Hash: {}", recv.inspect_string()),
            &[("receiver", recv.clone())],
        ));
    }
    Ok(())
}

ruby_class! {
    Hash = zeo_abi::HASH_CLASS < zeo_abi::OBJECT_CLASS;
    receiver rhash = crate::RubyValue::Hash;
    include zeo_abi::ENUMERABLE_CLASS;

    // `Hash.new` / `Hash.new(default)` / `Hash.new { |hash, key| ... }`. The
    // default value and default block are mutually exclusive -- passing both
    // is an ArgumentError, matching CRuby.
    // `inherits`: ruby reaches `Hash.new` through `Class#new`, so it is not in
    // `Hash.singleton_methods(false)`. The row has to live here anyway -- it
    // is what a Hash receiver's allocator is -- so reflection looks past it.
    def self."new" allocs inherits (_recv, arg?, &block) {
        if let Some(RubyValue::Proc(_)) = &block {
            if !arg.is_none() {
                return Err(arg_error!("wrong number of arguments (given 1, expected 0)"));
            }
            return Ok(RubyValue::Hash(crate::hash_new_with_default(RubyValue::Nil, block)));
        }
        let default = arg.cloned().unwrap_or(RubyValue::Nil);
        Ok(RubyValue::Hash(crate::hash_new_with_default(default, None)))
    }
    // `Hash[]` class constructor -- distinct from the INSTANCE `Hash#[]` (key
    // lookup). Three shapes: a single Hash to copy, a single Array of `[k, v]`
    // pairs, or an even-length flat `k1, v1, k2, v2, ...` list.
    // `Hash.ruby2_keywords_hash(h)` flags a copy as the keyword-splat marker
    // `ruby2_keywords` uses. zeo resolves keyword forwarding at COMPILE time
    // and carries no such flag at run time, so the copy is a plain Hash and
    // the predicate is false for every one -- which is what CRuby answers for
    // any hash that was not marked.
    def self."ruby2_keywords_hash" (_recv, hash) {
        let RubyValue::Hash(h) = hash else {
            return Err(type_error!("wrong argument type {} (expected Hash)",
                crate::builtins::class_name_of(hash)));
        };
        Ok(RubyValue::Hash(crate::hash_new(h.lock().values().cloned().collect())))
    }
    def self."ruby2_keywords_hash?" (_recv, hash) {
        if !matches!(hash, RubyValue::Hash(_)) {
            return Err(type_error!("wrong argument type {} (expected Hash)",
                crate::builtins::class_name_of(hash)));
        }
        Ok(RubyValue::Bool(false))
    }
    def self."[]" allocs (_recv, *args, &_block) {
        if args.len() == 1 {
            match &args[0] {
                RubyValue::Hash(h) => {
                    let pairs = h.lock().values().map(|(k, v)| (k.clone(), v.clone())).collect();
                    return Ok(RubyValue::Hash(crate::hash_new(pairs)));
                }
                RubyValue::Array(a) => {
                    let mut pairs = Vec::new();
                    for el in a.lock().iter() {
                        let RubyValue::Array(kv) = el else {
                            return Err(arg_error!("wrong element type {} (expected array)",
                                    crate::builtins::class_name_of(el)));
                        };
                        let kv = kv.lock();
                        if kv.is_empty() || kv.len() > 2 {
                            return Err(arg_error!("invalid number of elements ({} for 1..2)", kv.len()));
                        }
                        pairs.push((kv[0].clone(), kv.get(1).cloned().unwrap_or(RubyValue::Nil)));
                    }
                    return Ok(RubyValue::Hash(crate::hash_new(pairs)));
                }
                _ => {}
            }
        }
        if !args.len().is_multiple_of(2) {
            return Err(arg_error!("odd number of arguments for Hash"));
        }
        let pairs = args.chunks(2).map(|c| (c[0].clone(), c[1].clone())).collect();
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }
    // `Hash.try_convert(obj)`: `obj` if it's already a Hash, its `to_hash` if
    // it defines one (which must yield a Hash or nil), else nil. Answers the
    // object it was handed, so a `class H < Hash` stays an `H`.
    def self."try_convert" (_recv, arg) {
        Ok(convert::try_convert_value(arg, "Hash", "to_hash")?.unwrap_or(RubyValue::Nil))
    }

    def "[]" (recv, arg) {
        crate::hash_index(rhash, arg)
    }
    // The per-instance default set by `Hash.new(default)` / `Hash.new { }`.
    // `#default(key)` optionally runs a default proc for `key`, matching CRuby.
    def "default"(recv, arg?) {
        let h = rhash;
        let (default, proc) = {
            let g = h.lock();
            (g.default.clone(), g.default_proc.clone())
        };
        match (proc, arg) {
            (Some(p), Some(key)) => crate::dispatch::send_value(
                &p,
                crate::Symbol::intern("call"),
                &[recv.clone(), key.clone()],
                None,
            ),
            _ => Ok(default),
        }
    }
    def "default=" (recv, arg) {
        guard_hash_frozen(recv)?;
        let mut g = rhash.lock();
        g.default = (*arg).clone();
        g.default_proc = None;
        Ok((*arg).clone())
    }
    def "default_proc" (recv) {
        Ok(rhash.lock().default_proc.clone().unwrap_or(RubyValue::Nil))
    }
    def "default_proc=" (recv, arg) {
        let mut g = rhash.lock();
        match arg {
            RubyValue::Nil => g.default_proc = None,
            p @ RubyValue::Proc(_) => g.default_proc = Some(p.clone()),
            // CRuby probes `to_proc` and, failing that (or a lying answer),
            // raises its own shape: "wrong default_proc type X (expected
            // Proc)" -- NOT the generic implicit-conversion TypeError.
            other => {
                let to_proc = crate::Symbol::intern("to_proc");
                let ducked = if crate::dispatch::responds_to_value(other, to_proc, true) {
                    Some(crate::dispatch::send_value(other, to_proc, &[], None)?)
                } else {
                    None
                };
                match ducked {
                    Some(p @ RubyValue::Proc(_)) => g.default_proc = Some(p),
                    _ => {
                        return Err(type_error!(
                            "wrong default_proc type {} (expected Proc)",
                            crate::builtins::class_name_of(other)
                        ))
                    }
                }
            }
        }
        Ok((*arg).clone())
    }
    // Switch to identity keying (`equal?`/`object_id` instead of `eql?`/`hash`);
    // re-projects existing entries so keys stay reachable by their own object.
    def "compare_by_identity" (recv) {
        let h = rhash;
        if h.is_frozen() {
            return Err(frozen_error!("can't modify frozen Hash: {}", recv.inspect_string()));
        }
        crate::hash_enable_compare_by_identity(h);
        Ok(recv.clone())
    }
    def "compare_by_identity?" (recv) {
        Ok(RubyValue::Bool(rhash.lock().compare_by_identity))
    }
    def "[]=" | "store" (recv, arg1, arg2) {
        let h = rhash;
        // CRuby's `rb_hash_aset` checks modifiability first, so `h[k] = v` (and
        // the `h[k] += v` opassign desugaring) on a frozen Hash raises.
        if h.is_frozen() {
            return Err(crate::dispatch::raise_error_details(
                "FrozenError",
                format!("can't modify frozen Hash: {}", recv.inspect_string()),
                &[("receiver", recv.clone())],
            ));
        }
        crate::collections::hash_set_checked(h, (*arg1).clone(), (*arg2).clone())
    }
    def "delete" (recv, arg, &block) {
        let h = rhash;
        if h.is_frozen() {
            return Err(crate::dispatch::raise_error_details(
                "FrozenError",
                format!("can't modify frozen Hash: {}", recv.inspect_string()),
                &[("receiver", recv.clone())],
            ));
        }
        // A block supplies the return value when the key is ABSENT
        // (`h.delete(:z) { |k| ... }`), instead of the default nil.
        if !crate::hash_has_key(h, arg)
            && let Some(RubyValue::Proc(p)) = block {
                return p.call(&[(*arg).clone()]);
            }
        Ok(crate::hash_delete(h, arg))
    }
    def "key?" | "has_key?" | "include?" | "member?" (recv, arg) {
        Ok(RubyValue::Bool(crate::hash_has_key(rhash, arg)))
    }
    def "keys" (recv) {
        Ok(crate::hash_keys(rhash))
    }
    def "values" (recv) {
        Ok(crate::hash_values(rhash))
    }
    def "length" | "size" (recv) {
        Ok(RubyValue::Int(crate::hash_len(rhash)))
    }
    def "empty?" (recv) {
        Ok(RubyValue::Bool(crate::hash_len(rhash) == 0))
    }
    def "==" (recv, other) {
        Ok(RubyValue::Bool(recv.rb_eq(other)))
    }
    def "fetch" cfunc (recv, arg1, arg2?, &block) {
        // One probe, not `has_key?` then `[]` -- see `hash_lookup`.
        if let Some(v) = crate::collections::hash_lookup(rhash, arg1) {
            return Ok(v);
        }
        if let Some(default) = arg2 {
            return Ok(default.clone());
        }
        if let Some(RubyValue::Proc(p)) = &block {
            return p.call(std::slice::from_ref(arg1));
        }
        Err(crate::dispatch::raise_error_details(
            "KeyError",
            format!("key not found: {}", (*arg1).inspect_string()),
            &[("key", (*arg1).clone()), ("receiver", recv.clone())],
        ))
    }
    def "dig" cfunc (recv, _key, *_rest, &_block) {
        let args = __args;
        let cur = crate::hash_get(rhash, &args[0]);
        if args.len() == 1 {
            return Ok(cur);
        }
        // Remaining keys recurse through the intermediate's OWN `dig`; a
        // non-diggable there raises TypeError, matching CRuby's `rb_obj_dig`.
        crate::dispatch::obj_dig(cur, &args[1..])
    }
    // `merge` (fresh hash) with an optional conflict block;
    // `merge!`/`update` write into the receiver.
    def "merge"(recv, *args, &block) {
        let base = rhash;
        let fresh = crate::hash_new(base.lock().values().cloned().collect());
        crate::collections::copy_hash_meta(base, &fresh); // inherit the receiver's default
        let out = RubyValue::Hash(fresh);
        merge_into(&out, args, &block)?;
        Ok(out)
    }
    def "merge!" | "update"(recv, *args, &block) {
        guard_hash_frozen(recv)?;
        merge_into(recv, args, &block)?;
        Ok(recv.clone())
    }
    def "to_a" (recv) {
        let out = rhash
            .lock()
            .values()
            .map(|(k, v)| RubyValue::Array(crate::array_new(vec![k.clone(), v.clone()])))
            .collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // `slice(*keys)` / `except(*keys)`: a new Hash keeping (resp. dropping)
    // the named keys, preserving the receiver's insertion order.
    def "slice"(recv, *args, &_block) {
        let src = rhash;
        let pairs = src
            .lock()
            .values()
            .filter(|(k, _)| args.iter().any(|a| a.rb_eq(k)))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }
    def "except"(recv, *args, &_block) {
        let src = rhash;
        let pairs = src
            .lock()
            .values()
            .filter(|(k, _)| !args.iter().any(|a| a.rb_eq(k)))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }
    // `fetch_values(*keys)`: the values for `keys` in order. A missing key
    // yields the block's value if a block is given, else raises KeyError --
    // exactly `fetch`'s rule applied to each key.
    def "fetch_values"(recv, *args, &block) {
        let src = rhash;
        let mut out = Vec::with_capacity(args.len());
        for key in args {
            if crate::hash_has_key(src, key) {
                out.push(crate::hash_get(src, key));
            } else if let Some(RubyValue::Proc(p)) = &block {
                out.push(p.call(std::slice::from_ref(key))?);
            } else {
                return Err(crate::dispatch::raise_error_details(
                    "KeyError",
                    format!("key not found: {}", key.inspect_string()),
                    &[("key", key.clone()), ("receiver", recv.clone())],
                ));
            }
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // `flatten(depth = 1)`: the `[k, v, ...]` pairs concatenated, then
    // flattened `depth` more levels (so `flatten(2)` also splays array
    // values). Depth 0 leaves the pairs nested.
    def "flatten"(recv, depth?) {
        // `Hash#flatten(depth)` == `to_a.flatten(depth)`: depth 0 keeps the
        // `[k, v]` pairs intact, 1 (the argless default) splays one level,
        // and a negative depth flattens fully.
        let depth = match depth {
            None => 1,
            Some(v) => crate::builtins::arg_int!(v),
        };
        let pairs: Vec<RubyValue> = rhash
            .lock()
            .values()
            .map(|(k, v)| RubyValue::Array(crate::array_new(vec![k.clone(), v.clone()])))
            .collect();
        let out = crate::builtins::array::flatten_to_depth(&pairs, depth)?;
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // `compact` drops nil-valued entries into a new Hash; `compact!` does it
    // in place, answering nil when there was nothing to drop.
    def "compact" (recv) {
        let pairs = rhash
            .lock()
            .values()
            .filter(|(_, v)| !matches!(v, RubyValue::Nil))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }
    def "compact!" (recv) {
        // CRuby's modify check runs before the nothing-to-do nil answer.
        guard_hash_frozen(recv)?;
        let h = rhash;
        let nil_keys: Vec<RubyValue> = h
            .lock()
            .values()
            .filter(|(_, v)| matches!(v, RubyValue::Nil))
            .map(|(k, _)| k.clone())
            .collect();
        if nil_keys.is_empty() {
            return Ok(RubyValue::Nil);
        }
        for k in &nil_keys {
            crate::hash_delete(h, k);
        }
        Ok(recv.clone())
    }
    // `values_at(*keys)`: the values for `keys` in order (the hash's default
    // for a missing key, `nil` by default).
    def "values_at"(recv, *args, &_block) {
        // Each key goes through `[]`, so a missing key yields the hash's
        // DEFAULT (`Hash.new(0).values_at(:x) == [0]`), not a bare nil.
        let h = rhash;
        let mut out = Vec::with_capacity(args.len());
        for k in args {
            out.push(crate::hash_index(h, k)?);
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // `assoc(key)` / `rassoc(value)`: the `[key, value]` pair matched by key
    // (resp. value), or nil.
    def "assoc" (recv, arg) {
        for (k, v) in rhash.lock().values() {
            if k.rb_eq(arg) {
                return Ok(RubyValue::Array(crate::array_new(vec![k.clone(), v.clone()])));
            }
        }
        Ok(RubyValue::Nil)
    }
    def "rassoc" (recv, arg) {
        for (k, v) in rhash.lock().values() {
            if v.rb_eq(arg) {
                return Ok(RubyValue::Array(crate::array_new(vec![k.clone(), v.clone()])));
            }
        }
        Ok(RubyValue::Nil)
    }
    // `shift`: removes and returns the first `[key, value]` pair (insertion
    // order), or nil on an empty hash.
    def "shift" (recv) {
        guard_hash_frozen(recv)?;
        let h = rhash;
        let first = h.lock().values().next().map(|(k, v)| (k.clone(), v.clone()));
        match first {
            Some((k, v)) => {
                crate::hash_delete(h, &k);
                Ok(RubyValue::Array(crate::array_new(vec![k, v])))
            }
            None => Ok(RubyValue::Nil),
        }
    }
    // `deconstruct_keys(keys)`: a Hash pattern matches against the hash
    // itself, so this just answers the receiver (the `keys` hint is ignored).
    def "deconstruct_keys" (recv, _arg) {
        Ok(recv.clone())
    }
    // `replace(other)`: swaps this hash's contents for `other`'s, answering
    // the receiver.
    def "replace" (recv, arg) {
        guard_hash_frozen(recv)?;
        let other = &convert::to_rhash(arg)?;
        let h = rhash;
        let old_keys: Vec<RubyValue> = h.lock().values().map(|(k, _)| k.clone()).collect();
        for k in &old_keys {
            crate::hash_delete(h, k);
        }
        for (k, v) in other.lock().values() {
            crate::hash_set(h, k.clone(), v.clone());
        }
        Ok(recv.clone())
    }
    // ruby's `rb_hash_initialize`: sets the DEFAULT channel and nothing
    // else -- an already-filled receiver keeps its entries (oracle-pinned:
    // `{a: 1}.send(:initialize, 9)` stays `{a: 1}` with default 9).
    private def "initialize"(recv, ifnone?, &block) {
        guard_hash_frozen(recv)?;
        if let Some(RubyValue::Proc(_)) = &block {
            if ifnone.is_some() {
                return Err(arg_error!("wrong number of arguments (given 1, expected 0)"));
            }
            let mut g = rhash.lock();
            g.default = RubyValue::Nil;
            g.default_proc = block;
        } else {
            let mut g = rhash.lock();
            g.default = ifnone.cloned().unwrap_or(RubyValue::Nil);
            g.default_proc = None;
        }
        Ok(recv.clone())
    }
    // `initialize_copy` is the FULL replace -- entries plus both default
    // channels (`rb_hash_replace` backs it).
    private def "initialize_copy"(recv, arg) {
        guard_hash_frozen(recv)?;
        let other = &convert::to_rhash(arg)?;
        if std::sync::Arc::ptr_eq(rhash, other) {
            return Ok(recv.clone());
        }
        let old_keys: Vec<RubyValue> = rhash.lock().values().map(|(k, _)| k.clone()).collect();
        for k in &old_keys {
            crate::hash_delete(rhash, k);
        }
        for (k, v) in other.lock().values() {
            crate::hash_set(rhash, k.clone(), v.clone());
        }
        let (d, dp) = {
            let s = other.lock();
            (s.default.clone(), s.default_proc.clone())
        };
        let mut g = rhash.lock();
        g.default = d;
        g.default_proc = dp;
        Ok(recv.clone())
    }
    // Subset/superset by key AND value: `a <= b` iff every pair of `a` is in
    // `b`; `<` additionally requires `a` to be strictly smaller. `>`/`>=` are
    // the mirror.
    def "<=" (recv, other) {
        Ok(RubyValue::Bool(hash_subset(recv, other, false)?))
    }
    def "<" (recv, other) {
        Ok(RubyValue::Bool(hash_subset(recv, other, true)?))
    }
    def ">=" (recv, other) {
        Ok(RubyValue::Bool(hash_subset(other, recv, false)?))
    }
    def ">" (recv, other) {
        Ok(RubyValue::Bool(hash_subset(other, recv, true)?))
    }
    // In-place filters. `select!`/`filter!`/`keep_if` keep the entries the
    // block accepts; `reject!`/`delete_if` drop them. The `!`-suffixed forms
    // answer nil when nothing changed; `keep_if`/`delete_if` always answer
    // the receiver.
    def "select!" | "filter!" (recv, &block) {
        guard_hash_frozen(recv)?;
        hash_filter_bang(recv, &[], block, true, true)
    }
    def "keep_if" (recv, &block) {
        guard_hash_frozen(recv)?;
        hash_filter_bang(recv, &[], block, true, false)
    }
    def "reject!" (recv, &block) {
        guard_hash_frozen(recv)?;
        hash_filter_bang(recv, &[], block, false, true)
    }
    def "delete_if" (recv, &block) {
        guard_hash_frozen(recv)?;
        hash_filter_bang(recv, &[], block, false, false)
    }
    // `transform_values!` rewrites each value in place through the block,
    // keeping keys and order; answers the receiver.
    def "transform_values!" (recv, &block) {
        let p = block_or_enum!(recv, &[], block);
        // After the enumerator return -- CRuby's own order (frozen raises
        // only once a block makes this a real mutation).
        guard_hash_frozen(recv)?;
        let h = rhash;
        let pairs: Vec<(RubyValue, RubyValue)> =
            h.lock().values().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (k, v) in pairs {
            let nv = p.call(&[v])?;
            crate::hash_set(h, k, nv);
        }
        Ok(recv.clone())
    }
    // Blockless `to_h` on a Hash is identity; with a block each entry is
    // re-mapped, the block seeing the two RAW yielded values (`{ |k, v| }`).
    // `to_hash` is the implicit-conversion protocol and never takes a block.
    def "to_h" | "to_hash" (recv, &block) {
        let Some(blk) = block else {
            return Ok(recv.clone());
        };
        // Each entry yields TWO raw values (`{ |k, v| }`), so `raw` is the
        // pair itself and the packed element is the same `[k, v]` Array.
        let raws: Vec<Vec<RubyValue>> = rhash
            .lock()
            .values()
            .map(|(k, v)| vec![k.clone(), v.clone()])
            .collect();
        let packed: Vec<RubyValue> = raws
            .iter()
            .map(|kv| RubyValue::Array(crate::array_new(kv.clone())))
            .collect();
        let pairs = crate::builtins::enumerable::to_h_pairs(
            raws.iter().map(|kv| kv.as_slice()).zip(packed.iter()),
            &Some(blk),
        )?;
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }
    def "invert" (recv) {
        let pairs = rhash
            .lock()
            .values()
            .map(|(k, v)| (v.clone(), k.clone()))
            .collect();
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }
    def "key" (recv, arg) {
        for (k, v) in rhash.lock().values() {
            if v.rb_eq(arg) {
                return Ok(k.clone());
            }
        }
        Ok(RubyValue::Nil)
    }
    def "value?" | "has_value?" (recv, arg) {
        let found = rhash
            .lock()
            .values()
            .any(|(_, v)| v.rb_eq(arg));
        Ok(RubyValue::Bool(found))
    }
    def "each_key" (recv, &block) {
        let p = block_or_enum!(recv, &[], block);
        let keys: Vec<RubyValue> =
            rhash.lock().values().map(|(k, _)| k.clone()).collect();
        for k in keys {
            p.call(&[k])?;
        }
        Ok(recv.clone())
    }
    def "each_value" (recv, &block) {
        let p = block_or_enum!(recv, &[], block);
        let vals: Vec<RubyValue> =
            rhash.lock().values().map(|(_, v)| v.clone()).collect();
        for v in vals {
            p.call(&[v])?;
        }
        Ok(recv.clone())
    }
    // Hash-returning select/reject (Enumerable's array-returning forms
    // are shadowed by these, real Ruby's rule).
    def "select" | "filter" (recv, &block) {
        hash_filter(recv, &[], block, true)
    }
    def "reject" (recv, &block) {
        hash_filter(recv, &[], block, false)
    }
    def "transform_values" (recv, &block) {
        let p = block_or_enum!(recv, &[], block);
        let pairs = crate::collections::hash_pairs_snapshot(rhash);
        let mut out = Vec::with_capacity(pairs.len());
        for (k, v) in pairs {
            out.push((k, p.call(&[v])?));
        }
        Ok(RubyValue::Hash(crate::hash_new(out)))
    }
    // `transform_keys([mapping]) { |k| }` -- an optional mapping Hash renames
    // the keys it lists (block/identity handles the rest), then the block maps
    // any remaining keys; with neither, an Enumerator.
    def "transform_keys"(recv, mapping?, &block) {
        let mapping = transform_keys_mapping(mapping)?;
        let blk = match &block {
            Some(RubyValue::Proc(p)) => Some(p.clone()),
            _ => None,
        };
        if mapping.is_none() && blk.is_none() {
            return Ok(crate::builtins::enumerator::enumerator_for(recv, "transform_keys", __args));
        }
        let pairs = crate::collections::hash_pairs_snapshot(rhash);
        let mut out = Vec::with_capacity(pairs.len());
        for (k, v) in pairs {
            out.push((map_transform_key(&mapping, &blk, k)?, v));
        }
        Ok(RubyValue::Hash(crate::hash_new(out)))
    }
    // In-place `transform_keys` -- rebuilds the hash with each key mapped
    // through the mapping/block, keeping insertion order and answering the
    // receiver.
    def "transform_keys!"(recv, mapping?, &block) {
        let mapping = transform_keys_mapping(mapping)?;
        let blk = match &block {
            Some(RubyValue::Proc(p)) => Some(p.clone()),
            _ => None,
        };
        if mapping.is_none() && blk.is_none() {
            return Ok(crate::builtins::enumerator::enumerator_for(recv, "transform_keys!", __args));
        }
        guard_hash_frozen(recv)?;
        let h = rhash;
        let pairs: Vec<(RubyValue, RubyValue)> = h.lock().values().cloned().collect();
        let mut out = Vec::with_capacity(pairs.len());
        for (k, v) in pairs {
            out.push((map_transform_key(&mapping, &blk, k)?, v));
        }
        h.lock().clear();
        for (k, v) in out {
            crate::hash_set(h, k, v);
        }
        Ok(recv.clone())
    }
    // `to_proc` yields a lambda that looks a key up in this hash (`h.to_proc`
    // is `->(k) { h[k] }`); the hash is captured by identity, so later
    // mutations are visible through the proc.
    def "to_proc"(recv) {
        let h = rhash.clone();
        let p = crate::RProc::with_meta(
            move |args: &[RubyValue]| {
                let key = args.first().cloned().unwrap_or(RubyValue::Nil);
                crate::hash_index(&h, &key)
            },
            1,
            true,
        );
        Ok(RubyValue::Proc(p))
    }
    // `rehash` re-derives every key's digest after an in-place key mutation.
    // The map IS keyed by that digest, computed when the pair went in, so a
    // key mutated afterwards is filed under a digest that no longer describes
    // it -- `h[k]` misses even though `h.keys` shows `k`. (This was a no-op,
    // on the belief that lookups digest afresh. They do not.)
    //
    // Digests are computed BEFORE the lock: projecting a key can dispatch a
    // user `hash`, and the payload lock is not reentrant.
    def "rehash" (recv) {
        guard_hash_frozen(recv)?;
        let pairs = crate::collections::hash_pairs_snapshot(rhash);
        rhash.lock().clear();
        for (k, v) in pairs {
            crate::hash_set(rhash, k, v);
        }
        Ok(recv.clone())
    }
    def "clear" (recv) {
        guard_hash_frozen(recv)?;
        rhash.lock().clear();
        Ok(recv.clone())
    }
    def "each" | "each_pair" (recv, &block) {
        let p = block_or_enum!(recv, &[], block);
        // A block-raised exception shows a 'Hash#each' C-frame between the
        // block and the caller in CRuby's backtrace.
        let _frame = crate::frames::synthetic_c_frame("Hash#each");
        let _iter = crate::collections::hash_iter_guard(rhash);
        let pairs = crate::collections::hash_pairs_snapshot(rhash);
        for (k, v) in pairs {
            // Two values for a block that can take two, one packed array
            // otherwise -- `rb_hash_foreach`'s own rule. See `yield_pair`.
            crate::rproc::yield_pair(&p, k, v)?;
        }
        Ok(recv.clone())
    }

    // ---- rows ruby OWNS on this class while the body lives on an ancestor.
    // Each calls the very row it would otherwise have inherited, so `.owner`
    // and `instance_methods(false)` agree and there is still only one body.
    def "any?" cfunc (recv, *_args, &block) { own_row!(recv, |s| enumerable::any_all(s, __args, block, enumerable::Quantifier::Any)) }
    def "eql?"(recv, _other) { inherited_row!(kernel, "eql?", recv, __args, None) }
    def "freeze"(recv) { inherited_row!(kernel, "freeze", recv, __args, None) }
    def "hash"(recv) { inherited_row!(kernel, "hash", recv, __args, None) }
    def "inspect"(recv) { inherited_row!(kernel, "inspect", recv, __args, None) }
    // NOT an alias of `#inspect`: `Complex`, `Rational` and `Regexp` all
    // spell the two differently, so each goes to its own Kernel row.
    def "to_s"(recv) { inherited_row!(kernel, "to_s", recv, __args, None) }
}

/// `transform_keys`'s optional first argument: a mapping Hash (`nil`/absent is
/// none), else CRuby's `no implicit conversion into Hash` TypeError.
fn transform_keys_mapping(
    mapping: Option<&RubyValue>,
) -> Result<Option<crate::collections::RHash>, crate::Signal> {
    match mapping {
        None => Ok(None),
        // Strict: an explicit nil raises too (CRuby's rb_to_hash_type).
        Some(v) => Ok(Some(convert::to_rhash(v)?)),
    }
}

/// One key's new name under `transform_keys`: a listed mapping key wins, then
/// the block, then the key unchanged.
fn map_transform_key(
    mapping: &Option<crate::collections::RHash>,
    blk: &Option<crate::RProc>,
    k: RubyValue,
) -> Result<RubyValue, crate::Signal> {
    if let Some(m) = mapping
        && crate::hash_has_key(m, &k)
    {
        return Ok(crate::hash_get(m, &k));
    }
    if let Some(p) = blk {
        return p.call(std::slice::from_ref(&k));
    }
    Ok(k)
}

/// `merge`/`merge!`'s shared writer: later hashes win, unless the conflict
/// block chooses (`old`/`new` order is real Ruby's).
/// The in-place block filters. `keep == true` keeps the entries the block
/// accepts (`select!`/`keep_if`), else drops them (`reject!`/`delete_if`).
/// When `nil_if_unchanged`, answers nil if nothing was removed (the `!`
/// forms); otherwise always the receiver (`keep_if`/`delete_if`).
fn hash_filter_bang(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
    keep: bool,
    nil_if_unchanged: bool,
) -> Result<RubyValue, crate::Signal> {
    let p = block_or_enum!(recv, if keep { "select!" } else { "reject!" }, args, block);
    let RubyValue::Hash(h) = recv else {
        unreachable!("Hash table row dispatched on a non-Hash receiver");
    };
    // A frozen receiver raises before any block runs -- CRuby's
    // `rb_hash_modify_check` at the top of every in-place filter, whether or
    // not an entry would actually be removed.
    if h.is_frozen() {
        return Err(frozen_error!(
            "can't modify frozen Hash: {}",
            recv.inspect_string()
        ));
    }
    let pairs: Vec<(RubyValue, RubyValue)> = h
        .lock()
        .values()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut removed = 0;
    for (k, v) in pairs {
        let accepted = p.call(&[k.clone(), v])?.truthy();
        if accepted != keep {
            crate::hash_delete(h, &k);
            removed += 1;
        }
    }
    if nil_if_unchanged && removed == 0 {
        return Ok(RubyValue::Nil);
    }
    Ok(recv.clone())
}

/// `a <= b` (and, with `proper`, `a < b`): every pair of `a` appears in `b`
/// with an equal value. A non-Hash `b` is a TypeError, matching CRuby.
fn hash_subset(a: &RubyValue, b: &RubyValue, proper: bool) -> Result<bool, crate::Signal> {
    // One side is always the (Hash) receiver; the other converts through the
    // `to_hash` protocol (`{} > 1` names Integer -- oracle-verified).
    let small = &convert::to_rhash(a)?;
    let big = &convert::to_rhash(b)?;
    if proper && crate::hash_len(small) >= crate::hash_len(big) {
        return Ok(false);
    }
    let contained = small
        .lock()
        .values()
        .all(|(k, v)| crate::hash_has_key(big, k) && crate::hash_get(big, k).rb_eq(v));
    Ok(contained)
}

fn merge_into(
    target: &RubyValue,
    args: &[RubyValue],
    block: &Option<RubyValue>,
) -> Result<(), crate::Signal> {
    let RubyValue::Hash(target) = target else {
        unreachable!("Hash table row dispatched on a non-Hash receiver");
    };
    for a in args {
        let other = &convert::to_rhash(a)?;
        let pairs: Vec<(RubyValue, RubyValue)> = other.lock().values().cloned().collect();
        for (k, v) in pairs {
            let value = match block {
                Some(RubyValue::Proc(p)) if crate::hash_has_key(target, &k) => {
                    let old = crate::hash_get(target, &k);
                    p.call(&[k.clone(), old, v])?
                }
                _ => v,
            };
            crate::hash_set(target, k, value);
        }
    }
    Ok(())
}

/// Hash-returning select/reject core.
fn hash_filter(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
    keep: bool,
) -> Result<RubyValue, crate::Signal> {
    let RubyValue::Hash(handle) = recv else {
        unreachable!("Hash table row dispatched on a non-Hash receiver");
    };
    let p = block_or_enum!(recv, if keep { "select" } else { "reject" }, args, block);
    let pairs: Vec<(RubyValue, RubyValue)> = handle.lock().values().cloned().collect();
    let mut out = Vec::new();
    for (k, v) in pairs {
        if p.call(&[k.clone(), v.clone()])?.truthy() == keep {
            out.push((k, v));
        }
    }
    Ok(RubyValue::Hash(crate::hash_new(out)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        (crate::builtins::registered_table(zeo_abi::HASH_CLASS)
            .unwrap()
            .class
            .as_ref()
            .unwrap()
            .lookup)(name)
        .unwrap()
    }
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        (crate::builtins::registered_table(zeo_abi::HASH_CLASS)
            .unwrap()
            .instance
            .as_ref()
            .unwrap()
            .lookup)(name)
        .unwrap()
    }

    #[test]
    fn bracket_class_constructor_shapes() {
        // Flat k, v, k, v list.
        let flat = cmethod("[]")(
            &RubyValue::Nil,
            &[
                RubyValue::Int(1),
                RubyValue::Int(2),
                RubyValue::Int(3),
                RubyValue::Int(4),
            ],
            None,
        )
        .unwrap();
        let RubyValue::Hash(h) = flat else { panic!() };
        assert_eq!(h.lock().len(), 2);

        // Single Array of [k, v] pairs.
        let pairs = RubyValue::Array(crate::array_new(vec![RubyValue::Array(crate::array_new(
            vec![RubyValue::Int(9), RubyValue::Int(8)],
        ))]));
        let from_pairs = cmethod("[]")(&RubyValue::Nil, &[pairs], None).unwrap();
        let RubyValue::Hash(h2) = from_pairs else {
            panic!()
        };
        assert_eq!(h2.lock().len(), 1);

        // Empty.
        assert!(matches!(
            cmethod("[]")(&RubyValue::Nil, &[], None).unwrap(),
            RubyValue::Hash(_)
        ));
    }

    #[test]
    #[should_panic(expected = "ArgumentError")]
    fn bracket_odd_flat_arg_count_is_argument_error() {
        // Registry-less, `raise_error` panics -- the message is the assertion.
        let _ = cmethod("[]")(&RubyValue::Nil, &[RubyValue::Int(1)], None);
    }

    #[test]
    fn store_alias_sets_and_keys_reports() {
        let h = RubyValue::Hash(crate::hash_new(Vec::new()));
        let k = RubyValue::Symbol(crate::Symbol::intern("a"));
        imethod("[]=")(&h, &[k.clone(), RubyValue::Int(1)], None).unwrap();
        let r = imethod("key?")(&h, &[k], None).unwrap();
        assert!(matches!(r, RubyValue::Bool(true)));
        let RubyValue::Array(ks) = imethod("keys")(&h, &[], None).unwrap() else {
            panic!()
        };
        assert_eq!(ks.lock().len(), 1);
    }
}

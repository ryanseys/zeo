//! `Hash` (CRuby hash.c) -- stage B carries the rows migrated from the old
//! curated table; the Tier A breadth (merge/fetch/dig/...) lands in stage E.

use crate::RubyValue;
use crate::builtins::{
    arg_error, arity, block_or_enum, builtin_methods, convert, frozen_error, recv_hash, type_error,
};

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

builtin_methods! {
    pub(crate) fn lookup;

    "[]"[1] => fn index(recv, args, _block) {
        arity!(args, 1);
        crate::hash_index(recv_hash!(recv), &args[0])
    }
    // The per-instance default set by `Hash.new(default)` / `Hash.new { }`.
    // `#default(key)` optionally runs a default proc for `key`, matching CRuby.
    "default" => fn default_m(recv, args, _block) {
        arity!(args, 0..=1);
        let h = recv_hash!(recv);
        let (default, proc) = {
            let g = h.lock();
            (g.default.clone(), g.default_proc.clone())
        };
        match (proc, args.first()) {
            (Some(p), Some(key)) => crate::dispatch::send_value(
                &p,
                crate::Symbol::intern("call"),
                &[recv.clone(), key.clone()],
                None,
            ),
            _ => Ok(default),
        }
    }
    "default="[1] => fn default_set(recv, args, _block) {
        arity!(args, 1);
        guard_hash_frozen(recv)?;
        let mut g = recv_hash!(recv).lock();
        g.default = args[0].clone();
        g.default_proc = None;
        Ok(args[0].clone())
    }
    "default_proc"[0] => fn default_proc_m(recv, args, _block) {
        arity!(args, 0);
        Ok(recv_hash!(recv).lock().default_proc.clone().unwrap_or(RubyValue::Nil))
    }
    "default_proc="[1] => fn default_proc_set(recv, args, _block) {
        arity!(args, 1);
        let mut g = recv_hash!(recv).lock();
        match &args[0] {
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
        Ok(args[0].clone())
    }
    // Switch to identity keying (`equal?`/`object_id` instead of `eql?`/`hash`);
    // re-projects existing entries so keys stay reachable by their own object.
    "compare_by_identity"[0] => fn compare_by_identity(recv, args, _block) {
        arity!(args, 0);
        let h = recv_hash!(recv);
        if h.is_frozen() {
            return Err(frozen_error!("can't modify frozen Hash: {}", recv.inspect_string()));
        }
        crate::hash_enable_compare_by_identity(h);
        Ok(recv.clone())
    }
    "compare_by_identity?"[0] => fn compare_by_identity_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv_hash!(recv).lock().compare_by_identity))
    }
    "[]="[2] | "store"[2] => fn index_set(recv, args, _block) {
        arity!(args, 2);
        let h = recv_hash!(recv);
        // CRuby's `rb_hash_aset` checks modifiability first, so `h[k] = v` (and
        // the `h[k] += v` opassign desugaring) on a frozen Hash raises.
        if h.is_frozen() {
            return Err(crate::dispatch::raise_error_details(
                "FrozenError",
                format!("can't modify frozen Hash: {}", recv.inspect_string()),
                &[("receiver", recv.clone())],
            ));
        }
        Ok(crate::hash_set(h, args[0].clone(), args[1].clone()))
    }
    "delete"[1] => fn delete(recv, args, block) {
        arity!(args, 1);
        let h = recv_hash!(recv);
        if h.is_frozen() {
            return Err(crate::dispatch::raise_error_details(
                "FrozenError",
                format!("can't modify frozen Hash: {}", recv.inspect_string()),
                &[("receiver", recv.clone())],
            ));
        }
        // A block supplies the return value when the key is ABSENT
        // (`h.delete(:z) { |k| ... }`), instead of the default nil.
        if !crate::hash_has_key(h, &args[0]) {
            if let Some(RubyValue::Proc(p)) = block {
                return p.call(&[args[0].clone()]);
            }
        }
        Ok(crate::hash_delete(h, &args[0]))
    }
    "key?"[1] | "has_key?"[1] | "include?"[1] | "member?"[1] => fn key_p(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(crate::hash_has_key(recv_hash!(recv), &args[0])))
    }
    "keys"[0] => fn keys(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::hash_keys(recv_hash!(recv)))
    }
    "values"[0] => fn values(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::hash_values(recv_hash!(recv)))
    }
    "length"[0] | "size"[0] => fn length(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(crate::hash_len(recv_hash!(recv))))
    }
    "empty?"[0] => fn empty_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(crate::hash_len(recv_hash!(recv)) == 0))
    }
    "=="[1] => fn eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.rb_eq(&args[0])))
    }
    "fetch" => fn fetch(recv, args, block) {
        arity!(args, 1..=2);
        if crate::hash_has_key(recv_hash!(recv), &args[0]) {
            return Ok(crate::hash_get(recv_hash!(recv), &args[0]));
        }
        if let Some(default) = args.get(1) {
            return Ok(default.clone());
        }
        if let Some(RubyValue::Proc(p)) = &block {
            return p.call(std::slice::from_ref(&args[0]));
        }
        Err(crate::dispatch::raise_error_details(
            "KeyError",
            format!("key not found: {}", args[0].inspect_string()),
            &[("key", args[0].clone()), ("receiver", recv.clone())],
        ))
    }
    "dig" => fn dig(recv, args, _block) {
        if args.is_empty() {
            return Err(arg_error!("wrong number of arguments (given 0, expected 1+)"));
        }
        let cur = crate::hash_get(recv_hash!(recv), &args[0]);
        if args.len() == 1 {
            return Ok(cur);
        }
        // Remaining keys recurse through the intermediate's OWN `dig`; a
        // non-diggable there raises TypeError, matching CRuby's `rb_obj_dig`.
        crate::dispatch::obj_dig(cur, &args[1..])
    }
    // `merge` (fresh hash) with an optional conflict block;
    // `merge!`/`update` write into the receiver.
    "merge" => fn merge(recv, args, block) {
        let out = RubyValue::Hash(crate::hash_new(
            recv_hash!(recv).lock().values().cloned().collect(),
        ));
        merge_into(&out, args, &block)?;
        Ok(out)
    }
    "merge!" | "update" => fn merge_bang(recv, args, block) {
        guard_hash_frozen(recv)?;
        merge_into(recv, args, &block)?;
        Ok(recv.clone())
    }
    "to_a"[0] => fn to_a(recv, args, _block) {
        arity!(args, 0);
        let out = recv_hash!(recv)
            .lock()
            .values()
            .map(|(k, v)| RubyValue::Array(crate::array_new(vec![k.clone(), v.clone()])))
            .collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // `slice(*keys)` / `except(*keys)`: a new Hash keeping (resp. dropping)
    // the named keys, preserving the receiver's insertion order.
    "slice" => fn slice(recv, args, _block) {
        let src = recv_hash!(recv);
        let pairs = src
            .lock()
            .values()
            .filter(|(k, _)| args.iter().any(|a| a.rb_eq(k)))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }
    "except" => fn except(recv, args, _block) {
        let src = recv_hash!(recv);
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
    "fetch_values" => fn fetch_values(recv, args, block) {
        let src = recv_hash!(recv);
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
    "flatten" => fn flatten(recv, args, _block) {
        arity!(args, 0..=1);
        // `Hash#flatten(depth)` == `to_a.flatten(depth)`: depth 0 keeps the
        // `[k, v]` pairs intact, 1 (the argless default) splays one level,
        // and a negative depth flattens fully.
        let depth = match args.first() {
            None => 1,
            Some(_) => crate::builtins::arg_int!(args, 0),
        };
        let pairs: Vec<RubyValue> = recv_hash!(recv)
            .lock()
            .values()
            .map(|(k, v)| RubyValue::Array(crate::array_new(vec![k.clone(), v.clone()])))
            .collect();
        let out = crate::builtins::array::flatten_to_depth(&pairs, depth);
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // `compact` drops nil-valued entries into a new Hash; `compact!` does it
    // in place, answering nil when there was nothing to drop.
    "compact"[0] => fn compact(recv, args, _block) {
        arity!(args, 0);
        let pairs = recv_hash!(recv)
            .lock()
            .values()
            .filter(|(_, v)| !matches!(v, RubyValue::Nil))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }
    "compact!"[0] => fn compact_bang(recv, args, _block) {
        arity!(args, 0);
        // CRuby's modify check runs before the nothing-to-do nil answer.
        guard_hash_frozen(recv)?;
        let h = recv_hash!(recv);
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
    "values_at" => fn values_at(recv, args, _block) {
        // Each key goes through `[]`, so a missing key yields the hash's
        // DEFAULT (`Hash.new(0).values_at(:x) == [0]`), not a bare nil.
        let h = recv_hash!(recv);
        let mut out = Vec::with_capacity(args.len());
        for k in args {
            out.push(crate::hash_index(h, k)?);
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // `assoc(key)` / `rassoc(value)`: the `[key, value]` pair matched by key
    // (resp. value), or nil.
    "assoc"[1] => fn assoc(recv, args, _block) {
        arity!(args, 1);
        for (k, v) in recv_hash!(recv).lock().values() {
            if k.rb_eq(&args[0]) {
                return Ok(RubyValue::Array(crate::array_new(vec![k.clone(), v.clone()])));
            }
        }
        Ok(RubyValue::Nil)
    }
    "rassoc"[1] => fn rassoc(recv, args, _block) {
        arity!(args, 1);
        for (k, v) in recv_hash!(recv).lock().values() {
            if v.rb_eq(&args[0]) {
                return Ok(RubyValue::Array(crate::array_new(vec![k.clone(), v.clone()])));
            }
        }
        Ok(RubyValue::Nil)
    }
    // `shift`: removes and returns the first `[key, value]` pair (insertion
    // order), or nil on an empty hash.
    "shift"[0] => fn shift(recv, args, _block) {
        arity!(args, 0);
        guard_hash_frozen(recv)?;
        let h = recv_hash!(recv);
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
    "deconstruct_keys"[1] => fn deconstruct_keys(recv, args, _block) {
        arity!(args, 1);
        Ok(recv.clone())
    }
    // `replace(other)`: swaps this hash's contents for `other`'s, answering
    // the receiver.
    "replace"[1] => fn replace(recv, args, _block) {
        guard_hash_frozen(recv)?;
        arity!(args, 1);
        let other = &convert::to_rhash(&args[0])?;
        let h = recv_hash!(recv);
        let old_keys: Vec<RubyValue> = h.lock().values().map(|(k, _)| k.clone()).collect();
        for k in &old_keys {
            crate::hash_delete(h, k);
        }
        for (k, v) in other.lock().values() {
            crate::hash_set(h, k.clone(), v.clone());
        }
        Ok(recv.clone())
    }
    // Subset/superset by key AND value: `a <= b` iff every pair of `a` is in
    // `b`; `<` additionally requires `a` to be strictly smaller. `>`/`>=` are
    // the mirror.
    "<="[1] => fn subset_eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(hash_subset(recv, &args[0], false)?))
    }
    "<"[1] => fn subset(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(hash_subset(recv, &args[0], true)?))
    }
    ">="[1] => fn superset_eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(hash_subset(&args[0], recv, false)?))
    }
    ">"[1] => fn superset(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(hash_subset(&args[0], recv, true)?))
    }
    // In-place filters. `select!`/`filter!`/`keep_if` keep the entries the
    // block accepts; `reject!`/`delete_if` drop them. The `!`-suffixed forms
    // answer nil when nothing changed; `keep_if`/`delete_if` always answer
    // the receiver.
    "select!"[0] | "filter!"[0] => fn select_bang(recv, args, block) {
        guard_hash_frozen(recv)?;
        arity!(args, 0);
        hash_filter_bang(recv, args, block, true, true)
    }
    "keep_if"[0] => fn keep_if(recv, args, block) {
        guard_hash_frozen(recv)?;
        arity!(args, 0);
        hash_filter_bang(recv, args, block, true, false)
    }
    "reject!"[0] => fn reject_bang(recv, args, block) {
        guard_hash_frozen(recv)?;
        arity!(args, 0);
        hash_filter_bang(recv, args, block, false, true)
    }
    "delete_if"[0] => fn delete_if(recv, args, block) {
        guard_hash_frozen(recv)?;
        arity!(args, 0);
        hash_filter_bang(recv, args, block, false, false)
    }
    // `transform_values!` rewrites each value in place through the block,
    // keeping keys and order; answers the receiver.
    "transform_values!"[0] => fn transform_values_bang(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "transform_values!", args, block);
        // After the enumerator return -- CRuby's own order (frozen raises
        // only once a block makes this a real mutation).
        guard_hash_frozen(recv)?;
        let h = recv_hash!(recv);
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
    "to_h"[0] | "to_hash"[0] => fn to_h(recv, args, block) {
        arity!(args, 0);
        let Some(blk) = block else {
            return Ok(recv.clone());
        };
        // Each entry yields TWO raw values (`{ |k, v| }`), so `raw` is the
        // pair itself and the packed element is the same `[k, v]` Array.
        let raws: Vec<Vec<RubyValue>> = recv_hash!(recv)
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
    "invert"[0] => fn invert(recv, args, _block) {
        arity!(args, 0);
        let pairs = recv_hash!(recv)
            .lock()
            .values()
            .map(|(k, v)| (v.clone(), k.clone()))
            .collect();
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }
    "key"[1] => fn key(recv, args, _block) {
        arity!(args, 1);
        for (k, v) in recv_hash!(recv).lock().values() {
            if v.rb_eq(&args[0]) {
                return Ok(k.clone());
            }
        }
        Ok(RubyValue::Nil)
    }
    "value?"[1] | "has_value?"[1] => fn value_p(recv, args, _block) {
        arity!(args, 1);
        let found = recv_hash!(recv)
            .lock()
            .values()
            .any(|(_, v)| v.rb_eq(&args[0]));
        Ok(RubyValue::Bool(found))
    }
    "each_key"[0] => fn each_key(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "each_key", args, block);
        let keys: Vec<RubyValue> =
            recv_hash!(recv).lock().values().map(|(k, _)| k.clone()).collect();
        for k in keys {
            p.call(&[k])?;
        }
        Ok(recv.clone())
    }
    "each_value"[0] => fn each_value(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "each_value", args, block);
        let vals: Vec<RubyValue> =
            recv_hash!(recv).lock().values().map(|(_, v)| v.clone()).collect();
        for v in vals {
            p.call(&[v])?;
        }
        Ok(recv.clone())
    }
    // Hash-returning select/reject (Enumerable's array-returning forms
    // are shadowed by these, real Ruby's rule).
    "select"[0] | "filter"[0] => fn select(recv, args, block) {
        arity!(args, 0);
        hash_filter(recv, args, block, true)
    }
    "reject"[0] => fn reject(recv, args, block) {
        arity!(args, 0);
        hash_filter(recv, args, block, false)
    }
    "transform_values"[0] => fn transform_values(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "transform_values", args, block);
        let pairs: Vec<(RubyValue, RubyValue)> =
            recv_hash!(recv).lock().values().cloned().collect();
        let mut out = Vec::with_capacity(pairs.len());
        for (k, v) in pairs {
            out.push((k, p.call(&[v])?));
        }
        Ok(RubyValue::Hash(crate::hash_new(out)))
    }
    // `transform_keys([mapping]) { |k| }` -- an optional mapping Hash renames
    // the keys it lists (block/identity handles the rest), then the block maps
    // any remaining keys; with neither, an Enumerator.
    "transform_keys" => fn transform_keys(recv, args, block) {
        arity!(args, 0..=1);
        let mapping = transform_keys_mapping(args)?;
        let blk = match &block {
            Some(RubyValue::Proc(p)) => Some(p.clone()),
            _ => None,
        };
        if mapping.is_none() && blk.is_none() {
            return Ok(crate::builtins::enumerator::enumerator_for(recv, "transform_keys", args));
        }
        let pairs: Vec<(RubyValue, RubyValue)> =
            recv_hash!(recv).lock().values().cloned().collect();
        let mut out = Vec::with_capacity(pairs.len());
        for (k, v) in pairs {
            out.push((map_transform_key(&mapping, &blk, k)?, v));
        }
        Ok(RubyValue::Hash(crate::hash_new(out)))
    }
    // In-place `transform_keys` -- rebuilds the hash with each key mapped
    // through the mapping/block, keeping insertion order and answering the
    // receiver.
    "transform_keys!" => fn transform_keys_bang(recv, args, block) {
        arity!(args, 0..=1);
        let mapping = transform_keys_mapping(args)?;
        let blk = match &block {
            Some(RubyValue::Proc(p)) => Some(p.clone()),
            _ => None,
        };
        if mapping.is_none() && blk.is_none() {
            return Ok(crate::builtins::enumerator::enumerator_for(recv, "transform_keys!", args));
        }
        guard_hash_frozen(recv)?;
        let h = recv_hash!(recv);
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
    "to_proc"[0] => fn to_proc(recv, args, _block) {
        arity!(args, 0);
        let h = recv_hash!(recv).clone();
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
    // `rehash` recomputes key digests after in-place key mutation. Zeo
    // hashes digest each key on lookup, so nothing is cached to rebuild --
    // it is a self-returning no-op here.
    "rehash"[0] => fn rehash(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    "clear"[0] => fn clear(recv, args, _block) {
        arity!(args, 0);
        guard_hash_frozen(recv)?;
        recv_hash!(recv).lock().clear();
        Ok(recv.clone())
    }
    "each"[0] | "each_pair"[0] => fn each(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "each", args, block);
        let pairs: Vec<(RubyValue, RubyValue)> =
            recv_hash!(recv).lock().values().cloned().collect();
        for (k, v) in pairs {
            // CRuby yields the pair as ONE array, so `{ |pair| }` and a
            // forwarded 1-arg callable (`&method(:m)`) get it whole while
            // `{ |k, v| }` auto-splats it.
            crate::rproc::yield_tuple(&p, vec![k, v])?;
        }
        Ok(recv.clone())
    }
}

/// `transform_keys`'s optional first argument: a mapping Hash (`nil`/absent is
/// none), else CRuby's `no implicit conversion into Hash` TypeError.
fn transform_keys_mapping(
    args: &[RubyValue],
) -> Result<Option<crate::collections::RHash>, crate::Signal> {
    match args.first() {
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
    if let Some(m) = mapping {
        if crate::hash_has_key(m, &k) {
            return Ok(crate::hash_get(m, &k));
        }
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

builtin_methods! {
    pub(crate) fn lookup_class;

    // `Hash.new` / `Hash.new(default)` / `Hash.new { |hash, key| ... }`. The
    // default value and default block are mutually exclusive -- passing both
    // is an ArgumentError, matching CRuby.
    "new" => fn hash_new_m(_recv, args, block) {
        arity!(args, 0..=1);
        if let Some(RubyValue::Proc(_)) = &block {
            if !args.is_empty() {
                return Err(arg_error!("wrong number of arguments (given 1, expected 0)"));
            }
            return Ok(RubyValue::Hash(crate::hash_new_with_default(RubyValue::Nil, block)));
        }
        let default = args.first().cloned().unwrap_or(RubyValue::Nil);
        Ok(RubyValue::Hash(crate::hash_new_with_default(default, None)))
    }
    // `Hash[]` class constructor -- distinct from the INSTANCE `Hash#[]` (key
    // lookup). Three shapes: a single Hash to copy, a single Array of `[k, v]`
    // pairs, or an even-length flat `k1, v1, k2, v2, ...` list.
    "[]"[1] => fn hash_bracket(_recv, args, _block) {
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
    // it defines one (which must yield a Hash or nil), else nil.
    "try_convert" => fn try_convert(_recv, args, _block) {
        arity!(args, 1);
        let v = &args[0];
        if matches!(v, RubyValue::Hash(_)) {
            return Ok(v.clone());
        }
        let to_hash = crate::Symbol::intern("to_hash");
        if crate::dispatch::responds_to(v.class_id(), to_hash, false) {
            return match crate::dispatch::send_value(v, to_hash, &[], None)? {
                r @ (RubyValue::Hash(_) | RubyValue::Nil) => Ok(r),
                other => Err(type_error!("can't convert {} to Hash ({}#to_hash gives {})",
                        crate::builtins::class_name_of(v),
                        crate::builtins::class_name_of(v),
                        crate::builtins::class_name_of(&other))),
            };
        }
        Ok(RubyValue::Nil)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bracket_class_constructor_shapes() {
        // Flat k, v, k, v list.
        let flat = hash_bracket(
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
        let from_pairs = hash_bracket(&RubyValue::Nil, &[pairs], None).unwrap();
        let RubyValue::Hash(h2) = from_pairs else {
            panic!()
        };
        assert_eq!(h2.lock().len(), 1);

        // Empty.
        assert!(matches!(
            hash_bracket(&RubyValue::Nil, &[], None).unwrap(),
            RubyValue::Hash(_)
        ));
    }

    #[test]
    #[should_panic(expected = "ArgumentError")]
    fn bracket_odd_flat_arg_count_is_argument_error() {
        // Registry-less, `raise_error` panics -- the message is the assertion.
        let _ = hash_bracket(&RubyValue::Nil, &[RubyValue::Int(1)], None);
    }

    #[test]
    fn store_alias_sets_and_keys_reports() {
        let h = RubyValue::Hash(crate::hash_new(Vec::new()));
        let k = RubyValue::Symbol(crate::Symbol::intern("a"));
        index_set(&h, &[k.clone(), RubyValue::Int(1)], None).unwrap();
        let r = key_p(&h, &[k], None).unwrap();
        assert!(matches!(r, RubyValue::Bool(true)));
        let RubyValue::Array(ks) = keys(&h, &[], None).unwrap() else {
            panic!()
        };
        assert_eq!(ks.lock().len(), 1);
    }
}

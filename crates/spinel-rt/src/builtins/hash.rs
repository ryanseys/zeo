//! `Hash` (CRuby hash.c) -- stage B carries the rows migrated from the old
//! curated table; the Tier A breadth (merge/fetch/dig/...) lands in stage E.

use crate::builtins::{arity, block_or_enum, builtin_methods, recv_hash};
use crate::RubyValue;

builtin_methods! {
    pub(crate) fn lookup;

    "[]" => fn index(recv, args, _block) {
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
    "default=" => fn default_set(recv, args, _block) {
        arity!(args, 1);
        let mut g = recv_hash!(recv).lock();
        g.default = args[0].clone();
        g.default_proc = None;
        Ok(args[0].clone())
    }
    "default_proc" => fn default_proc_m(recv, args, _block) {
        arity!(args, 0);
        Ok(recv_hash!(recv).lock().default_proc.clone().unwrap_or(RubyValue::Nil))
    }
    "default_proc=" => fn default_proc_set(recv, args, _block) {
        arity!(args, 1);
        let mut g = recv_hash!(recv).lock();
        match &args[0] {
            RubyValue::Nil => g.default_proc = None,
            p @ RubyValue::Proc(_) => g.default_proc = Some(p.clone()),
            other => return Err(crate::dispatch::raise_error(
                "TypeError",
                format!("no implicit conversion of {} into Proc", crate::builtins::class_name_of(other)),
            )),
        }
        Ok(args[0].clone())
    }
    "[]=" | "store" => fn index_set(recv, args, _block) {
        arity!(args, 2);
        Ok(crate::hash_set(recv_hash!(recv), args[0].clone(), args[1].clone()))
    }
    "delete" => fn delete(recv, args, _block) {
        arity!(args, 1);
        Ok(crate::hash_delete(recv_hash!(recv), &args[0]))
    }
    "key?" | "has_key?" | "include?" | "member?" => fn key_p(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(crate::hash_has_key(recv_hash!(recv), &args[0])))
    }
    "keys" => fn keys(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::hash_keys(recv_hash!(recv)))
    }
    "values" => fn values(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::hash_values(recv_hash!(recv)))
    }
    "length" | "size" => fn length(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(crate::hash_len(recv_hash!(recv))))
    }
    "empty?" => fn empty_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(crate::hash_len(recv_hash!(recv)) == 0))
    }
    "==" => fn eq(recv, args, _block) {
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
        Err(crate::dispatch::raise_error(
            "KeyError",
            format!("key not found: {}", args[0].inspect_string()),
        ))
    }
    "dig" => fn dig(recv, args, _block) {
        if args.is_empty() {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                "wrong number of arguments (given 0, expected 1+)".to_string(),
            ));
        }
        let mut cur = crate::hash_get(recv_hash!(recv), &args[0]);
        for key in &args[1..] {
            if cur.is_nil() {
                return Ok(RubyValue::Nil);
            }
            cur = crate::dispatch::send_value(
                &cur,
                crate::Symbol::intern("[]"),
                std::slice::from_ref(key),
                None,
            )?;
        }
        Ok(cur)
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
        merge_into(recv, args, &block)?;
        Ok(recv.clone())
    }
    "to_a" => fn to_a(recv, args, _block) {
        arity!(args, 0);
        let out = recv_hash!(recv)
            .lock()
            .values()
            .map(|(k, v)| RubyValue::Array(crate::array_new(vec![k.clone(), v.clone()])))
            .collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // Blockless `to_h` on a Hash is identity; with a block each entry is
    // re-mapped, the block seeing the two RAW yielded values (`{ |k, v| }`).
    // `to_hash` is the implicit-conversion protocol and never takes a block.
    "to_h" | "to_hash" => fn to_h(recv, args, block) {
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
    "invert" => fn invert(recv, args, _block) {
        arity!(args, 0);
        let pairs = recv_hash!(recv)
            .lock()
            .values()
            .map(|(k, v)| (v.clone(), k.clone()))
            .collect();
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }
    "key" => fn key(recv, args, _block) {
        arity!(args, 1);
        for (k, v) in recv_hash!(recv).lock().values() {
            if v.rb_eq(&args[0]) {
                return Ok(k.clone());
            }
        }
        Ok(RubyValue::Nil)
    }
    "value?" | "has_value?" => fn value_p(recv, args, _block) {
        arity!(args, 1);
        let found = recv_hash!(recv)
            .lock()
            .values()
            .any(|(_, v)| v.rb_eq(&args[0]));
        Ok(RubyValue::Bool(found))
    }
    "each_key" => fn each_key(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "each_key", args, block);
        let keys: Vec<RubyValue> =
            recv_hash!(recv).lock().values().map(|(k, _)| k.clone()).collect();
        for k in keys {
            p.call(&[k])?;
        }
        Ok(recv.clone())
    }
    "each_value" => fn each_value(recv, args, block) {
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
    "select" | "filter" => fn select(recv, args, block) {
        arity!(args, 0);
        hash_filter(recv, args, block, true)
    }
    "reject" => fn reject(recv, args, block) {
        arity!(args, 0);
        hash_filter(recv, args, block, false)
    }
    "transform_values" => fn transform_values(recv, args, block) {
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
    "transform_keys" => fn transform_keys(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "transform_keys", args, block);
        let pairs: Vec<(RubyValue, RubyValue)> =
            recv_hash!(recv).lock().values().cloned().collect();
        let mut out = Vec::with_capacity(pairs.len());
        for (k, v) in pairs {
            out.push((p.call(&[k])?, v));
        }
        Ok(RubyValue::Hash(crate::hash_new(out)))
    }
    "clear" => fn clear(recv, args, _block) {
        arity!(args, 0);
        recv_hash!(recv).lock().clear();
        Ok(recv.clone())
    }
    "each" | "each_pair" => fn each(recv, args, block) {
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


/// `merge`/`merge!`'s shared writer: later hashes win, unless the conflict
/// block chooses (`old`/`new` order is real Ruby's).
fn merge_into(
    target: &RubyValue,
    args: &[RubyValue],
    block: &Option<RubyValue>,
) -> Result<(), crate::Signal> {
    let RubyValue::Hash(target) = target else {
        unreachable!("Hash table row dispatched on a non-Hash receiver");
    };
    for a in args {
        let RubyValue::Hash(other) = a else {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Hash",
                    crate::builtins::class_name_of(a)
                ),
            ));
        };
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
                return Err(crate::dispatch::raise_error(
                    "ArgumentError",
                    "wrong number of arguments (given 1, expected 0)".to_string(),
                ));
            }
            return Ok(RubyValue::Hash(crate::hash_new_with_default(RubyValue::Nil, block)));
        }
        let default = args.first().cloned().unwrap_or(RubyValue::Nil);
        Ok(RubyValue::Hash(crate::hash_new_with_default(default, None)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_alias_sets_and_keys_reports() {
        let h = RubyValue::Hash(crate::hash_new(Vec::new()));
        let k = RubyValue::Symbol(crate::Symbol::intern("a"));
        index_set(&h, &[k.clone(), RubyValue::Int(1)], None).unwrap();
        let r = key_p(&h, &[k], None).unwrap();
        assert!(matches!(r, RubyValue::Bool(true)));
        let RubyValue::Array(ks) = keys(&h, &[], None).unwrap() else { panic!() };
        assert_eq!(ks.lock().len(), 1);
    }
}

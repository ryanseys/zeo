//! Keyword binding for DYNAMIC calls (the G2 trailing-kwargs-hash
//! convention): `bind_dynamic_kwargs`, the `**nil` reject, and the shared
//! missing/unknown-keyword error shape.

use super::*;

/// [`bind_dynamic_kwargs`]'s result: `(positional, required_values,
/// optional_values, rest_pairs)`.
pub type BoundKwargs<'a> = (
    &'a [RubyValue],
    Vec<RubyValue>,
    Vec<Option<RubyValue>>,
    Vec<(RubyValue, RubyValue)>,
);

/// Binds a DYNAMIC call's keyword arguments for a keyword-declaring callee
/// (the G2 trailing-kwargs-hash convention): when the last argument is a
/// Hash, it's the keyword set; otherwise there are no keywords. Returns
/// `(positional, required_values, optional_values, rest_pairs)` for the
/// generated trampoline to splice into the direct call -- optional `None`s
/// let the callee's own prologue lazily evaluate defaults, exactly like
/// Path 1.
///
/// Documented approximation (plan G2): no `ruby2_keywords` flagging, so a
/// bare trailing Hash passed positionally through `send` to a
/// keyword-declaring method binds as keywords.
pub fn bind_dynamic_kwargs<'a>(
    method: &str,
    args: &'a [RubyValue],
    required: &[&str],
    optional: &[&str],
    has_kwrest: bool,
) -> Result<BoundKwargs<'a>, Signal> {
    // Only a MARKED trailing hash is keywords. An unmarked one arrived
    // positionally -- a plain `f(9, h)`, or a hash a splat expanded, which
    // ruby makes positional again (see `unmark_kwargs_tail`).
    let (positional, kw_hash) = match args.split_last() {
        Some((RubyValue::Hash(h), rest)) if crate::collections::hash_is_kwargs(h) => {
            (rest, Some(h.clone()))
        }
        _ => (args, None),
    };
    let mut req_values = Vec::with_capacity(required.len());
    let mut opt_values = Vec::with_capacity(optional.len());
    let mut rest_pairs = Vec::new();

    let pairs: Vec<(RubyValue, RubyValue)> = match &kw_hash {
        Some(h) => h.lock().values().cloned().collect(),
        None => Vec::new(),
    };
    let lookup = |name: &str| -> Option<RubyValue> {
        pairs.iter().find_map(|(k, v)| match k {
            RubyValue::Symbol(s) if s.name_str() == name => Some(v.clone()),
            _ => None,
        })
    };
    let mut missing = Vec::new();
    for name in required {
        match lookup(name) {
            Some(v) => req_values.push(v),
            None => missing.push(name.to_string()),
        }
    }
    if !missing.is_empty() {
        return Err(kw_names_error("missing", &missing));
    }
    for name in optional {
        opt_values.push(lookup(name));
    }
    let mut unknown = Vec::new();
    for (k, v) in &pairs {
        if let RubyValue::Symbol(s) = k {
            let n = s.name_str();
            if required.contains(&n) || optional.contains(&n) {
                continue;
            }
            if has_kwrest {
                rest_pairs.push((k.clone(), v.clone()));
                continue;
            }
            unknown.push(n.to_string());
            continue;
        }
        // A non-Symbol key in the trailing hash lands in `**kwrest` as-is (Ruby
        // allows non-symbol keys there); without kwrest it can't bind as a
        // keyword -- real Ruby treats the hash as positional then, but this
        // convention already committed it as keywords (the documented
        // no-ruby2_keywords approximation).
        if has_kwrest {
            rest_pairs.push((k.clone(), v.clone()));
        } else {
            return Err(arg_error!("wrong number of arguments (in '{method}')"));
        }
    }
    if !unknown.is_empty() {
        return Err(kw_names_error("unknown", &unknown));
    }
    Ok((positional, req_values, opt_values, rest_pairs))
}

/// A `**nil` callee reached dynamically: a kw-marked trailing Hash means the
/// caller WROTE keywords (`**h` splat or `send`'s literal kwargs -- see
/// [`crate::collections::RHashData::kw_marked`]), which the declaration
/// refuses before any arity check (CRuby `vm_args.c` order). An unmarked
/// trailing Hash is an ordinary positional and passes through. A marked
/// EMPTY hash never arrives -- every writer drops a runtime-empty keyword
/// set before pushing it.
pub fn reject_marked_kwargs(args: &[RubyValue]) -> Result<(), Signal> {
    if let Some(RubyValue::Hash(h)) = args.last()
        && crate::collections::hash_is_kwargs(h)
    {
        return Err(arg_error!("no keywords accepted"));
    }
    Ok(())
}

/// CRuby's keyword-error wording: `missing keyword: :a` / `unknown keywords:
/// :c, :d` -- singular or plural by count, each name a `:sym`, no `(in ...)`
/// suffix. `kind` is `"missing"` or `"unknown"`.
fn kw_names_error(kind: &str, names: &[String]) -> Signal {
    let plural = if names.len() == 1 { "" } else { "s" };
    let list = names
        .iter()
        .map(|n| format!(":{n}"))
        .collect::<Vec<_>>()
        .join(", ");
    arg_error!("{kind} keyword{plural}: {list}")
}

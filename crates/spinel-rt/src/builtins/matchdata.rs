//! `MatchData`'s runtime method table.
//!
//! Every method here already existed as a `regexp::matchdata_*` function
//! reachable from `codegen::call`'s STATIC fast path (`ty ==
//! TyKind::MatchData`). This table is what makes them reachable when the
//! receiver's type ISN'T statically known -- which is exactly the `$~` case,
//! since `$~` is nil whenever the last match failed and so can never infer
//! as `MatchData`. Without it, `$~[0]` raised NoMethodError while
//! `re.match(s)[0]` worked.
//!
//! The rows delegate rather than reimplement, so the two paths cannot drift.

use crate::builtins::{arity, builtin_methods};
use crate::RubyValue;

/// One end (`idx` 0 = begin, 1 = end) of a group's `offset`/`byteoffset` pair,
/// nil when the group didn't participate.
fn offset_end(
    md: &crate::regexp::RMatchData,
    arg: &RubyValue,
    byte: bool,
    idx: usize,
) -> Result<RubyValue, crate::Signal> {
    match crate::regexp::matchdata_offset(md, arg, byte)? {
        RubyValue::Array(a) => Ok(a.lock().get(idx).cloned().unwrap_or(RubyValue::Nil)),
        _ => Ok(RubyValue::Nil),
    }
}

fn recv_md(recv: &RubyValue) -> crate::regexp::RMatchData {
    recv.as_matchdata_unchecked()
}

builtin_methods! {
    pub(crate) fn lookup;

    // Two MatchData are equal when they cover the same string with the same
    // group spans (CRuby also checks the regexp; same-string-same-spans is the
    // observable equivalent here).
    "=="[1] | "eql?"[1] => fn eq(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::MatchData(other) = &args[0] else {
            return Ok(RubyValue::Bool(false));
        };
        let a = recv_md(recv);
        Ok(RubyValue::Bool(a.haystack == other.haystack && a.groups == other.groups))
    }
    // `md[i]` / `md[name]` -- a single group; `md[start, length]` / `md[range]`
    // slice the group array (delegated to `Array#[]`, like CRuby).
    "[]" => fn get(recv, args, _block) {
        arity!(args, 1..=2);
        let md = recv_md(recv);
        if args.len() == 2 || matches!(&args[0], RubyValue::Range(..)) {
            let all = crate::regexp::matchdata_to_a(&md);
            return crate::dispatch::send_value(&all, crate::Symbol::intern("[]"), args, None);
        }
        crate::regexp::matchdata_get(&md, &args[0])
    }
    "pre_match"[0] => fn pre_match(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::regexp::matchdata_pre_match(&recv_md(recv)))
    }
    "post_match"[0] => fn post_match(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::regexp::matchdata_post_match(&recv_md(recv)))
    }
    "to_a"[0] => fn to_a(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::regexp::matchdata_to_a(&recv_md(recv)))
    }
    "captures"[0] => fn captures(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::regexp::matchdata_captures(&recv_md(recv)))
    }
    // `#size`/`#length` -- the number of elements (whole match + every group),
    // i.e. `to_a.length`.
    "size"[0] | "length"[0] => fn size(recv, args, _block) {
        arity!(args, 0);
        let arr = crate::regexp::matchdata_to_a(&recv_md(recv));
        let n = match &arr {
            RubyValue::Array(a) => a.lock().len(),
            _ => unreachable!("matchdata_to_a always answers an Array"),
        };
        Ok(RubyValue::Int(n as i64))
    }
    // `values_at(*indices)` -- the groups at those indices (`0` is the whole
    // match), each resolved the same way `[]` does, gathered into an Array.
    "values_at" => fn values_at(recv, args, _block) {
        let md = recv_md(recv);
        let out = args
            .iter()
            .map(|a| crate::regexp::matchdata_get(&md, a))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    "named_captures" => fn named_captures(recv, args, _block) {
        arity!(args, 0..=1);
        let nc = crate::regexp::matchdata_named_captures(&recv_md(recv));
        // `named_captures(symbolize_names: true)` keys the result with Symbols.
        let symbolize = matches!(args.first(), Some(RubyValue::Hash(h))
            if crate::collections::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("symbolize_names"))).truthy());
        if !symbolize {
            return Ok(nc);
        }
        let RubyValue::Hash(h) = &nc else { return Ok(nc) };
        let pairs = crate::collections::hash_pairs(h)
            .into_iter()
            .map(|(k, v)| {
                let key = match &k {
                    RubyValue::Str(s) => RubyValue::Symbol(crate::Symbol::intern(&s.lock().to_utf8_lossy())),
                    other => other.clone(),
                };
                (key, v)
            })
            .collect();
        Ok(RubyValue::Hash(crate::collections::hash_new(pairs)))
    }
    "string"[0] => fn string(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::regexp::matchdata_string(&recv_md(recv)))
    }
    "to_s"[0] => fn to_s(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::regexp::matchdata_to_s(&recv_md(recv)))
    }
    // `offset(n)`/`byteoffset(n)` -- the char/byte `[start, end]` of group `n`
    // (index or named-group Symbol/String).
    "offset"[1] => fn offset(recv, args, _block) {
        arity!(args, 1);
        crate::regexp::matchdata_offset(&recv_md(recv), &args[0], false)
    }
    "byteoffset"[1] => fn byteoffset(recv, args, _block) {
        arity!(args, 1);
        crate::regexp::matchdata_offset(&recv_md(recv), &args[0], true)
    }
    // `begin`/`end` are the character start/end of a group; `bytebegin`/`byteend`
    // the byte start/end. Each is one end of the corresponding `offset` pair.
    "begin"[1] => fn md_begin(recv, args, _block) {
        arity!(args, 1);
        offset_end(&recv_md(recv), &args[0], false, 0)
    }
    "end"[1] => fn md_end(recv, args, _block) {
        arity!(args, 1);
        offset_end(&recv_md(recv), &args[0], false, 1)
    }
    "bytebegin"[1] => fn md_bytebegin(recv, args, _block) {
        arity!(args, 1);
        offset_end(&recv_md(recv), &args[0], true, 0)
    }
    "byteend"[1] => fn md_byteend(recv, args, _block) {
        arity!(args, 1);
        offset_end(&recv_md(recv), &args[0], true, 1)
    }
    // `MatchData#match(n)` -- the n-th group (like `[n]`); `match_length(n)` its
    // character length, or nil when the group didn't participate.
    "match"[1] => fn md_match(recv, args, _block) {
        arity!(args, 1);
        crate::regexp::matchdata_get(&recv_md(recv), &args[0])
    }
    "match_length"[1] => fn md_match_length(recv, args, _block) {
        arity!(args, 1);
        Ok(match crate::regexp::matchdata_get(&recv_md(recv), &args[0])? {
            RubyValue::Str(s) => RubyValue::Int(s.lock().char_len() as i64),
            _ => RubyValue::Nil,
        })
    }
    // `deconstruct` -> the captures array (pattern-matching's array form).
    "deconstruct"[0] => fn md_deconstruct(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::regexp::matchdata_captures(&recv_md(recv)))
    }
    // `deconstruct_keys(keys)` -> the named captures as a Symbol-keyed Hash; with
    // an Array of keys, only those that name a capture (in the given order); with
    // nil, all of them (pattern-matching's hash form).
    "deconstruct_keys"[1] => fn md_deconstruct_keys(recv, args, _block) {
        arity!(args, 1);
        let md = recv_md(recv);
        let named: Vec<(String, RubyValue)> = md
            .names
            .iter()
            .map(|(name, idx)| (name.clone(), crate::regexp::matchdata_group(&md, *idx as i64)))
            .collect();
        let sym = |s: &str| RubyValue::Symbol(crate::Symbol::intern(s));
        let pairs: Vec<(RubyValue, RubyValue)> = match &args[0] {
            RubyValue::Nil => named.iter().map(|(n, v)| (sym(n), v.clone())).collect(),
            RubyValue::Array(keys) => {
                let keys: Vec<RubyValue> = keys.lock().iter().cloned().collect();
                // CRuby's rule: more keys than captures can't match -> empty; and
                // the first key that isn't a capture stops the walk (partial).
                if keys.len() > named.len() {
                    Vec::new()
                } else {
                    let mut out = Vec::new();
                    for k in &keys {
                        let RubyValue::Symbol(s) = k else { break };
                        match named.iter().find(|(n, _)| *n == s.name()) {
                            Some((_, v)) => out.push((RubyValue::Symbol(*s), v.clone())),
                            None => break,
                        }
                    }
                    out
                }
            }
            other => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!("wrong argument type {} (expected Array)", crate::builtins::class_name_of(other)),
                ))
            }
        };
        Ok(RubyValue::Hash(crate::collections::hash_new(pairs)))
    }
    // `#<MatchData "whole" 1:"a" name:"b" ...>` -- groups labeled by name when
    // named, by 1-based index otherwise.
    "inspect"[0] => fn md_inspect(recv, args, _block) {
        arity!(args, 0);
        let md = recv_md(recv);
        let RubyValue::Array(a) = crate::regexp::matchdata_to_a(&md) else { unreachable!() };
        let items: Vec<RubyValue> = a.lock().iter().cloned().collect();
        let mut s = format!("#<MatchData {}", items[0].inspect_string());
        for (i, item) in items.iter().enumerate().skip(1) {
            let label = md
                .names
                .iter()
                .find(|(_, idx)| *idx == i)
                .map(|(n, _)| n.clone())
                .unwrap_or_else(|| i.to_string());
            s.push_str(&format!(" {label}:{}", item.inspect_string()));
        }
        s.push('>');
        Ok(RubyValue::Str(crate::string_new(s)))
    }
    "names"[0] => fn names(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::regexp::matchdata_names(&recv_md(recv)))
    }
    "regexp"[0] => fn regexp(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::regexp::matchdata_regexp(&recv_md(recv)))
    }
}

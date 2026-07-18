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

fn recv_md(recv: &RubyValue) -> crate::regexp::RMatchData {
    recv.as_matchdata_unchecked()
}

builtin_methods! {
    pub(crate) fn lookup;

    // Two MatchData are equal when they cover the same string with the same
    // group spans (CRuby also checks the regexp; same-string-same-spans is the
    // observable equivalent here).
    "==" | "eql?" => fn eq(recv, args, _block) {
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
        Ok(crate::regexp::matchdata_get(&md, &args[0]))
    }
    "pre_match" => fn pre_match(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::regexp::matchdata_pre_match(&recv_md(recv)))
    }
    "post_match" => fn post_match(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::regexp::matchdata_post_match(&recv_md(recv)))
    }
    "to_a" => fn to_a(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::regexp::matchdata_to_a(&recv_md(recv)))
    }
    "captures" => fn captures(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::regexp::matchdata_captures(&recv_md(recv)))
    }
    // `values_at(*indices)` -- the groups at those indices (`0` is the whole
    // match), each resolved the same way `[]` does, gathered into an Array.
    "values_at" => fn values_at(recv, args, _block) {
        let md = recv_md(recv);
        let out = args
            .iter()
            .map(|a| crate::regexp::matchdata_get(&md, a))
            .collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    "named_captures" => fn named_captures(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::regexp::matchdata_named_captures(&recv_md(recv)))
    }
    "string" => fn string(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::regexp::matchdata_string(&recv_md(recv)))
    }
    "to_s" => fn to_s(recv, args, _block) {
        arity!(args, 0);
        Ok(crate::regexp::matchdata_to_s(&recv_md(recv)))
    }
}

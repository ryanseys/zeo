//! `case/in`, `expr in pattern` and `expr => pattern` -- Ruby's pattern
//! matching, lowered as branching control flow.
//!
//! A pattern lowers to a chain of sub-checks with one shared
//! FAILURE BLOCK threaded through: every sub-check
//! jumps there on mismatch and falls through on a match, so a pattern is
//! "reached the end without jumping". Bindings are side effects along the
//! way and survive a later sub-check's failure, exactly as they do in
//! ruby.
//!
//! Every leaf test and every failure record calls the runtime helpers in
//! `crates/zeo-rt/src/capi/patterns.rs`,
//! so compiled and runtime `NoMatchingPatternError` messages cannot drift.

use super::ctx::Fx;
use super::operand::{Operand, TagInfo};
use super::ownership;
use crate::hir::{HashPatternRest, NodeId, Pattern, PatternArm};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{self, InstBuilder, MemFlagsData, types};
use zeo_abi::abi::{TAG_OFFSET, ValueTag};

/// `case subject; in ...; end` in value position: the arms' value lands in
/// `dst`.
pub(crate) fn lower_case_in(
    fx: &mut Fx,
    site: NodeId,
    subject: NodeId,
    arms: &[PatternArm],
    else_body: &Option<Vec<NodeId>>,
    dst: ir::Value,
) -> Result<(), String> {
    // The subject is evaluated ONCE and borrowed by every arm; an owned
    // temp hands its value to the pool first so a raising arm cannot
    // strand it.
    let subject_op = super::expr::lower_expr(fx, subject)?;
    let subj = ownership::borrow_ptr(fx, &subject_op);
    if subject_op.owned() {
        ownership::pool_owned(fx, subj, subject_op.tag());
    }
    // Arming the key-miss record costs a thread-local write, so only the
    // shape that will READ one pays for clearing it.
    let single = arms.len() == 1;
    if single {
        fx.call("zeo_rt_pat_key_miss_clear", &[]);
    }

    let done = fx.b.create_block();
    for arm in arms {
        let fail = fx.b.create_block();
        match_pattern(fx, site, &arm.pattern, subj, fail)?;
        if let Some((guard, is_unless)) = &arm.guard {
            let (guard, is_unless) = (*guard, *is_unless);
            let g = super::expr::lower_expr(fx, guard)?;
            let t = ownership::truthy(fx, g);
            let ok = fx.b.create_block();
            let bad = fx.b.create_block();
            if is_unless {
                fx.b.ins().brif(t, bad, &[], ok, &[]);
            } else {
                fx.b.ins().brif(t, ok, &[], bad, &[]);
            }
            fx.b.switch_to_block(bad);
            // A guard that rejects is its own reason: the pattern matched
            // and the condition did not.
            fx.call("zeo_rt_pat_fail_guard", &[]);
            fx.b.ins().jump(fail, &[]);
            fx.b.switch_to_block(ok);
        }
        super::stmt::lower_value_body_into(fx, &arm.body, dst)?;
        fx.b.ins().jump(done, &[]);
        fx.b.switch_to_block(fail);
    }
    // Every arm rejected.
    match else_body {
        Some(body) => super::stmt::lower_value_body_into(fx, body, dst)?,
        None => {
            let f = if single {
                "zeo_rt_pat_match_error"
            } else {
                "zeo_rt_pat_match_error_bare"
            };
            let st = fx.call_status(f, &[subj]);
            fx.fallible(st);
            // The raise never returns; keep the block well formed.
            ownership::write_move_into(fx, &Operand::Nil, dst);
        }
    }
    fx.b.ins().jump(done, &[]);
    fx.b.switch_to_block(done);
    Ok(())
}

/// `expr in pattern` -- a boolean one-liner that never raises. Bindings a
/// successful match makes still leak into the enclosing scope, as ruby's do.
pub(crate) fn lower_match_predicate(
    fx: &mut Fx,
    site: NodeId,
    subject: NodeId,
    pattern: &Pattern,
) -> Result<Operand, String> {
    let subject_op = super::expr::lower_expr(fx, subject)?;
    let subj = ownership::borrow_ptr(fx, &subject_op);
    if subject_op.owned() {
        ownership::pool_owned(fx, subj, subject_op.tag());
    }
    let fail = fx.b.create_block();
    let join = fx.b.create_block();
    fx.b.append_block_param(join, types::I8);
    match_pattern(fx, site, pattern, subj, fail)?;
    let yes = fx.b.ins().iconst(types::I8, 1);
    fx.b.ins().jump(join, &[yes.into()]);
    fx.b.switch_to_block(fail);
    let no = fx.b.ins().iconst(types::I8, 0);
    fx.b.ins().jump(join, &[no.into()]);
    fx.b.switch_to_block(join);
    let matched = fx.b.block_params(join)[0];
    Ok(Operand::Bool(matched))
}

/// `expr => pattern` -- the same match, raising `NoMatchingPatternError`
/// (or `...KeyError`) instead of answering false. Its value is `nil`.
pub(crate) fn lower_match_required(
    fx: &mut Fx,
    site: NodeId,
    subject: NodeId,
    pattern: &Pattern,
) -> Result<Operand, String> {
    let subject_op = super::expr::lower_expr(fx, subject)?;
    let subj = ownership::borrow_ptr(fx, &subject_op);
    if subject_op.owned() {
        ownership::pool_owned(fx, subj, subject_op.tag());
    }
    fx.call("zeo_rt_pat_key_miss_clear", &[]);
    let fail = fx.b.create_block();
    let ok = fx.b.create_block();
    match_pattern(fx, site, pattern, subj, fail)?;
    fx.b.ins().jump(ok, &[]);
    fx.b.switch_to_block(fail);
    let st = fx.call_status("zeo_rt_pat_match_error", &[subj]);
    fx.fallible(st);
    fx.b.ins().jump(ok, &[]);
    fx.b.switch_to_block(ok);
    Ok(Operand::Nil)
}

/// Emit `pattern`'s checks against the 24-byte value at `scrut`: a
/// mismatch jumps to `fail`, a match falls through.
fn match_pattern(
    fx: &mut Fx,
    site: NodeId,
    pattern: &Pattern,
    scrut: ir::Value,
    fail: ir::Block,
) -> Result<(), String> {
    match pattern {
        // A bare identifier always matches, binding the scrutinee.
        Pattern::Bind(name) => {
            bind_name(fx, name, scrut);
            Ok(())
        }
        // Value and pin patterns both match through `case_eq` -- ruby's
        // `#===` protocol, the same entry `case/when` uses, so `in` and
        // `when` cannot drift apart.
        Pattern::Value(node) | Pattern::Pin(node) => {
            let node = *node;
            let pat = super::expr::lower_expr(fx, node)?;
            let p = ownership::borrow_ptr(fx, &pat);
            if pat.owned() {
                ownership::pool_owned(fx, p, pat.tag());
            }
            case_eq_check(fx, p, scrut, fail)
        }
        Pattern::ClassCheck(name) => class_check(fx, site, name, scrut, fail),
        Pattern::Range {
            start,
            end,
            exclusive,
        } => {
            let (start, end, exclusive) = (*start, *end, *exclusive);
            let r = range_value(fx, start, end, exclusive)?;
            case_eq_check(fx, r, scrut, fail)
        }
        // `P1 | P2`: the first alternative that matches wins. Ruby forbids
        // an alternative from binding, so no bindings escape a failed one.
        Pattern::Or(pats) => {
            let hit = fx.b.create_block();
            for p in pats {
                let next = fx.b.create_block();
                match_pattern(fx, site, p, scrut, next)?;
                fx.b.ins().jump(hit, &[]);
                fx.b.switch_to_block(next);
            }
            fx.b.ins().jump(fail, &[]);
            fx.b.switch_to_block(hit);
            Ok(())
        }
        // `PAT => name` binds only once `PAT` itself matched.
        Pattern::Capture(inner, name) => {
            match_pattern(fx, site, inner, scrut, fail)?;
            bind_name(fx, name, scrut);
            Ok(())
        }
        Pattern::Array {
            constant,
            pre,
            rest,
            post,
        } => array_pattern(fx, site, constant, pre, rest, post, scrut, fail),
        Pattern::Find {
            constant,
            pre_rest,
            mid,
            post_rest,
        } => find_pattern(fx, site, constant, pre_rest, mid, post_rest, scrut, fail),
        Pattern::Hash {
            constant,
            pairs,
            rest,
        } => hash_pattern(fx, site, constant, pairs, rest, scrut, fail),
    }
}

/// Bind `name` to the scrutinee (a retained copy -- the local owns its
/// value independently of the subject's lifetime).
fn bind_name(fx: &mut Fx, name: &str, scrut: ir::Value) {
    let op = Operand::Ptr {
        addr: scrut,
        owned: false,
        tag: TagInfo::Unknown,
    };
    ownership::write_local(fx, name, &op);
}

/// `pat === scrut`, recording `P === v does not return true` when it
/// rejects. Every leaf pattern with a nameable left-hand side comes here.
fn case_eq_check(
    fx: &mut Fx,
    pat: ir::Value,
    scrut: ir::Value,
    fail: ir::Block,
) -> Result<(), String> {
    let out = fx.temp_slot();
    let dst = fx.slot_addr(out, 0);
    let st = fx.call_status("zeo_rt_case_eq", &[pat, scrut, dst]);
    fx.fallible(st);
    let fl = MemFlagsData::trusted();
    let matched = fx.b.ins().load(types::I8, fl, dst, 0);
    let ok = fx.b.create_block();
    let no = fx.b.create_block();
    fx.b.ins().brif(matched, ok, &[], no, &[]);
    fx.b.switch_to_block(no);
    fx.call("zeo_rt_pat_fail_case_eq", &[pat, scrut]);
    fx.b.ins().jump(fail, &[]);
    fx.b.switch_to_block(ok);
    Ok(())
}

/// `in Integer` / `in SomeClass` (also an Array/Hash/Find pattern's
/// optional constant guard). A statically resolvable class is an `is_a?`
/// over the linearized ancestry; a builtin primitive NAME is a runtime
/// tag test on the scrutinee (every CLIF value is
/// dynamic); anything else reads the constant and matches by `===`, so a
/// runtime class bound to a constant (`Struct.new`) still works and an
/// unset one raises `NameError` from the read.
fn class_check(
    fx: &mut Fx,
    site: NodeId,
    name: &str,
    scrut: ir::Value,
    fail: ir::Block,
) -> Result<(), String> {
    if let Some(tag) = builtin_tag(name) {
        let fl = MemFlagsData::trusted();
        let t = fx.b.ins().load(types::I8, fl, scrut, TAG_OFFSET as i32);
        let matched = match tag {
            // `TrueClass`/`FalseClass` share the Bool tag: the payload
            // byte decides.
            BuiltinTag::Bool(want) => {
                let is_bool =
                    fx.b.ins()
                        .icmp_imm_u(IntCC::Equal, t, i64::from(ValueTag::Bool as u8));
                let payload =
                    fx.b.ins()
                        .load(types::I8, fl, scrut, zeo_abi::abi::PAYLOAD_OFFSET as i32);
                let want = fx.b.ins().iconst(types::I8, i64::from(u8::from(want)));
                let same = fx.b.ins().icmp(IntCC::Equal, payload, want);
                fx.b.ins().band(is_bool, same)
            }
            BuiltinTag::Tag(v) => fx.b.ins().icmp_imm_u(IntCC::Equal, t, i64::from(v)),
        };
        return record_class_miss(fx, name, matched, scrut, fail);
    }
    if let Some(cid) = super::boxes::resolve_class_here(fx, name) {
        let id = fx.b.ins().iconst(types::I32, i64::from(cid.0));
        let matched = fx.call_status("zeo_rt_pat_is_a", &[scrut, id]);
        return record_class_miss(fx, name, matched, scrut, fail);
    }
    // Not a statically-known class: read the constant and match by `===`.
    let konst = super::consts::const_read(fx, site, name)?;
    let p = ownership::borrow_ptr(fx, &konst);
    if konst.owned() {
        ownership::pool_owned(fx, p, konst.tag());
    }
    case_eq_check(fx, p, scrut, fail)
}

/// A class check's rejection reports the same sentence a value pattern
/// does -- the class is the left-hand side of the `===` ruby names.
fn record_class_miss(
    fx: &mut Fx,
    name: &str,
    matched: ir::Value,
    scrut: ir::Value,
    fail: ir::Block,
) -> Result<(), String> {
    let ok = fx.b.create_block();
    let no = fx.b.create_block();
    fx.b.ins().brif(matched, ok, &[], no, &[]);
    fx.b.switch_to_block(no);
    // The reported left-hand side is the CLASS, built as an immediate;
    // an unresolvable name reached `case_eq_check` instead of here.
    if let Some(cid) = class_of_name(fx, name) {
        let ss = fx.temp_slot();
        let addr = fx.slot_addr(ss, 0);
        let fl = MemFlagsData::trusted();
        let z = fx.b.ins().iconst(types::I64, 0);
        for off in [0, 8, 16] {
            fx.b.ins().store(fl, z, addr, off);
        }
        let tag =
            fx.b.ins()
                .iconst(types::I8, i64::from(ValueTag::Class as u8));
        fx.b.ins().store(fl, tag, addr, TAG_OFFSET as i32);
        let id = fx.b.ins().iconst(types::I32, i64::from(cid.0));
        fx.b.ins()
            .store(fl, id, addr, zeo_abi::abi::PAYLOAD_OFFSET as i32);
        fx.call("zeo_rt_pat_fail_case_eq", &[addr, scrut]);
    }
    fx.b.ins().jump(fail, &[]);
    fx.b.switch_to_block(ok);
    Ok(())
}

/// The class id a pattern's class NAME reports on rejection: a builtin
/// primitive name resolves through the abi table, a user class through the
/// compiler.
fn class_of_name(fx: &Fx, name: &str) -> Option<zeo_abi::ClassId> {
    if let Some(cid) = super::boxes::resolve_class_here(fx, name) {
        return Some(cid);
    }
    zeo_abi::BUILTINS
        .iter()
        .find(|b| b.name == name)
        .map(|b| b.id)
}

/// The builtin primitive names a pattern position may name, as the runtime
/// TAG test used for the scrutinee.
enum BuiltinTag {
    Tag(u8),
    Bool(bool),
}

fn builtin_tag(name: &str) -> Option<BuiltinTag> {
    Some(match name {
        "Integer" => BuiltinTag::Tag(ValueTag::Int as u8),
        "String" => BuiltinTag::Tag(ValueTag::Str as u8),
        "Symbol" => BuiltinTag::Tag(ValueTag::Symbol as u8),
        "Float" => BuiltinTag::Tag(ValueTag::Float as u8),
        "Array" => BuiltinTag::Tag(ValueTag::Array as u8),
        "Hash" => BuiltinTag::Tag(ValueTag::Hash as u8),
        "Range" => BuiltinTag::Tag(ValueTag::Range as u8),
        "Proc" => BuiltinTag::Tag(ValueTag::Proc as u8),
        "NilClass" => BuiltinTag::Tag(ValueTag::Nil as u8),
        "TrueClass" => BuiltinTag::Bool(true),
        "FalseClass" => BuiltinTag::Bool(false),
        _ => return None,
    })
}

/// `1..10` as a pattern: a real `Range` value matched by `===`, so
/// exclusivity, beginless/endless and a Float scrutinee all work through
/// the runtime's own cover check.
fn range_value(
    fx: &mut Fx,
    start: Option<NodeId>,
    end: Option<NodeId>,
    exclusive: bool,
) -> Result<ir::Value, String> {
    let bound = |fx: &mut Fx, n: Option<NodeId>| -> Result<ir::Value, String> {
        match n {
            Some(n) => {
                let op = super::expr::lower_expr(fx, n)?;
                Ok(ownership::move_ptr(fx, &op))
            }
            None => {
                let ss = fx.temp_slot();
                let addr = fx.slot_addr(ss, 0);
                ownership::write_move_into(fx, &Operand::Nil, addr);
                Ok(addr)
            }
        }
    };
    let a = bound(fx, start)?;
    let b = bound(fx, end)?;
    let excl = fx.b.ins().iconst(types::I8, i64::from(u8::from(exclusive)));
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    let st = fx.call_status("zeo_rt_range_new", &[a, b, excl, dst]);
    fx.fallible(st);
    fx.owned_created += 1;
    ownership::pool_owned(fx, dst, TagInfo::Unknown);
    Ok(dst)
}

/// `[pre.., *rest, post..]` -- `#deconstruct`, a length check, then the
/// element sub-patterns.
#[allow(
    clippy::too_many_arguments,
    reason = "an array pattern's parts (constant guard, pre, rest, post) each carry their own meaning; bundling them into a struct would only move the list"
)]
fn array_pattern(
    fx: &mut Fx,
    site: NodeId,
    constant: &Option<String>,
    pre: &[Pattern],
    rest: &Option<Option<String>>,
    post: &[Pattern],
    scrut: ir::Value,
    fail: ir::Block,
) -> Result<(), String> {
    if let Some(name) = constant {
        class_check(fx, site, name, scrut, fail)?;
    }
    let arr = deconstruct(fx, scrut, Protocol::Array, fail)?;
    let len = fx.call_status("zeo_rt_pat_array_len", &[arr]);
    let min = pre.len() + post.len();
    let open = rest.is_some();
    let cc = if open {
        IntCC::UnsignedGreaterThanOrEqual
    } else {
        IntCC::Equal
    };
    let ok_len = fx.b.ins().icmp_imm_u(cc, len, min as i64);
    let ok = fx.b.create_block();
    let bad = fx.b.create_block();
    fx.b.ins().brif(ok_len, ok, &[], bad, &[]);
    fx.b.switch_to_block(bad);
    // The length mismatch names the DECONSTRUCTED array, not the subject.
    let expected = fx.b.ins().iconst(fx.em.ptr, min as i64);
    let open_v = fx.b.ins().iconst(types::I8, i64::from(u8::from(open)));
    fx.call("zeo_rt_pat_fail_length", &[arr, expected, open_v]);
    fx.b.ins().jump(fail, &[]);
    fx.b.switch_to_block(ok);

    for (i, p) in pre.iter().enumerate() {
        let idx = fx.b.ins().iconst(fx.em.ptr, i as i64);
        let elem = array_elem(fx, arr, idx);
        match_pattern(fx, site, p, elem, fail)?;
    }
    let post_len = post.len();
    for (i, p) in post.iter().enumerate() {
        let offset = (post_len - i) as i64;
        let idx = fx.b.ins().iadd_imm_s(len, -offset);
        let elem = array_elem(fx, arr, idx);
        match_pattern(fx, site, p, elem, fail)?;
    }
    if let Some(Some(name)) = rest {
        let from = fx.b.ins().iconst(fx.em.ptr, pre.len() as i64);
        let to = fx.b.ins().iadd_imm_s(len, -(post_len as i64));
        slice_into_local(fx, name, arr, from, to);
    }
    Ok(())
}

/// `[*, mid.., *]` -- `mid` must match SOME contiguous window, searched
/// left to right, first hit winning. Inherently a runtime loop.
#[allow(
    clippy::too_many_arguments,
    reason = "a find pattern's four independent parts (constant guard, both splat names, the window) each carry their own meaning; bundling them into a struct would only move the list"
)]
fn find_pattern(
    fx: &mut Fx,
    site: NodeId,
    constant: &Option<String>,
    pre_rest: &Option<String>,
    mid: &[Pattern],
    post_rest: &Option<String>,
    scrut: ir::Value,
    fail: ir::Block,
) -> Result<(), String> {
    if let Some(name) = constant {
        class_check(fx, site, name, scrut, fail)?;
    }
    let arr = deconstruct(fx, scrut, Protocol::Array, fail)?;
    let len = fx.call_status("zeo_rt_pat_array_len", &[arr]);
    let width = mid.len() as i64;
    // The search loop carries its window offset as a block parameter.
    let head = fx.b.create_block();
    fx.b.append_block_param(head, fx.em.ptr);
    let none = fx.b.create_block();
    let fits =
        fx.b.ins()
            .icmp_imm_u(IntCC::UnsignedGreaterThanOrEqual, len, width);
    let zero = fx.b.ins().iconst(fx.em.ptr, 0);
    fx.b.ins().brif(fits, head, &[zero.into()], none, &[]);

    fx.b.switch_to_block(none);
    fx.call("zeo_rt_pat_fail_find", &[arr]);
    fx.b.ins().jump(fail, &[]);

    fx.b.switch_to_block(head);
    let start = fx.b.block_params(head)[0];
    // `last = len - width`; `fits` above proved it non-negative.
    let last = fx.b.ins().iadd_imm_s(len, -width);
    let body = fx.b.create_block();
    let hit = fx.b.create_block();
    let next = fx.b.create_block();
    let in_range = fx.b.ins().icmp(IntCC::UnsignedLessThanOrEqual, start, last);
    fx.b.ins().brif(in_range, body, &[], none, &[]);

    fx.b.switch_to_block(body);
    for (i, p) in mid.iter().enumerate() {
        let idx = fx.b.ins().iadd_imm_s(start, i as i64);
        let elem = array_elem(fx, arr, idx);
        match_pattern(fx, site, p, elem, next)?;
    }
    fx.b.ins().jump(hit, &[]);
    fx.b.switch_to_block(next);
    let bumped = fx.b.ins().iadd_imm_s(start, 1);
    fx.b.ins().jump(head, &[bumped.into()]);

    fx.b.switch_to_block(hit);
    if let Some(name) = pre_rest {
        slice_into_local(fx, name, arr, zero, start);
    }
    if let Some(name) = post_rest {
        let from = fx.b.ins().iadd_imm_s(start, width);
        slice_into_local(fx, name, arr, from, len);
    }
    Ok(())
}

/// `arr[from...to]` bound to a local -- what an array/find pattern's `*`
/// names.
fn slice_into_local(fx: &mut Fx, name: &str, arr: ir::Value, from: ir::Value, to: ir::Value) {
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    fx.call("zeo_rt_pat_array_slice", &[arr, from, to, dst]);
    fx.owned_created += 1;
    ownership::pool_owned(fx, dst, TagInfo::Known(ValueTag::Array as u8));
    let op = Operand::Ptr {
        addr: dst,
        owned: false,
        tag: TagInfo::Known(ValueTag::Array as u8),
    };
    ownership::write_local(fx, name, &op);
}

/// `{key: pattern, ..., **rest}` -- `#deconstruct_keys`, then per-key
/// presence and value checks.
fn hash_pattern(
    fx: &mut Fx,
    site: NodeId,
    constant: &Option<String>,
    pairs: &[(String, Option<Pattern>)],
    rest: &HashPatternRest,
    scrut: ir::Value,
    fail: ir::Block,
) -> Result<(), String> {
    if let Some(name) = constant {
        class_check(fx, site, name, scrut, fail)?;
    }
    // CRuby passes the NAMED keys, and `nil` for a pattern that can take
    // everything: one with a rest (`**r` or `**nil`) or with no keys at
    // all. Oracle-verified in all four shapes.
    let named: Option<Vec<String>> = match rest {
        HashPatternRest::None if !pairs.is_empty() => {
            Some(pairs.iter().map(|(k, _)| k.clone()).collect())
        }
        _ => None,
    };
    let h = deconstruct(fx, scrut, Protocol::Keys(named.as_deref()), fail)?;
    // `in {}` is NOT the lenient form: an EMPTY hash pattern asks for an
    // empty hash, ruby's one exception to hash-pattern leniency.
    if pairs.is_empty() && matches!(rest, HashPatternRest::None) {
        let n = fx.call_status("zeo_rt_pat_hash_len", &[h]);
        let empty = fx.b.ins().icmp_imm_u(IntCC::Equal, n, 0);
        let ok = fx.b.create_block();
        let bad = fx.b.create_block();
        fx.b.ins().brif(empty, ok, &[], bad, &[]);
        fx.b.switch_to_block(bad);
        let zero = fx.b.ins().iconst(types::I8, 0);
        fx.call("zeo_rt_pat_fail_not_empty", &[h, zero]);
        fx.b.ins().jump(fail, &[]);
        fx.b.switch_to_block(ok);
    }
    // `**nil` asks that nothing else remain; ruby words it differently
    // from the empty-pattern case and names the LEFTOVERS.
    if matches!(rest, HashPatternRest::NoMoreKeys) {
        let n = fx.call_status("zeo_rt_pat_hash_len", &[h]);
        let exact = fx.b.ins().icmp_imm_u(IntCC::Equal, n, pairs.len() as i64);
        let ok = fx.b.create_block();
        let bad = fx.b.create_block();
        fx.b.ins().brif(exact, ok, &[], bad, &[]);
        fx.b.switch_to_block(bad);
        if pairs.is_empty() {
            let zero = fx.b.ins().iconst(types::I8, 0);
            fx.call("zeo_rt_pat_fail_not_empty", &[h, zero]);
        } else {
            let leftovers = hash_except(fx, h, pairs);
            let one = fx.b.ins().iconst(types::I8, 1);
            fx.call("zeo_rt_pat_fail_not_empty", &[leftovers, one]);
        }
        fx.b.ins().jump(fail, &[]);
        fx.b.switch_to_block(ok);
    }
    for (key, pat) in pairs {
        let sym = fx.sym_id(key);
        let present = fx.call_status("zeo_rt_pat_hash_has_key", &[h, sym]);
        let ok = fx.b.create_block();
        let missing = fx.b.create_block();
        fx.b.ins().brif(present, ok, &[], missing, &[]);
        fx.b.switch_to_block(missing);
        // A MISSING key is ruby's `NoMatchingPatternKeyError`; a key that
        // IS present with a non-matching value is the plain parent error,
        // so only this one records itself.
        fx.call("zeo_rt_pat_key_miss_record", &[sym, h]);
        fx.b.ins().jump(fail, &[]);
        fx.b.switch_to_block(ok);

        let ss = fx.temp_slot();
        let value = fx.slot_addr(ss, 0);
        fx.call("zeo_rt_pat_hash_get", &[h, sym, value]);
        fx.owned_created += 1;
        ownership::pool_owned(fx, value, TagInfo::Unknown);
        match pat {
            Some(p) => match_pattern(fx, site, p, value, fail)?,
            // `{key:}` shorthand: binds a local named `key` directly.
            None => bind_name(fx, key, value),
        }
    }
    if let HashPatternRest::Rest(Some(name)) = rest {
        let leftovers = hash_except(fx, h, pairs);
        let op = Operand::Ptr {
            addr: leftovers,
            owned: false,
            tag: TagInfo::Known(ValueTag::Hash as u8),
        };
        ownership::write_local(fx, name, &op);
    }
    Ok(())
}

/// Everything the pattern did not name, as a new pooled Hash.
fn hash_except(fx: &mut Fx, h: ir::Value, pairs: &[(String, Option<Pattern>)]) -> ir::Value {
    let n = pairs.len();
    let keys = if n == 0 {
        fx.b.ins().iconst(fx.em.ptr, 0)
    } else {
        let ss = fx.b.create_sized_stack_slot(ir::StackSlotData::new(
            ir::StackSlotKind::ExplicitSlot,
            (n * 4) as u32,
            2,
        ));
        for (i, (key, _)) in pairs.iter().enumerate() {
            let sym = fx.sym_id(key);
            let at = fx.slot_addr(ss, (i * 4) as i32);
            fx.b.ins().store(MemFlagsData::trusted(), sym, at, 0);
        }
        fx.slot_addr(ss, 0)
    };
    let count = fx.b.ins().iconst(fx.em.ptr, n as i64);
    let out = fx.temp_slot();
    let dst = fx.slot_addr(out, 0);
    fx.call("zeo_rt_pat_hash_except", &[h, keys, count, dst]);
    fx.owned_created += 1;
    ownership::pool_owned(fx, dst, TagInfo::Known(ValueTag::Hash as u8));
    dst
}

/// `#deconstruct` / `#deconstruct_keys`: an Array/Hash is itself, a value
/// answering the protocol is asked, anything else simply does not match.
/// Which destructuring protocol a pattern asks for.
enum Protocol<'a> {
    /// `#deconstruct` -- an array pattern.
    Array,
    /// `#deconstruct_keys`, with the keys the pattern NAMES, or `None` for
    /// CRuby's `nil` (a pattern that can take everything).
    Keys(Option<&'a [String]>),
}

fn deconstruct(
    fx: &mut Fx,
    scrut: ir::Value,
    protocol: Protocol<'_>,
    fail: ir::Block,
) -> Result<ir::Value, String> {
    let out = fx.temp_slot();
    let dst = fx.slot_addr(out, 0);
    let flag = fx.b.create_sized_stack_slot(ir::StackSlotData::new(
        ir::StackSlotKind::ExplicitSlot,
        1,
        0,
    ));
    let flag_ptr = fx.slot_addr(flag, 0);
    let st = match protocol {
        // The keys the pattern NAMES, as interned symbol ids. A null
        // pointer is CRuby's `nil` -- what it passes for a pattern that can
        // take everything.
        Protocol::Keys(named) => {
            let named: &[String] = named.unwrap_or(&[]);
            let (ptr, n) = if named.is_empty() {
                (fx.b.ins().iconst(fx.em.ptr, 0), 0)
            } else {
                let ss = fx.b.create_sized_stack_slot(ir::StackSlotData::new(
                    ir::StackSlotKind::ExplicitSlot,
                    named.len() as u32 * 4,
                    2,
                ));
                for (i, k) in named.iter().enumerate() {
                    let sym = fx.sym_id(k);
                    let at = fx.slot_addr(ss, i as i32 * 4);
                    fx.b.ins().store(MemFlagsData::trusted(), sym, at, 0);
                }
                (fx.slot_addr(ss, 0), named.len())
            };
            let count = fx.b.ins().iconst(fx.em.ptr, n as i64);
            fx.call(
                "zeo_rt_pat_deconstruct_keys",
                &[scrut, ptr, count, dst, flag_ptr],
            )
        }
        Protocol::Array => fx.call("zeo_rt_pat_deconstruct", &[scrut, dst, flag_ptr]),
    }
    .expect("deconstruct returns a status");
    fx.fallible(st);
    let matched =
        fx.b.ins()
            .load(types::I8, MemFlagsData::trusted(), flag_ptr, 0);
    let ok = fx.b.create_block();
    fx.b.ins().brif(matched, ok, &[], fail, &[]);
    fx.b.switch_to_block(ok);
    // The deconstructed value is a fresh owned reference; the frame pool
    // owns it from here (the sub-patterns only borrow).
    fx.owned_created += 1;
    ownership::pool_owned(fx, dst, TagInfo::Unknown);
    Ok(dst)
}

/// One element of the deconstructed array, in a pooled temp slot.
fn array_elem(fx: &mut Fx, arr: ir::Value, idx: ir::Value) -> ir::Value {
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    fx.call("zeo_rt_pat_array_get", &[arr, idx, dst]);
    fx.owned_created += 1;
    ownership::pool_owned(fx, dst, TagInfo::Unknown);
    dst
}

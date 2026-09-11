//! Literals that reach rodata: the interpolation-free string a `Str` node
//! folds to, its encoding, and the compiled regexp beside it.

use super::*;

/// A baked `u32` array in rodata, 4-aligned (the runtime reads it as
/// `&[u32]`) -- a numeric literal's digits.
pub(super) fn u32_array(
    fx: &mut Fx,
    digits: &[u32],
) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
    let bytes: Vec<u8> = digits.iter().flat_map(|d| d.to_ne_bytes()).collect();
    let off = fx.em.intern_rodata_aligned(&bytes, 4);
    let ptr = fx.rod(off);
    let n = fx.b.ins().iconst(fx.em.ptr, digits.len() as i64);
    (ptr, n)
}

/// The `EncodingId` a `# encoding:` magic comment puts on every literal in
/// the program, if one is set. Lowering already normalized the comment to
/// one of the four names zeo supports (UTF-8 is `None` -- the default).
pub(super) fn script_encoding_id(fx: &Fx) -> Option<i64> {
    let name = fx.an.compiler.hir.script_encoding.as_deref()?;
    let id = match name {
        "US_ASCII" => zeo_rt::encoding::US_ASCII,
        "ASCII_8BIT" => zeo_rt::encoding::ASCII_8BIT,
        "ISO_8859_1" => zeo_rt::encoding::ISO_8859_1,
        other => unreachable!("lower::literals normalizes the magic comment; got `{other}`"),
    };
    Some(i64::from(id.0))
}

/// The pure (single non-interpolated UTF-8 part) text of a string literal.
pub(crate) fn pure_literal(parts: &[StrPart]) -> Option<String> {
    match parts {
        [] => Some(String::new()),
        [StrPart::Lit(s)] => Some(s.clone()),
        [StrPart::Bytes(_) | StrPart::Interp(_)] | [_, _, ..] => None,
    }
}

/// `name`'s bytes interned in `.rodata`, as a `(ptr, len)` argument pair.
pub(crate) fn rodata_name(
    fx: &mut Fx,
    name: &str,
) -> (cranelift_codegen::ir::Value, cranelift_codegen::ir::Value) {
    let off = fx.em.intern_rodata(name.as_bytes());
    let ptr = fx.rod(off);
    let len = fx.b.ins().iconst(fx.em.ptr, name.len() as i64);
    (ptr, len)
}

/// A regexp literal. Static parts (raw-byte segments rendered lossily)
/// fold into one source string served by a per-site cache
/// (`zeo_rt_regexp_lit` -- one frozen object per site); an interpolated
/// pattern builds a fresh string through the to_s dispatch, then
/// `zeo_rt_regexp_interp` compiles it, frozen at birth; under `/o` only the
/// first evaluation does, and the site keeps that object. Both raise
/// `RegexpError` on a bad pattern.
pub(super) fn regexp_lit(
    fx: &mut Fx,
    parts: &[crate::hir::StrPart],
    flags: crate::hir::RegexpFlags,
) -> CResult<Operand> {
    use crate::hir::StrPart;
    let enc_byte = match flags.encoding {
        zeo_abi::RegexpEncoding::Source => 0i64,
        zeo_abi::RegexpEncoding::None => 1,
        zeo_abi::RegexpEncoding::EucJp => 2,
        zeo_abi::RegexpEncoding::Windows31j => 3,
        zeo_abi::RegexpEncoding::Utf8 => 4,
    };
    let flag_vals = |fx: &mut Fx| {
        let ic = fx.b.ins().iconst(types::I8, i64::from(flags.ignore_case));
        let ext = fx.b.ins().iconst(types::I8, i64::from(flags.extended));
        let ml = fx.b.ins().iconst(types::I8, i64::from(flags.multiline));
        let enc = fx.b.ins().iconst(types::I8, enc_byte);
        (ic, ext, ml, enc)
    };
    let is_static = parts
        .iter()
        .all(|p| matches!(p, StrPart::Lit(_) | StrPart::Bytes(_)));
    if is_static {
        let mut source = String::new();
        for p in parts {
            match p {
                StrPart::Lit(s) => source.push_str(s),
                StrPart::Bytes(b) => source.push_str(&String::from_utf8_lossy(b)),
                StrPart::Interp(_) => unreachable!("static parts only"),
            }
        }
        let site = fx.em.mint_regexp_site();
        let site_v = fx.regexp_site_value(site);
        let off = fx.em.intern_rodata(source.as_bytes());
        let ptr = fx.rod(off);
        let len_v = fx.b.ins().iconst(fx.em.ptr, source.len() as i64);
        let (ic, ext, ml, enc) = flag_vals(fx);
        let ss = fx.temp_slot();
        let out = fx.slot_addr(ss, 0);
        let status = fx.call_status(
            "zeo_rt_regexp_lit",
            &[site_v, ptr, len_v, ic, ext, ml, enc, out],
        );
        fx.fallible(status);
        fx.owned_created += 1;
        return Ok(Operand::Slot {
            ss,
            owned: true,
            tag: TagInfo::Known(ValueTag::Regexp as u8),
        });
    }
    if !flags.once {
        let flag_args = flag_vals(fx);
        return interpolated_regexp(fx, parts, flag_args);
    }
    // `/o`: the site's object once built; the interpolations run only then.
    let site = fx.em.mint_regexp_site();
    let site_v = fx.regexp_site_value(site);
    let ss = fx.temp_slot();
    let dst = fx.slot_addr(ss, 0);
    let hit = fx.call_status("zeo_rt_regexp_once_get", &[site_v, dst]);
    let build = fx.b.create_block();
    let join = fx.b.create_block();
    fx.b.ins().brif(hit, join, &[], build, &[]);
    fx.b.switch_to_block(build);
    let flag_args = flag_vals(fx);
    let built = interpolated_regexp(fx, parts, flag_args)?;
    let p = ownership::borrow_ptr(fx, &built);
    fx.call("zeo_rt_regexp_once_put", &[site_v, p]);
    ownership::write_move_into(fx, &built, dst);
    fx.b.ins().jump(join, &[]);
    fx.b.switch_to_block(join);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Known(ValueTag::Regexp as u8),
    })
}

/// An interpolated pattern assembled exactly as string interpolation does
/// (pooled at creation; an interp piece can raise mid-build), then compiled
/// by `zeo_rt_regexp_interp`.
fn interpolated_regexp(
    fx: &mut Fx,
    parts: &[crate::hir::StrPart],
    (ic, ext, ml, enc): (
        cranelift_codegen::ir::Value,
        cranelift_codegen::ir::Value,
        cranelift_codegen::ir::Value,
        cranelift_codegen::ir::Value,
    ),
) -> CResult<Operand> {
    use crate::hir::StrPart;
    let ss_pat = fx.temp_slot();
    let pat = fx.slot_addr(ss_pat, 0);
    let null = fx.b.ins().iconst(fx.em.ptr, 0);
    let zero = fx.b.ins().iconst(fx.em.ptr, 0);
    let enc_utf8 = fx.b.ins().iconst(types::I8, ENC_UTF8);
    fx.call("zeo_rt_str_new", &[null, zero, enc_utf8, pat]);
    fx.owned_created += 1;
    ownership::pool_owned(fx, pat, TagInfo::Known(ValueTag::Str as u8));
    for part in parts {
        match part {
            StrPart::Lit(text) => {
                if text.is_empty() {
                    continue;
                }
                let off = fx.em.intern_rodata(text.as_bytes());
                let ptr = fx.rod(off);
                let len_v = fx.b.ins().iconst(fx.em.ptr, text.len() as i64);
                fx.call("zeo_rt_str_append_lit", &[pat, ptr, len_v]);
            }
            StrPart::Bytes(b) => {
                let text = String::from_utf8_lossy(b).into_owned();
                let off = fx.em.intern_rodata(text.as_bytes());
                let ptr = fx.rod(off);
                let len_v = fx.b.ins().iconst(fx.em.ptr, text.len() as i64);
                fx.call("zeo_rt_str_append_lit", &[pat, ptr, len_v]);
            }
            StrPart::Interp(n) => {
                let op = lower_expr(fx, *n)?;
                let p = ownership::borrow_ptr(fx, &op);
                if op.owned() {
                    ownership::pool_owned(fx, p, op.tag());
                }
                let status = fx.call_status("zeo_rt_str_append_value", &[pat, p]);
                fx.fallible(status);
            }
        }
    }
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx.call_status("zeo_rt_regexp_interp", &[pat, ic, ext, ml, enc, out]);
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Known(ValueTag::Regexp as u8),
    })
}

//! `attach_function` wrapper bodies: resolve the C symbol, marshal the
//! arguments, call, wrap the result.
//!
//! The symbol is resolved once per site: a `.bss` word (`zeo_ffi_sites`)
//! holds the address after the first call asked the runtime for it, so a
//! library named at build time and one opened at run time cost the same
//! load on every later call.
//!
//! Two call tiers. A fixed-signature call whose every position is a plain
//! C scalar is the DIRECT tier: the emitted code converts each argument
//! through one runtime row (ruby-ffi's own range checks and error texts
//! live there), then `call_indirect`s the declared C signature itself and
//! wraps the result inline. Everything else -- an enum, a callback, a
//! by-value struct, `:strptr`, varargs, `blocking:` -- is the LIBFFI tier:
//! one `.rodata` [`zeo_abi::abi::FfiCallC`] describing the whole signature,
//! one `zeo_rt_ffi_invoke` per call, which Cranelift cannot express (no
//! variadic calls, no aggregate ABI). Both answer identically; the tier is
//! a cost, never a behaviour.

use crate::diagnostics::clif::CResult;
use cranelift_codegen::ir::{InstBuilder, types};
use cranelift_module::Module;
use zeo_abi::ffi::CScalar;

use crate::hir::{FfiCall, FfiLib, FfiType, NodeId};

use super::ctx::Fx;
use super::expr::lower_expr;
use super::operand::{Operand, TagInfo};
use super::ownership;

mod direct;
mod markers;

pub(crate) use markers::{is_marker, marker_call};

/// One C type as the emitter records it for `.rodata` -- the `FfiTypeC`
/// tree with every compile-time decision already made (a platform typedef
/// resolved to the host's width, an inline array expanded to its elements).
pub(crate) enum TySpec {
    Scalar(CScalar),
    /// `(member name, value)` in declaration order.
    Enum(Vec<(String, i64)>),
    /// The runtime enum store the class body fills.
    EnumSlot(usize),
    /// `(C argument types, C return type)`.
    Callback(Vec<CScalar>, CScalar),
    /// `(fields in declaration order, byte size)`.
    Struct(Vec<TySpec>, usize),
    StrPtr,
}

/// One call site's whole C signature.
pub(crate) struct CallSpec {
    pub args: Vec<TySpec>,
    pub ret: TySpec,
    pub blocking: bool,
    pub variadic: bool,
}

/// The `attach_function` wrapper body: one C call.
pub(crate) fn lower_ffi_call(fx: &mut Fx, site: NodeId, call: &FfiCall) -> CResult<Operand> {
    let spec =
        call_spec(call).map_err(|e| e.with_span_if_missing(fx.an.compiler.hir.span(site)))?;
    let addr = resolve_symbol(fx, call);
    match direct::DirectSig::of(&spec) {
        Some(sig) => direct::lower(fx, call, &sig, addr),
        None => lower_libffi(fx, call, &spec, addr),
    }
}

/// The libffi tier: the whole signature in `.rodata`, the arguments in a
/// borrowed argv, one runtime call.
fn lower_libffi(
    fx: &mut Fx,
    call: &FfiCall,
    spec: &CallSpec,
    addr: cranelift_codegen::ir::Value,
) -> CResult<Operand> {
    // The arguments, in written order, into one contiguous slot array --
    // the same shape a dynamic send builds. Every marshaling decision is
    // already in the descriptor, so a coercion that raises inside the
    // runtime call strands no half-built argument list.
    let ids: Vec<NodeId> = call
        .args
        .iter()
        .map(|(id, _)| *id)
        .chain(call.variadic)
        .collect();
    let argv = build_argv(fx, &ids)?;
    let argc = fx.b.ins().iconst(fx.em.ptr, ids.len() as i64);

    let desc_id = super::statics::define_ffi_call(fx.em, spec)?;
    let desc_gv = fx.em.module.declare_data_in_func(desc_id, fx.b.func);
    let desc = fx.b.ins().symbol_value(fx.em.ptr, desc_gv);

    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx.call_status("zeo_rt_ffi_invoke", &[desc, addr, argv, argc, out]);
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// The C symbol's address for this site: the site's `.bss` word when a
/// call already filled it, else the runtime's resolver, whose answer is
/// stored there. A resolution failure raises and stores nothing, so the
/// next call asks again -- retrying a failed `require` re-raises too.
fn resolve_symbol(fx: &mut Fx, call: &FfiCall) -> cranelift_codegen::ir::Value {
    use cranelift_codegen::ir::MemFlagsData;
    use zeo_abi::abi::{FFISYM_SITE_ADDR, FFISYM_SITE_SIZE};
    let word = fx.em.mint_ffi_word();
    let gv = fx
        .em
        .module
        .declare_data_in_func(fx.em.ffi_sites_id, fx.b.func);
    let base = fx.b.ins().symbol_value(fx.em.ptr, gv);
    let slot = if word == 0 {
        base
    } else {
        fx.b.ins()
            .iadd_imm_u(base, i64::from(word) * FFISYM_SITE_SIZE as i64)
    };
    let cached = fx.b.ins().load(
        fx.em.ptr,
        MemFlagsData::trusted(),
        slot,
        FFISYM_SITE_ADDR as i32,
    );
    let resolve = fx.b.create_block();
    let join = fx.b.create_block();
    fx.b.append_block_param(join, fx.em.ptr);
    fx.b.set_cold_block(resolve);
    fx.b.ins()
        .brif(cached, join, &[cached.into()], resolve, &[]);
    fx.b.switch_to_block(resolve);
    let addr = resolve_symbol_slow(fx, call);
    fx.b.ins()
        .store(MemFlagsData::trusted(), addr, slot, FFISYM_SITE_ADDR as i32);
    fx.b.ins().jump(join, &[addr.into()]);
    fx.b.switch_to_block(join);
    fx.b.block_params(join)[0]
}

/// The runtime's resolver for one site, called on the cold edge.
fn resolve_symbol_slow(fx: &mut Fx, call: &FfiCall) -> cranelift_codegen::ir::Value {
    use zeo_abi::abi::{FFI_SYM_LIB, FFI_SYM_LIB_OR_PROCESS, FFI_SYM_PROCESS};
    let id = fx.em.mint_ffi_site();
    let site_v = fx.b.ins().iconst(types::I32, i64::from(id));
    let (sym_ptr, sym_len) = super::expr::rodata_name(fx, &call.symbol);
    // One pointer-sized, pointer-aligned out-slot for the resolved address.
    const PTR_SIZE: u32 = size_of::<usize>() as u32;
    const PTR_ALIGN_LOG2: u8 = PTR_SIZE.ilog2() as u8;
    let addr_ss =
        fx.b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            PTR_SIZE,
            PTR_ALIGN_LOG2,
        ));
    let addr_out = fx.slot_addr(addr_ss, 0);
    let status = match &call.lib {
        FfiLib::Deferred { slot } => {
            let slot_v = fx.b.ins().iconst(fx.em.ptr, *slot as i64);
            fx.call(
                "zeo_rt_ffi_sym_slot",
                &[site_v, slot_v, sym_ptr, sym_len, addr_out],
            )
        }
        lib => {
            let (names, mode): (Vec<&str>, u8) = match lib {
                FfiLib::Static(name) => (vec![name.as_str()], FFI_SYM_LIB_OR_PROCESS),
                FfiLib::Runtime(cands) => (cands.iter().map(String::as_str).collect(), FFI_SYM_LIB),
                FfiLib::None => (Vec::new(), FFI_SYM_PROCESS),
                FfiLib::Deferred { .. } => unreachable!("the deferred arm returned above"),
            };
            let (cands_ptr, n) = if names.is_empty() {
                let null = fx.b.ins().iconst(fx.em.ptr, 0);
                let zero = fx.b.ins().iconst(fx.em.ptr, 0);
                (null, zero)
            } else {
                super::statics::str_array(fx, &names)
            };
            let mode_v = fx.b.ins().iconst(types::I8, i64::from(mode));
            fx.call(
                "zeo_rt_ffi_sym",
                &[site_v, cands_ptr, n, sym_ptr, sym_len, mode_v, addr_out],
            )
        }
    }
    .expect("ffi_sym returns a status");
    fx.fallible(status);
    let at = fx.slot_addr(addr_ss, 0);
    fx.b.ins().load(
        fx.em.ptr,
        cranelift_codegen::ir::MemFlagsData::trusted(),
        at,
        0,
    )
}

/// A contiguous argv of BORROWED copies, owned temps pooled first --
/// `call::build_argv`'s shape over plain node ids.
pub(super) fn build_argv(fx: &mut Fx, ids: &[NodeId]) -> CResult<cranelift_codegen::ir::Value> {
    use cranelift_codegen::ir::{StackSlotData, StackSlotKind};
    use zeo_abi::abi::VALUE_SIZE;
    if ids.is_empty() {
        return Ok(fx.b.ins().iconst(fx.em.ptr, 0));
    }
    let ss = fx.b.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        ids.len() as u32 * VALUE_SIZE as u32,
        3,
    ));
    for (i, id) in ids.iter().enumerate() {
        let op = lower_expr(fx, *id)?;
        if op.owned() {
            let tag = op.tag();
            let addr = ownership::addr_of(fx, &op);
            ownership::pool_owned(fx, addr, tag);
        }
        let dst = fx.slot_addr(ss, (i * VALUE_SIZE) as i32);
        ownership::write_borrow(fx, &op, dst);
    }
    Ok(fx.slot_addr(ss, 0))
}

/// The whole signature as `.rodata` specs, or the refusal text for a shape
/// the descriptor cannot carry.
fn call_spec(call: &FfiCall) -> CResult<CallSpec> {
    Ok(CallSpec {
        args: call
            .args
            .iter()
            .map(|(_, ty)| ty_spec(ty))
            .collect::<Result<_, _>>()?,
        ret: ty_spec(&call.ret)?,
        blocking: call.blocking,
        variadic: call.variadic.is_some(),
    })
}

fn ty_spec(ty: &FfiType) -> CResult<TySpec> {
    Ok(match ty {
        FfiType::Enum(members) => TySpec::Enum(members.clone()),
        FfiType::EnumSlot(slot) => TySpec::EnumSlot(*slot),
        FfiType::Callback(args, ret) => TySpec::Callback(
            args.iter().map(scalar_of).collect::<Result<_, _>>()?,
            scalar_of(ret)?,
        ),
        FfiType::Struct(layout) => TySpec::Struct(
            layout
                .fields
                .iter()
                .map(|(_, t, _)| elems_of(t))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten()
                .collect(),
            layout.size,
        ),
        FfiType::StrPtr => TySpec::StrPtr,
        scalar => TySpec::Scalar(scalar_of(scalar)?),
    })
}

/// A by-value struct field's descriptor elements: an inline array expands
/// to its `count` elements, a nested struct recurses. No padding -- libffi
/// derives offsets from the same natural-alignment rules the recorded
/// layout's offsets came from.
fn elems_of(ty: &FfiType) -> CResult<Vec<TySpec>> {
    Ok(match ty {
        FfiType::Array(elem, count) => {
            let one = elems_of(elem)?;
            (0..*count)
                .flat_map(|_| one.iter().map(clone_spec).collect::<Vec<_>>())
                .collect()
        }
        FfiType::Struct(_) => vec![ty_spec(ty)?],
        scalar => vec![TySpec::Scalar(scalar_of(scalar)?)],
    })
}

/// A struct field spec repeated for each element of an inline array.
/// `TySpec` is not `Clone` (an enum member list has no business being
/// duplicated), and a struct FIELD is only ever a scalar or a nested
/// struct, so the two shapes an array element can take are all this needs.
fn clone_spec(t: &TySpec) -> TySpec {
    match t {
        TySpec::Scalar(s) => TySpec::Scalar(*s),
        TySpec::Struct(fields, size) => {
            TySpec::Struct(fields.iter().map(clone_spec).collect(), *size)
        }
        TySpec::Enum(_) | TySpec::EnumSlot(_) | TySpec::Callback(..) | TySpec::StrPtr => {
            unreachable!("a struct field descriptor is a scalar or a nested struct")
        }
    }
}

/// One type's C scalar. A platform typedef resolves to the width it has on
/// the HOST --
/// a Cranelift signature is decided here (`lower::ffi::platform_scalar_of`
/// is the same table struct layouts already resolve against).
fn scalar_of(ty: &FfiType) -> CResult<CScalar> {
    if let FfiType::PlatformScalar(name) = ty {
        return crate::lower::ffi::platform_scalar_of(name)
            .ok_or_else(|| cannot_lower(format!("the platform C typedef `{name}`")));
    }
    // A bare struct name in a signature is ruby-ffi's `StructByReference`,
    // whose ABI type is a pointer.
    if let FfiType::StructRef(_) = ty {
        return Ok(CScalar::Pointer);
    }
    ty.c_scalar()
        .ok_or_else(|| cannot_lower("this FFI type in a call position"))
}

/// The signature helpers' refusal: span-less here, stamped with the call
/// site's own by `lower_ffi_call`.
fn cannot_lower(what: impl std::fmt::Display) -> crate::diagnostics::clif::CodegenError {
    crate::diagnostics::clif::CodegenError::unsupported(
        format!("the CLIF backend cannot lower {what} yet"),
        None,
    )
}

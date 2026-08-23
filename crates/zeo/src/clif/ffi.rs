//! `attach_function` wrapper bodies: resolve the C symbol, marshal the
//! arguments, call, wrap the result.
//!
//! The rustc backend has four tiers (a fn-local `extern "C"` block for a
//! build-time-linkable library, a libffi call for a runtime-resolved one, a
//! variadic CIF, a `#[repr(C)]` mirror for a by-value aggregate). Cranelift
//! can declare neither a link directive nor an aggregate ABI, so EVERY tier
//! here goes through the runtime's libffi engine: one `.rodata`
//! [`zeo_abi::abi::FfiCallC`] describing the whole signature, one
//! `zeo_rt_ffi_invoke` per call. A library named at build time is simply
//! one whose symbol may also come from the process image -- which is what
//! linking it amounted to.
//!
//! A direct `call_indirect` on a declared C signature is the perf tier the
//! plan records as a lever; it changes no observable behaviour, so it does
//! not gate parity.

use cranelift_codegen::ir::{InstBuilder, types};
use cranelift_module::Module;
use zeo_abi::ffi::CScalar;

use crate::hir::{FfiCall, FfiLib, FfiType, NodeId};

use super::ctx::Fx;
use super::expr::lower_expr;
use super::operand::{Operand, TagInfo};
use super::ownership;

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
pub(crate) fn lower_ffi_call(fx: &mut Fx, site: NodeId, call: &FfiCall) -> Result<Operand, String> {
    let spec = match call_spec(call) {
        Ok(s) => s,
        Err(what) => return fx.unsupported(site, &what),
    };
    let addr = resolve_symbol(fx, call);

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

    let desc_id = super::statics::define_ffi_call(fx.em, &spec)?;
    let desc_gv = fx.em.module.declare_data_in_func(desc_id, fx.b.func);
    let desc = fx.b.ins().symbol_value(fx.em.ptr, desc_gv);

    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let status = fx
        .call("zeo_rt_ffi_invoke", &[desc, addr, argv, argc, out])
        .expect("ffi_invoke returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

/// The C symbol's address for this site, resolved once per site and cached
/// in the runtime (the CLIF twin of the rustc backend's `.bss`
/// `FfiSymSite`).
fn resolve_symbol(fx: &mut Fx, call: &FfiCall) -> cranelift_codegen::ir::Value {
    use zeo_abi::abi::{FFI_SYM_LIB, FFI_SYM_LIB_OR_PROCESS, FFI_SYM_PROCESS};
    let id = fx.em.mint_ffi_site();
    let site_v = fx.b.ins().iconst(types::I32, i64::from(id));
    let (sym_ptr, sym_len) = super::expr::rodata_name(fx, &call.symbol);
    let addr_ss =
        fx.b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            8,
            3,
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
fn build_argv(fx: &mut Fx, ids: &[NodeId]) -> Result<cranelift_codegen::ir::Value, String> {
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
fn call_spec(call: &FfiCall) -> Result<CallSpec, String> {
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

fn ty_spec(ty: &FfiType) -> Result<TySpec, String> {
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
fn elems_of(ty: &FfiType) -> Result<Vec<TySpec>, String> {
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
/// the HOST -- the rustc backend could leave it to the build machine, but
/// a Cranelift signature is decided here (`lower::ffi::platform_scalar_of`
/// is the same table struct layouts already resolve against).
fn scalar_of(ty: &FfiType) -> Result<CScalar, String> {
    if let FfiType::PlatformScalar(name) = ty {
        return crate::lower::ffi::platform_scalar_of(name)
            .ok_or_else(|| format!("the platform C typedef `{name}`"));
    }
    // A bare struct name in a signature is ruby-ffi's `StructByReference`,
    // whose ABI type is a pointer.
    if let FfiType::StructRef(_) = ty {
        return Ok(CScalar::Pointer);
    }
    ty.c_scalar()
        .ok_or_else(|| "this FFI type in a call position".to_string())
}

// ---------------------------------------------------------------------------
// The class-body markers `lower::ffi` desugars a deferred directive into
// ---------------------------------------------------------------------------

/// Is `name` one of the markers `lower::ffi` desugars a deferred
/// directive into? The names are reserved: no ruby source spells them.
pub(crate) fn is_marker(name: &str) -> bool {
    matches!(
        name,
        "__zeo_ffi_lib" | "__zeo_ffi_enum" | "__zeo_ffi_enum_get" | "__zeo_ffi_enum_put"
    )
}

/// One marker call, lowered where it stands.
pub(crate) fn marker_call(
    fx: &mut Fx,
    site: NodeId,
    name: &str,
    args: &[crate::hir::ArrayElem],
) -> Result<Operand, String> {
    let shape = || format!("the `{name}` marker in this shape");
    let ids: Option<Vec<NodeId>> = args
        .iter()
        .map(|a| match a {
            crate::hir::ArrayElem::Single(id) => Some(*id),
            crate::hir::ArrayElem::Splat(_) => None,
        })
        .collect();
    // `lower::ffi` writes every marker as a leading integer slot plus
    // plain arguments -- anything else is an internal error, not a program.
    let Some(ids) = ids else {
        return fx.unsupported(site, &shape());
    };
    let Some((&slot_id, rest)) = ids.split_first() else {
        return fx.unsupported(site, &shape());
    };
    let crate::hir::HirNode::IntegerLit(slot) = fx.an.compiler.hir[slot_id] else {
        return fx.unsupported(site, &shape());
    };
    let slot = slot as usize;
    match name {
        "__zeo_ffi_lib" if rest.len().is_multiple_of(2) => lib_store(fx, slot, rest),
        "__zeo_ffi_enum" => enum_store(fx, slot, rest),
        "__zeo_ffi_enum_get" | "__zeo_ffi_enum_put" if rest.len() == 1 => {
            enum_field(fx, slot, rest[0], name.ends_with("put"))
        }
        _ => fx.unsupported(site, &shape()),
    }
}

/// `__zeo_ffi_lib(slot, splatted?, expr, ...)`: the candidate expressions
/// evaluate here, in class-body order, and the runtime dlopens every value
/// EAGERLY -- so an unopenable library raises `LoadError` at this exact
/// statement, as CRuby's `ffi_lib` does.
fn lib_store(fx: &mut Fx, slot: usize, pairs: &[NodeId]) -> Result<Operand, String> {
    use cranelift_codegen::ir::{MemFlagsData, StackSlotData, StackSlotKind};
    let n = pairs.len() / 2;
    let splats = fx.b.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        n.max(1) as u32,
        0,
    ));
    let values: Vec<NodeId> = pairs.chunks_exact(2).map(|p| p[1]).collect();
    for (i, pair) in pairs.chunks_exact(2).enumerate() {
        let splatted = matches!(
            fx.an.compiler.hir[pair[0]],
            crate::hir::HirNode::IntegerLit(1)
        );
        let v = fx.b.ins().iconst(types::I8, i64::from(u8::from(splatted)));
        let at = fx.slot_addr(splats, i as i32);
        fx.b.ins().store(MemFlagsData::trusted(), v, at, 0);
    }
    let argv = build_argv(fx, &values)?;
    let splats_ptr = fx.slot_addr(splats, 0);
    let slot_v = fx.b.ins().iconst(fx.em.ptr, slot as i64);
    let n_v = fx.b.ins().iconst(fx.em.ptr, n as i64);
    call_out(fx, "zeo_rt_ffi_lib_store", &[slot_v, argv, splats_ptr, n_v])
}

/// `__zeo_ffi_enum(slot, member, ...)`: the member list evaluates here and
/// lands in the slot every signature lowered under it reads.
fn enum_store(fx: &mut Fx, slot: usize, members: &[NodeId]) -> Result<Operand, String> {
    let argv = build_argv(fx, members)?;
    let slot_v = fx.b.ins().iconst(fx.em.ptr, slot as i64);
    let n_v = fx.b.ins().iconst(fx.em.ptr, members.len() as i64);
    call_out(fx, "zeo_rt_ffi_enum_store", &[slot_v, argv, n_v])
}

/// A deferred enum STRUCT FIELD's read (`int` -> Symbol) or write.
fn enum_field(fx: &mut Fx, slot: usize, value: NodeId, put: bool) -> Result<Operand, String> {
    let op = lower_expr(fx, value)?;
    let ptr = ownership::borrow_ptr(fx, &op);
    if op.owned() {
        let tag = op.tag();
        ownership::pool_owned(fx, ptr, tag);
    }
    let slot_v = fx.b.ins().iconst(fx.em.ptr, slot as i64);
    let put_v = fx.b.ins().iconst(types::I8, i64::from(u8::from(put)));
    call_out(fx, "zeo_rt_ffi_enum_field", &[slot_v, put_v, ptr])
}

/// A fallible runtime call whose last argument is the `out` slot.
fn call_out(
    fx: &mut Fx,
    name: &'static str,
    args: &[cranelift_codegen::ir::Value],
) -> Result<Operand, String> {
    let ss = fx.temp_slot();
    let out = fx.slot_addr(ss, 0);
    let mut all = args.to_vec();
    all.push(out);
    let status = fx.call(name, &all).expect("the entry returns a status");
    fx.fallible(status);
    fx.owned_created += 1;
    Ok(Operand::Slot {
        ss,
        owned: true,
        tag: TagInfo::Unknown,
    })
}

//! The single table of runtime symbols emitted code imports, with their
//! C signatures -- what `emit` declares imports from, and what the
//! `capi_surface` test (M0-16) asserts against `zeo-rt`'s exports. A
//! symbol used by any lowering MUST come from here; a name the runtime
//! does not export fails the link, and this table is where that name is
//! grepped for.

/// A C parameter/return slot, mapped to a Cranelift type by `emit`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CTy {
    /// Any pointer (`*const`/`*mut`) -- the target's pointer type.
    Ptr,
    /// `usize` -- pointer-width integer.
    Usize,
    /// `i32` (the status protocol, C `int`).
    I32,
    /// `u32` (class/symbol ids, line numbers).
    U32,
    /// `u8` (encoding ids, flag bytes) -- unsigned-extended per the C ABI.
    U8,
    /// `i8` (boolean answers) -- sign-extended per the C ABI.
    I8,
    /// `f64` (the Float operator slow paths take unboxed operands).
    F64,
}

/// One imported runtime function: its exact exported name and C shape.
pub struct CapiSig {
    pub name: &'static str,
    pub params: &'static [CTy],
    pub ret: Option<CTy>,
}

use CTy::{F64, I8, I32, Ptr, U8, U32, Usize};

/// Every runtime symbol the emitter can import, alphabetical by name.
/// Grows with the lowerings; `sig` panics on a name not listed -- an
/// internal compiler error, never a user-visible path.
pub const CAPI: &[CapiSig] = &[
    CapiSig {
        name: "zeo_rt_alias_in_default_definee",
        params: &[Ptr, Ptr, Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_array_get",
        params: &[Ptr, Usize, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_array_len",
        params: &[Ptr],
        ret: Some(Usize),
    },
    CapiSig {
        name: "zeo_rt_array_new",
        params: &[Usize, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_array_push",
        params: &[Ptr, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_array_push_splat",
        params: &[Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_bind_block_params",
        params: &[Ptr, U8, Ptr, Usize, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_bind_params",
        params: &[Ptr, Ptr, Usize, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_binding_new",
        params: &[Ptr, Ptr, Ptr, Usize, Ptr, Usize, U32, U32, U32, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_block_arg_to_proc",
        params: &[Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_call_singleton_super_target_args",
        params: &[U32, U8, U32, U32, Ptr, U8, Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_callsite_init",
        params: &[Ptr, U32],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_case_eq",
        params: &[Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_case_eq_any",
        params: &[Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_cell_load",
        params: &[Ptr, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_cell_new",
        params: &[Ptr],
        ret: Some(Ptr),
    },
    CapiSig {
        name: "zeo_rt_cell_release",
        params: &[Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_cell_store",
        params: &[Ptr, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_check_ints",
        params: &[],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_class_new_instance",
        params: &[U32, Ptr, Usize, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_class_of",
        params: &[Ptr],
        ret: Some(U32),
    },
    CapiSig {
        name: "zeo_rt_classmethod_site_init",
        params: &[Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_complex_lit",
        params: &[Ptr, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_conceal_class",
        params: &[U32],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_conditional_class_ref",
        params: &[U32, Ptr, Usize, U32, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_const_get_at",
        params: &[U32, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_const_get_cref",
        params: &[Ptr, Usize, Ptr, Usize, Ptr, Usize, U8, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_const_get_on_value",
        params: &[Ptr, Ptr, Usize, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_const_get_or_nil",
        params: &[U32, Ptr, Usize, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_const_get_scoped",
        params: &[U32, Ptr, Usize, Ptr, Usize, U8, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_const_private",
        params: &[U32, Ptr, Usize],
        ret: Some(I8),
    },
    CapiSig {
        name: "zeo_rt_const_set_at",
        params: &[U32, Ptr, Usize, Ptr, Ptr, Usize, U32],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_cov_file_loaded",
        params: &[Ptr, Usize],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_cov_line",
        params: &[Ptr, Usize, U32],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_cvar_get",
        params: &[U32, Ptr, Usize, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_cvar_get_checked",
        params: &[U32, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_cvar_set",
        params: &[U32, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_define_in_default_definee",
        params: &[Ptr, Ptr, U32, Ptr, U8, U8, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_defined_const_in",
        params: &[U32, Ptr, Usize],
        ret: Some(I8),
    },
    CapiSig {
        name: "zeo_rt_defined_cvar",
        params: &[U32, Ptr, Usize],
        ret: Some(I8),
    },
    CapiSig {
        name: "zeo_rt_defined_gvar",
        params: &[U32, Ptr, Usize],
        ret: Some(I8),
    },
    CapiSig {
        name: "zeo_rt_defined_ivar",
        params: &[Ptr, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_defined_method",
        params: &[Ptr, U32, U8, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_eval_define",
        params: &[U8, Ptr, U32, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_eval_value_in_scope",
        params: &[Ptr, Ptr, Ptr, Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_ffi_enum_field",
        params: &[Usize, U8, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_ffi_enum_store",
        params: &[Usize, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_ffi_invoke",
        params: &[Ptr, Usize, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_ffi_lib_store",
        params: &[Usize, Ptr, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_ffi_sym",
        params: &[U32, Ptr, Usize, Ptr, Usize, U8, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_ffi_sym_slot",
        params: &[U32, Usize, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_flip_flop_on",
        params: &[U32],
        ret: Some(U8),
    },
    CapiSig {
        name: "zeo_rt_flip_flop_set",
        params: &[U32, U8],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_float_cmp",
        params: &[F64, F64, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_float_mod_checked",
        params: &[F64, F64, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_float_pow_checked",
        params: &[F64, F64, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_for_begin",
        params: &[Ptr, U8, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_for_end",
        params: &[Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_for_next",
        params: &[Ptr, Ptr, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_for_result",
        params: &[Ptr, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_frame_pop",
        params: &[],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_frame_push",
        params: &[Ptr, Usize, Ptr, Usize, U32, U32],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_frozen_check",
        params: &[Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_guard_class_reopen",
        params: &[U32, Ptr, Usize],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_gvar_alias",
        params: &[U32, Ptr, Usize, Ptr, Usize],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_gvar_assign",
        params: &[U32, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_gvar_child_status",
        params: &[Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_gvar_err_info",
        params: &[Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_gvar_get",
        params: &[U32, Ptr, Usize, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_handling_pop",
        params: &[],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_handling_push",
        params: &[Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_hash_new",
        params: &[Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_hash_set",
        params: &[Ptr, Ptr, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_home_pop",
        params: &[],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_home_push",
        params: &[],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_int_add_slow",
        params: &[Ptr, Ptr, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_int_digits",
        params: &[U8, Ptr, Ptr, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_int_div",
        params: &[Ptr, Ptr, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_int_mod",
        params: &[Ptr, Ptr, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_int_mul_slow",
        params: &[Ptr, Ptr, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_int_pow",
        params: &[Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_int_shl",
        params: &[Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_int_shr",
        params: &[Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_int_sub_slow",
        params: &[Ptr, Ptr, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_iter_inline_ok_for",
        params: &[U32, U32],
        ret: Some(I8),
    },
    CapiSig {
        name: "zeo_rt_ivar_get_dyn",
        params: &[Ptr, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_ivar_get_slot",
        params: &[Ptr, Usize, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_ivar_set_dyn",
        params: &[Ptr, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_ivar_set_slot",
        params: &[Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_kernel_puts",
        params: &[Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_kw_splat_into",
        params: &[Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_last_match_ref",
        params: &[U8, Usize, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_main",
        params: &[I32, Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_main_object",
        params: &[Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_method_capture_inherited",
        params: &[Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_multi_split",
        params: &[Ptr, Usize, U8, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_object_new_sentinel",
        params: &[Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_pat_array_get",
        params: &[Ptr, Usize, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_pat_array_len",
        params: &[Ptr],
        ret: Some(Usize),
    },
    CapiSig {
        name: "zeo_rt_pat_array_slice",
        params: &[Ptr, Usize, Usize, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_pat_deconstruct",
        params: &[Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_pat_deconstruct_keys",
        params: &[Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_pat_fail_case_eq",
        params: &[Ptr, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_pat_fail_find",
        params: &[Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_pat_fail_guard",
        params: &[],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_pat_fail_length",
        params: &[Ptr, Usize, U8],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_pat_fail_not_empty",
        params: &[Ptr, U8],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_pat_hash_except",
        params: &[Ptr, Ptr, Usize, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_pat_hash_get",
        params: &[Ptr, U32, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_pat_hash_has_key",
        params: &[Ptr, U32],
        ret: Some(U8),
    },
    CapiSig {
        name: "zeo_rt_pat_hash_len",
        params: &[Ptr],
        ret: Some(Usize),
    },
    CapiSig {
        name: "zeo_rt_pat_is_a",
        params: &[Ptr, U32],
        ret: Some(U8),
    },
    CapiSig {
        name: "zeo_rt_pat_key_miss_clear",
        params: &[],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_pat_key_miss_record",
        params: &[U32, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_pat_match_error",
        params: &[Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_pat_match_error_bare",
        params: &[Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_pending_defs_begin",
        params: &[U32, Ptr, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_pending_defs_end",
        params: &[],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_pool_mark",
        params: &[],
        ret: Some(Usize),
    },
    CapiSig {
        name: "zeo_rt_pool_push",
        params: &[Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_pool_reset",
        params: &[Usize],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_proc_call_or_send",
        params: &[Ptr, U32, Ptr, Usize, Ptr, U32, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_proc_new",
        params: &[
            Ptr, Ptr, Usize, Ptr, Ptr, Ptr, I32, U32, Ptr, Usize, Ptr, Usize, U32, Ptr, Usize, Ptr,
        ],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_propagating_enter",
        params: &[Ptr],
        ret: Some(U8),
    },
    CapiSig {
        name: "zeo_rt_propagating_leave",
        params: &[U8],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_ractor_new",
        params: &[Ptr, Ptr, Usize, Ptr, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_raise_error",
        params: &[U32, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_raise_private_constant",
        params: &[U32, Ptr, Usize, Ptr, Usize],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_raise_with_explicit_cause",
        params: &[Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_range_new",
        params: &[Ptr, Ptr, U8, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_rational_digits",
        params: &[U8, Ptr, Ptr, Ptr, Ptr, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_record_const_location",
        params: &[U32, Ptr, Usize, Ptr, Usize, U32],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_refined_send_in",
        params: &[U32, Ptr, U32, Ptr, Usize, Ptr, Ptr, Ptr, Usize, U8, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_reflect_dispatch_in",
        params: &[U32, Ptr, U8, Ptr, Usize, Ptr, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_regexp_interp",
        params: &[Ptr, U8, U8, U8, U8, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_regexp_lit",
        params: &[U32, Ptr, Usize, U8, U8, U8, U8, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_release",
        params: &[Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_rescue_matches",
        params: &[Ptr, Ptr, Usize],
        ret: Some(U8),
    },
    CapiSig {
        name: "zeo_rt_rescue_matches_any",
        params: &[Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_retain",
        params: &[Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_return_targets_here",
        params: &[],
        ret: Some(U8),
    },
    CapiSig {
        name: "zeo_rt_reveal_class",
        params: &[U32],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_runtime_class_method_visibility",
        params: &[U32, U32, U8],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_runtime_replace_method",
        params: &[U32, Ptr, Usize, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_runtime_set_visibility",
        params: &[U32, U32, U8],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_runtime_undef_class_method",
        params: &[U32, U32],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_scope_const_defined",
        params: &[Ptr, Ptr, Usize],
        ret: Some(I8),
    },
    CapiSig {
        name: "zeo_rt_scope_const_get",
        params: &[Ptr, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_scope_const_get_or_nil",
        params: &[Ptr, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_scope_const_set",
        params: &[Ptr, Ptr, Usize, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_send_class_cached",
        params: &[Ptr, U32, Ptr, U32, Ptr, Usize, Ptr, U32, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_send_super_class_from_args",
        params: &[U32, U32, U32, Ptr, U8, Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_send_super_dynamic_args",
        params: &[Ptr, Ptr, U8, Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_send_super_from_args",
        params: &[Ptr, U32, U32, Ptr, U8, Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_send_value_args_in",
        params: &[U32, Ptr, U32, Ptr, U8, Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_send_value_cached",
        params: &[Ptr, U32, Ptr, U32, Ptr, Usize, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_send_value_explicit_args_in",
        params: &[U32, Ptr, U32, Ptr, U8, Ptr, Ptr, U32, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_send_value_explicit_in",
        params: &[U32, Ptr, U32, Ptr, Usize, Ptr, U32, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_send_value_explicit_kw_in",
        params: &[U32, Ptr, U32, Ptr, Usize, Ptr, Ptr, U32, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_send_value_in",
        params: &[U32, Ptr, U32, Ptr, Usize, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_send_value_kw_in",
        params: &[U32, Ptr, U32, Ptr, Usize, Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_send_value_vcall_in",
        params: &[U32, Ptr, U32, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_set_line",
        params: &[U32],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_signal_drop",
        params: &[Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_signal_kind",
        params: &[],
        ret: Some(U8),
    },
    CapiSig {
        name: "zeo_rt_signal_restore",
        params: &[Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_signal_save",
        params: &[],
        ret: Some(Ptr),
    },
    CapiSig {
        name: "zeo_rt_signal_set",
        params: &[U8, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_signal_take",
        params: &[Ptr],
        ret: Some(U8),
    },
    CapiSig {
        name: "zeo_rt_stack_check",
        params: &[],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_str_append_bytes",
        params: &[Ptr, Ptr, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_str_append_lit",
        params: &[Ptr, Ptr, Usize],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_str_append_value",
        params: &[Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_str_lit",
        params: &[Ptr, Ptr, U8, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_str_new",
        params: &[Ptr, Usize, U8, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_super_defined",
        params: &[Ptr, U32, U32],
        ret: Some(I8),
    },
    CapiSig {
        name: "zeo_rt_svar_scope_pop",
        params: &[],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_svar_scope_push",
        params: &[],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_sym_intern",
        params: &[Ptr, Usize],
        ret: Some(U32),
    },
    CapiSig {
        name: "zeo_rt_sym_value",
        params: &[U32, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_truthy",
        params: &[Ptr],
        ret: Some(U8),
    },
    CapiSig {
        name: "zeo_rt_validate_class_aliases",
        params: &[U32],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_value_super_args",
        params: &[Ptr, Ptr, Usize, Ptr, U8, Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_wrong_arity",
        params: &[Usize, Usize, Usize],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_yield",
        params: &[Ptr, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_yield_args",
        params: &[Ptr, Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
];

/// The signature row for `name`.
pub fn sig(name: &str) -> &'static CapiSig {
    CAPI.iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("capi_names: no signature row for {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_sorted_and_unique() {
        for pair in CAPI.windows(2) {
            assert!(
                pair[0].name < pair[1].name,
                "{} must sort before {}",
                pair[0].name,
                pair[1].name
            );
        }
    }
}

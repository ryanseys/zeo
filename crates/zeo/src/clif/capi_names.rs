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
}

/// One imported runtime function: its exact exported name and C shape.
pub struct CapiSig {
    pub name: &'static str,
    pub params: &'static [CTy],
    pub ret: Option<CTy>,
}

use CTy::{I32, Ptr, U8, U32, Usize};

/// Every runtime symbol the emitter can import, alphabetical by name.
/// Grows with the lowerings; `sig` panics on a name not listed -- an
/// internal compiler error, never a user-visible path.
pub const CAPI: &[CapiSig] = &[
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
        name: "zeo_rt_block_arg_to_proc",
        params: &[Ptr, Ptr],
        ret: Some(I32),
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
        name: "zeo_rt_const_get_at",
        params: &[U32, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_const_get_cref",
        params: &[Ptr, Usize, Ptr, Usize, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_const_get_scoped",
        params: &[U32, Ptr, Usize, Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_const_set_at",
        params: &[U32, Ptr, Usize, Ptr, Ptr, Usize, U32],
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
        name: "zeo_rt_int_mul_slow",
        params: &[Ptr, Ptr, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_int_sub_slow",
        params: &[Ptr, Ptr, Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_ivar_get_slot",
        params: &[Ptr, Usize, Ptr],
        ret: None,
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
        name: "zeo_rt_multi_split",
        params: &[Ptr, Usize, U8, Usize, Ptr],
        ret: Some(I32),
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
        name: "zeo_rt_proc_new",
        params: &[Ptr, Ptr, Usize, Ptr, Ptr, Ptr, I32, U32, Ptr],
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
        name: "zeo_rt_range_new",
        params: &[Ptr, Ptr, U8, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_record_const_location",
        params: &[U32, Ptr, Usize, Ptr, Usize, U32],
        ret: None,
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
        name: "zeo_rt_send_value_args_in",
        params: &[U32, Ptr, U32, Ptr, U8, Ptr, Ptr, Ptr],
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
        name: "zeo_rt_str_new",
        params: &[Ptr, Usize, U8, Ptr],
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
        name: "zeo_rt_wrong_arity",
        params: &[Usize, Usize, Usize],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_yield",
        params: &[Ptr, Ptr, Usize, Ptr],
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

//! The in-process capi symbol table: every exported `zeo_rt_*` function as
//! a `(name, address)` row. The JIT run path resolves emitted code's
//! imports here -- the `zeo` binary does not export these symbols, so
//! `dlsym` cannot (see the M0-8 notes) -- and the table is what keeps
//! "exists in the runtime" and "resolvable by the JIT" the same fact: a
//! capi function missing a row fails the emitter's coverage test, not a
//! user's program.

use super::{
    bind, dispatch, frames, kernel, lifecycle, literals, numeric, objects, procs, registry,
    signals, values,
};

macro_rules! capi_symbols {
    ($($m:ident :: $f:ident),* $(,)?) => {
        /// Every exported capi symbol, alphabetical (tested).
        pub const NAMES: &[&str] = &[$(stringify!($f)),*];

        /// The in-process address of capi symbol `name`.
        #[must_use]
        pub fn addr(name: &str) -> Option<*const u8> {
            match name {
                $(stringify!($f) => Some($m::$f as *const u8),)*
                _ => None,
            }
        }
    };
}

capi_symbols!(
    literals::zeo_rt_array_get,
    literals::zeo_rt_array_len,
    literals::zeo_rt_array_new,
    literals::zeo_rt_array_push,
    dispatch::zeo_rt_array_push_splat,
    lifecycle::zeo_rt_at_exit_register,
    values::zeo_rt_bignum_from_decimal,
    bind::zeo_rt_bind_block_params,
    bind::zeo_rt_bind_params,
    procs::zeo_rt_block_arg_to_proc,
    dispatch::zeo_rt_callsite_init,
    dispatch::zeo_rt_case_eq,
    dispatch::zeo_rt_case_eq_any,
    procs::zeo_rt_cell_load,
    procs::zeo_rt_cell_new,
    procs::zeo_rt_cell_release,
    procs::zeo_rt_cell_retain,
    procs::zeo_rt_cell_store,
    frames::zeo_rt_check_ints,
    objects::zeo_rt_class_new_instance,
    values::zeo_rt_class_of,
    dispatch::zeo_rt_classmethod_site_init,
    objects::zeo_rt_const_get_at,
    objects::zeo_rt_const_get_cref,
    objects::zeo_rt_const_get_scoped,
    objects::zeo_rt_const_set_at,
    objects::zeo_rt_cvar_get,
    objects::zeo_rt_cvar_get_checked,
    objects::zeo_rt_cvar_set,
    values::zeo_rt_eq,
    frames::zeo_rt_frame_pop,
    frames::zeo_rt_frame_push,
    objects::zeo_rt_frozen_check,
    objects::zeo_rt_gvar_assign,
    objects::zeo_rt_gvar_child_status,
    objects::zeo_rt_gvar_err_info,
    objects::zeo_rt_gvar_get,
    signals::zeo_rt_handling_pop,
    signals::zeo_rt_handling_push,
    literals::zeo_rt_hash_new,
    literals::zeo_rt_hash_set,
    signals::zeo_rt_home_pop,
    signals::zeo_rt_home_push,
    values::zeo_rt_inspect_to_stderr,
    numeric::zeo_rt_int_add_slow,
    numeric::zeo_rt_int_cmp_slow,
    numeric::zeo_rt_int_div,
    numeric::zeo_rt_int_mod,
    numeric::zeo_rt_int_mul_slow,
    numeric::zeo_rt_int_sub_slow,
    values::zeo_rt_is_a,
    objects::zeo_rt_ivar_get_dyn,
    objects::zeo_rt_ivar_get_slot,
    objects::zeo_rt_ivar_set_dyn,
    objects::zeo_rt_ivar_set_slot,
    kernel::zeo_rt_kernel_p,
    kernel::zeo_rt_kernel_print,
    kernel::zeo_rt_kernel_puts,
    dispatch::zeo_rt_kw_splat_into,
    literals::zeo_rt_last_match_ref,
    lifecycle::zeo_rt_main,
    registry::zeo_rt_main_object,
    literals::zeo_rt_multi_split,
    objects::zeo_rt_object_alloc,
    values::zeo_rt_pool_mark,
    values::zeo_rt_pool_push,
    values::zeo_rt_pool_reset,
    procs::zeo_rt_proc_call,
    procs::zeo_rt_proc_new,
    signals::zeo_rt_propagating_enter,
    signals::zeo_rt_propagating_leave,
    signals::zeo_rt_raise_error,
    literals::zeo_rt_range_new,
    objects::zeo_rt_record_const_location,
    literals::zeo_rt_regexp_interp,
    literals::zeo_rt_regexp_lit,
    values::zeo_rt_release,
    signals::zeo_rt_report_uncaught,
    signals::zeo_rt_rescue_matches,
    signals::zeo_rt_rescue_matches_any,
    values::zeo_rt_retain,
    signals::zeo_rt_return_targets_here,
    dispatch::zeo_rt_send_class_cached,
    dispatch::zeo_rt_send_super_from_args,
    dispatch::zeo_rt_send_value_args_in,
    dispatch::zeo_rt_send_value_cached,
    dispatch::zeo_rt_send_value_explicit_args_in,
    dispatch::zeo_rt_send_value_explicit_in,
    dispatch::zeo_rt_send_value_explicit_kw_in,
    dispatch::zeo_rt_send_value_in,
    dispatch::zeo_rt_send_value_kw_in,
    frames::zeo_rt_set_line,
    signals::zeo_rt_signal_drop,
    signals::zeo_rt_signal_kind,
    signals::zeo_rt_signal_restore,
    signals::zeo_rt_signal_save,
    signals::zeo_rt_signal_set,
    signals::zeo_rt_signal_take,
    frames::zeo_rt_stack_check,
    frames::zeo_rt_stamp_backtrace,
    literals::zeo_rt_str_append_lit,
    literals::zeo_rt_str_append_value,
    literals::zeo_rt_str_lit,
    literals::zeo_rt_str_new,
    signals::zeo_rt_svar_scope_pop,
    signals::zeo_rt_svar_scope_push,
    literals::zeo_rt_sym_intern,
    literals::zeo_rt_sym_value,
    frames::zeo_rt_synthetic_c_frame_pop,
    frames::zeo_rt_synthetic_c_frame_push,
    signals::zeo_rt_system_exit_status,
    frames::zeo_rt_trace_frame_self,
    values::zeo_rt_truthy,
    lifecycle::zeo_rt_validate_class_aliases,
    dispatch::zeo_rt_value_super_args,
    signals::zeo_rt_wrong_arity,
    procs::zeo_rt_yield,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_sorted_and_unique() {
        for pair in NAMES.windows(2) {
            assert!(
                pair[0] < pair[1],
                "capi symbol table out of order: {} then {}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn every_name_resolves() {
        for name in NAMES {
            assert!(addr(name).is_some(), "{name} has no address");
        }
        assert!(addr("zeo_rt_no_such_symbol").is_none());
    }
}

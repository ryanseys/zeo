//! The FFI directive family (the real `ffi` gem's idioms): `extend
//! FFI::Library` recognition, `ffi_lib`/`typedef`/`enum`/`callback`/
//! `attach_function` lowering, the `class < FFI::Struct` `layout` ->
//! accessor-method synthesis, and the C type-name mapping shared by both.
//!
//! - [`recognize`]: pure AST-shape matching, no HIR emission;
//! - [`directive`]: `lower_ffi_directive` and its two arms (lib-name
//!   resolution, `attach_function`);
//! - [`synth`]: `FFI::Struct` layout -> accessor synthesis;
//! - [`types`]: keyword -> `FfiType` resolution.

mod directive;
mod recognize;
mod synth;
mod types;

pub(crate) use directive::{as_ffi_layout, ffi_const_int, lower_ffi_directive};
pub(crate) use recognize::{
    as_global_ffi_typedef, const_path_string, extend_target_path, ffi_extender_hook,
    ffi_layout_hook, ffi_type_constant_of, is_extend_ffi_data_converter, is_extend_ffi_library,
    native_type_of, platform_scalar_of, prescan_declaration, prescan_layout,
};
pub(crate) use synth::{ffi_struct_layout, needs_inline_array_classes, synthesize_ffi_struct};
pub(crate) use types::ffi_type_of;

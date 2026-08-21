//! The Cranelift backend: HIR -> CLIF -> object files, linked against the
//! prebuilt `libzeo.a` (plan decision 1). Grows milestone by milestone;
//! the M0 slice covers literals, locals (slots and capture cells),
//! numeric ops with the dynamic-send fallback, `if`/`while`/fused
//! literal iterators, methods (direct calls, trampolines, dynamic
//! sends), plain user classes with slot ivars and accessors,
//! `begin/rescue/else/ensure/retry`, and escaping blocks with
//! `yield`/`break`/`next`. Everything outside the slice refuses loudly
//! with its source location -- the compile contract is CRuby-identical
//! or FAIL, never a silent drop.

pub(crate) mod blocks;
pub(crate) mod call;
pub mod capi_names;
pub(crate) mod classes;
pub(crate) mod control;
pub(crate) mod ctx;
pub(crate) mod debuginfo;
pub mod emit;
pub(crate) mod eval;
pub(crate) mod expr;
pub(crate) mod ffi;
pub(crate) mod iter;
pub mod names;
pub(crate) mod operand;
pub(crate) mod ownership;
pub(crate) mod params;
pub(crate) mod patterns;
pub(crate) mod refine;
pub(crate) mod statics;
pub(crate) mod stmt;
pub(crate) mod verify;

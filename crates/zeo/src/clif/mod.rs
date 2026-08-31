//! The Cranelift backend: HIR -> CLIF -> native code, run in-process
//! (JIT) or emitted as object files linked against the prebuilt
//! `libzeo.a` (AOT). The one backend zeo has. Anything the emitter
//! cannot express refuses loudly with its source location -- the
//! compile contract is CRuby-identical or FAIL, never a silent drop.

pub(crate) mod binop;
pub(crate) mod blocks;
pub(crate) mod body;
pub(crate) mod boxes;
pub(crate) mod call;
pub mod capi_names;
pub(crate) mod classes;
pub(crate) mod collect;
pub(crate) mod consts;
pub(crate) mod control;
pub(crate) mod ctx;
pub(crate) mod debuginfo;
pub(crate) mod defined;
pub mod emit;
pub(crate) mod eval;
pub(crate) mod expr;
pub(crate) mod ffi;
pub(crate) mod frames;
pub(crate) mod iter;
pub(crate) mod ivars;
pub(crate) mod module;
pub(crate) mod multi;
pub mod names;
pub(crate) mod operand;
pub(crate) mod ownership;
pub(crate) mod params;
pub(crate) mod patterns;
pub(crate) mod pkg;
pub(crate) mod refine;
pub(crate) mod statics;
pub(crate) mod stmt;
pub(crate) mod verify;

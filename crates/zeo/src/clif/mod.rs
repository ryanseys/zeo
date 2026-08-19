//! The Cranelift backend: HIR -> CLIF -> object files, linked against the
//! prebuilt `libzeo.a` (plan decision 1). Grows milestone by milestone; at
//! M0-9 it lowers exactly the hello slice (a top level of literal-string
//! `puts` statements) end to end, refusing everything else loudly -- the
//! full statement/expression lowering lands as `ctx`/`stmt`/`expr` modules.

pub(crate) mod call;
pub mod capi_names;
pub(crate) mod ctx;
pub mod emit;
pub(crate) mod expr;
pub mod names;
pub(crate) mod operand;
pub(crate) mod ownership;
pub(crate) mod params;
pub(crate) mod statics;
pub(crate) mod stmt;
pub(crate) mod verify;

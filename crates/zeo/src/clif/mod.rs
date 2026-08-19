//! The Cranelift backend: HIR -> CLIF -> object files, linked against the
//! prebuilt `libzeo.a` (plan decision 1). Grows milestone by milestone; at
//! M0-9 it lowers exactly the hello slice (a top level of literal-string
//! `puts` statements) end to end, refusing everything else loudly -- the
//! full statement/expression lowering lands as `ctx`/`stmt`/`expr` modules.

pub mod capi_names;
pub mod emit;
pub mod names;

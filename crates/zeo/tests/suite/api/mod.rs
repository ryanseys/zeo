//! Everything that drives the compiler as a library or the CLI in more than
//! one step: package workflows, the program cache, gem stores, C extension
//! builds, the link ledger, debug info.

mod autopkg;
mod backend_clif;
mod bundler_parity;
mod capi_sweep;
mod cext_build;
mod cext_data_object;
mod classes;
mod concurrency;
mod debuginfo;
mod enumerable;
mod eval;
mod eval_prepare;
mod exceptions;
mod ffi;
mod gem_install;
mod gems_all;
mod gems_require;
mod gems_vendored;
mod install;
mod io_files;
mod language_core;
mod linkage;
mod metaprog;
mod packages;
mod pure_gems;
mod variables;

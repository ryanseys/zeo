//! End-to-end tests via `support::run_ruby` (see its module docs). Each
//! snippet is compiled and run -- a spawned JIT `zeo` child by default, a
//! linked binary under `ZEO_E2E_BACKEND=aot` -- and stdout/stderr/exit
//! status are asserted separately, plus an in-process negative-path tier
//! (`compile_project(...).unwrap_err()`) for compile errors. The
//! `tests/*.rb` example programs with `.rb.expected` remain as the
//! ruby-oracle golden suite (`cargo test --test examples`); this is the
//! default place to add coverage.
//!
//! Split by subject into the sibling modules below. This is ONE test binary --
//! a `tests/e2e/` directory with `main.rs`, not a file per subject -- so the
//! split costs no extra compiling or linking. It exists purely so a test can be
//! found, and added next to its neighbours, in what was a single 21,000-line
//! file.

#[path = "../harness/e2e_support.rs"]
mod support;
#[path = "../harness/paths.rs"]
mod paths;
#[path = "../harness/zeo_bin.rs"]
mod zeo_bin;

mod blocks_procs;
mod boxes;
mod bundler_parity;
mod cext_build;
mod classes;
mod codegen;
mod collections;
mod concurrency;
mod control_flow;
mod debuginfo;
mod default_gem_store;
mod enumerable;
mod etc;
mod eval;
mod exceptions;
mod ffi;
mod gc_weak;
mod gem_install;
mod gems_all;
mod gems_require;
mod gems_vendored;
mod install;
mod io_files;
mod language_core;
mod linkage;
mod marshal;
mod metaprog;
mod methods;
mod numerics;
mod object_protocol;
mod pattern_match;
mod process_exit;
mod regexp;
mod securerandom;
mod strings;
mod time;
mod variables;

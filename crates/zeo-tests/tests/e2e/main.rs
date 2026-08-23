//! In-process end-to-end tests via `support::run_ruby` (see its module docs).
//! Compiles each snippet with `zeo` in-process, builds and runs the
//! generated program, and asserts stdout/stderr/exit status separately, plus a
//! zero-subprocess negative-path tier (`compile_project(...).unwrap_err()`) for
//! compile errors. The `tests/*.rb` example programs with `.rb.expected` remain
//! as a smaller ruby-oracle golden smoke suite (`cargo test --test examples`);
//! this is the default place to add coverage.
//!
//! Split by subject into the sibling modules below. This is ONE test binary --
//! a `tests/e2e/` directory with `main.rs`, not a file per subject -- so the
//! split costs no extra compiling or linking. It exists purely so a test can be
//! found, and added next to its neighbours, in what was a single 21,000-line
//! file.

#[path = "../support/mod.rs"]
mod support;

/// The compiler repo's own bundled packages/ dir, as the test-project
/// harness's package-dir argument (absolute, so the temp-dir join is a
/// no-op replacement).
pub const REPO_PACKAGES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../gems");

mod blocks_procs;
mod cext_build;
mod classes;
mod codegen;
mod collections;
mod concurrency;
mod control_flow;
mod debuginfo;
mod enumerable;
mod etc;
mod eval;
mod exceptions;
mod ffi;
mod gc_weak;
mod gems_all;
mod gems_require;
mod gems_vendored;
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

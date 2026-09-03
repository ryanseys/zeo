//! The tests that drive the compiler as a library or the CLI in more than
//! one step: package workflows, the program cache, gem stores, C extension
//! builds, the link ledger, debug info. Everything that is "run a program,
//! compare its output" lives under `test/` instead.
//!
//! One binary, split by subject into the modules below: a test target costs
//! a full link of the compiler, so the split exists only so a test can be
//! found beside its neighbours.

#[path = "../common/mod.rs"]
mod common;
mod support;

/// The names the modules grew up with.
#[path = "../corpus/case.rs"]
#[allow(dead_code)]
mod case;
#[path = "../corpus/oracle.rs"]
#[allow(dead_code)]
mod oracle;
mod paths {
    pub use crate::common::{profile_dir, runtime_archive, workspace_root};

    /// The pinned oracle, as an absolute path.
    pub fn resolve_ruby(_cwd: &std::path::Path) -> std::path::PathBuf {
        crate::oracle::find(&workspace_root())
            .map(|o| o.bin)
            .unwrap_or_else(|e| panic!("{e}"))
    }
}
mod zeo_bin {
    pub use crate::common::zeo_cli;
}

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

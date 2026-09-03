//! Everything that needs no golden corpus: the CLI surface, CLIF snapshots,
//! `test/` corpus hygiene, the class surface against the ABI table,
//! `Gemfile.lock` against the vendored gems, and the diagnostic renderings.
//! One binary -- a test target costs a full link of the whole compiler.

mod binary_size;
mod builtin_shape;
mod cli;
mod clif;
mod corpus_hygiene;
mod diagnostics;
mod env_vars;
mod gem_versions;
mod mkmf_probes;
mod no_c;
mod ze0_clif;

// The corpus file format: its unit tests only run under a libtest harness,
// which the datatest `corpus` binary is not.
#[path = "../corpus/case.rs"]
#[allow(dead_code)]
mod case;
#[path = "../corpus/suites.rs"]
#[allow(dead_code)]
mod suites;

// The golden comparison's scrubbing. Its suites run under datatest, which
// replaces the libtest harness, so its unit tests only run from here.
#[path = "../corpus/normalize.rs"]
mod normalize;
mod normalize_tests;

#[path = "../common/mod.rs"]
#[allow(dead_code)]
mod common;
/// The names the modules grew up with.
mod paths {
    pub use crate::common::{profile_dir, runtime_archive, workspace_root};
}
mod zeo_bin {
    pub use crate::common::zeo_cli;
}

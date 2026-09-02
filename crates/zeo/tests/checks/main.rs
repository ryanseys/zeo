//! Everything that needs no golden corpus: the CLI surface, CLIF snapshots,
//! `tests/` sidecar hygiene, the class surface against the ABI table,
//! `Gemfile.lock` against the vendored gems, and the diagnostic renderings.
//! One binary -- a test target costs a full link of the whole compiler.

mod binary_size;
mod builtin_shape;
mod cli;
mod clif;
mod diagnostics;
mod env_vars;
mod gem_versions;
mod goldens_hygiene;
mod mkmf_probes;
mod no_c;

// The golden comparison's scrubbing. Its suites run under datatest, which
// replaces the libtest harness, so its unit tests only run from here.
#[path = "../harness/normalize.rs"]
mod normalize;
mod normalize_tests;

// `clif` reads `libzeo.a`, which a test run does not build.
#[path = "../harness/paths.rs"]
mod paths;

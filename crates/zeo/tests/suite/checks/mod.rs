//! In-process invariants that need no corpus: the CLI surface, CLIF
//! snapshots, `test/` corpus hygiene, the class surface against the ABI
//! table, `Gemfile.lock` against the vendored gems, tracked-tree hygiene and
//! the diagnostic renderings.

mod binary_size;
mod builtin_shape;
mod cli;
mod clif;
mod corpus_hygiene;
mod diagnostics;
mod docs;
mod env_vars;
mod gem_versions;
mod hygiene;
mod mkmf_probes;
mod no_c;
mod ze0_clif;
mod ze0_hygiene;

// The corpus file format's and the scrubber's own unit tests. Both modules
// live under `corpus/`, whose binary replaces the libtest harness with
// datatest -- so their `#[test]`s can only run from here.
mod normalize_tests;

//! Every test that is Rust rather than a Ruby program.
//!
//! One binary, because a test target costs a full link of the whole
//! compiler and that link is the dominant cost of adding another. It holds
//! two groups:
//!
//! - `checks`: in-process invariants that need no corpus -- CLIF snapshots,
//!   the class surface against the ABI table, `Gemfile.lock` against the
//!   vendored gems, tracked-tree hygiene, the diagnostic renderings.
//! - `api`: everything that drives the compiler as a library or the CLI in
//!   more than one step -- package workflows, the program cache, gem stores,
//!   C extension builds, the link ledger, debug info.
//!
//! Anything that is "run a program, compare its output" belongs under
//! `test/` as a `.rb` file instead, not here.

// Shared with the `corpus` binary and with `cargo xtask bless`, so each uses
// a part of each.
#[path = "../corpus/case.rs"]
#[allow(
    dead_code,
    reason = "shared through `#[path]`; each binary uses a part of it"
)]
mod case;
#[path = "../common/mod.rs"]
#[allow(
    dead_code,
    reason = "shared through `#[path]`; each binary uses a part of it"
)]
mod common;
#[path = "../corpus/normalize.rs"]
mod normalize;
#[path = "../corpus/oracle.rs"]
#[allow(
    dead_code,
    reason = "shared through `#[path]`; each binary uses a part of it"
)]
mod oracle;
#[path = "../corpus/suites.rs"]
#[allow(
    dead_code,
    reason = "shared through `#[path]`; each binary uses a part of it"
)]
mod suites;

/// The names the test modules use for the shared helpers.
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

#[path = "api/support.rs"]
mod support;

mod api;
mod checks;

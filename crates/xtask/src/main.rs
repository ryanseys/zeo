//! `cargo run -p xtask -- <command>`: the project's build/test automation,
//! mirroring zeo's `make test` / `make regen-expected` without pulling in
//! a task-runner crate -- the Rust community's usual way to add custom
//! project automation without extra dependencies.
//!
//! (`examples/*.rb` + `.rb.expected` now run as `cargo test --test examples`
//! via datatest-stable; `cargo xtask bless <filter>` re-records their goldens from
//! ruby -- see `crates/zeo/tests/support/golden.rs`.)
//!
//! - `xtask bench [--filter <substr>] [--runs N] [--update-baseline]`: the
//!   golden-output performance suite under `bench/` (see `bench.rs`).
//! - `xtask arity-oracle`: re-records `conformance/builtin-arity.tsv` from the
//!   installed ruby, which the `builtin_arity` drift test reads.
//! - `xtask method-census`: re-records `conformance/method-census.tsv` -- what
//!   every module CRuby reaches from `Object` owns -- which the
//!   `method_census` coverage ratchet reads.
//! - `xtask dist [--target <triple>]`: assembles the relocatable distribution
//!   (`bin/zeo` + `share/zeo/{gems,runtime}`) and its tarball -- the single
//!   artifact the GitHub Release, Homebrew, and the platform gems all carry
//!   (see `dist.rs`).
//! - `xtask stage-publish [--check]`: stages the two artifacts the published
//!   `zeo` crate ships but the repo does not commit -- the pregenerated class
//!   surface and the embedded gems archive (see `stage_publish.rs`).
//!
//! (The golden-file conformance corpus, examples, and gaps all run as
//! `cargo test`/nextest -- see `crates/zeo/tests/`.)

mod arity_oracle;
mod bench;
mod bless;
mod dist;
mod exec;
mod gem;
mod gem_probe;
mod gemtests;
mod jobs;
mod method_census;
mod stage_publish;

use std::path::PathBuf;
use std::process::ExitCode;

/// The repository root, canonicalized.
///
/// The manifest-relative form ends `crates/xtask/../..`, and every path built
/// from it inherits those segments -- so a tool that prints where it wrote
/// something shows `.../crates/xtask/../../conformance/gem-probe.md`. That is
/// the same directory, and unreadable. Falls back to the literal path when the
/// tree cannot be canonicalized, which only happens if it has been removed.
fn workspace_root() -> PathBuf {
    let literal = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    std::fs::canonicalize(&literal).unwrap_or(literal)
}

fn main() -> ExitCode {
    let root = workspace_root();
    let args: Vec<String> = std::env::args().skip(2).collect();
    match std::env::args().nth(1).as_deref() {
        Some("bench") => bench::main(&root, &args),
        Some("bless") => bless::main(&root, &args),
        Some("gem-probe") => gem_probe::main(&root, &args),
        Some("gem") => gem::main(&root, &args),
        Some("gemtests") => gemtests::main(&root, &args),
        Some("arity-oracle") => arity_oracle::main(&root, &args),
        Some("method-census") => method_census::main(&root, &args),
        Some("stage-publish") => stage_publish::main(&root, &args),
        Some("dist") => dist::main(&root, &args),
        _ => {
            eprintln!(
                "usage: cargo run -p xtask -- \
                 <bench|bless|gem|gem-probe|gemtests|arity-oracle|\
                 method-census|stage-publish|dist>"
            );
            ExitCode::FAILURE
        }
    }
}

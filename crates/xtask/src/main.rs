//! `cargo run -p xtask -- <command>`: the project's build/test automation,
//! mirroring zeo's `make test` / `make regen-expected` without pulling in
//! a task-runner crate -- the Rust community's usual way to add custom
//! project automation without extra dependencies.
//!
//! (`examples/*.rb` + `.rb.expected` now run as `cargo test --test examples`
//! via datatest-stable; `ZEO_BLESS=1 cargo test` re-records their goldens from
//! ruby -- see `crates/zeo/tests/support/golden.rs`.)
//!
//! - `xtask bench [--filter <substr>] [--runs N] [--update-baseline]`: the
//!   golden-output performance suite under `bench/` (see `bench.rs`).
//! - `xtask compile-bench [--filter <substr>] [--runs N] [--update-baseline]`:
//!   compiler wall time + generated-code size/cleanliness over a fixed program
//!   set, recorded in `bench/compile-baseline.tsv` (see `compile_bench.rs`).
//! - `xtask stdlib-status [<lib-dir>]`: sweeps the installed Ruby stdlib `lib`
//!   (dropped in via `-I`, no bespoke flag) and records which files `zeo`
//!   can compile -- the stdlib progress tracker (see `stdlib_status.rs`).
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
mod compile_bench;
mod dist;
mod exec;
mod gem;
mod gem_compat;
mod gem_probe;
mod method_census;
mod prebuild;
mod stage_publish;
mod stdlib_status;

use std::path::PathBuf;
use std::process::ExitCode;

fn workspace_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn main() -> ExitCode {
    let root = workspace_root();
    let args: Vec<String> = std::env::args().skip(2).collect();
    match std::env::args().nth(1).as_deref() {
        Some("bench") => bench::main(&root, &args),
        Some("compile-bench") => compile_bench::main(&root, &args),
        Some("prebuild-runtimes") => prebuild::main(&root, &args),
        Some("stdlib-status") => stdlib_status::main(&root, &args),
        Some("gem-compat") => gem_compat::main(&root, &args),
        Some("gem-probe") => gem_probe::main(&root, &args),
        Some("gem") => gem::main(&root, &args),
        Some("arity-oracle") => arity_oracle::main(&root, &args),
        Some("method-census") => method_census::main(&root, &args),
        Some("stage-publish") => stage_publish::main(&root, &args),
        Some("dist") => dist::main(&root, &args),
        _ => {
            eprintln!(
                "usage: cargo run -p xtask -- \
                 <bench|compile-bench|prebuild-runtimes|stdlib-status|gem-compat|gem|\
                 arity-oracle|method-census|stage-publish|dist>"
            );
            ExitCode::FAILURE
        }
    }
}

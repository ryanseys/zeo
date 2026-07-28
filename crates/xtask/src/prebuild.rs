//! `cargo run -p xtask -- prebuild-runtimes`: build every (runtime, linkage)
//! zeo-rt combination the test suites link, release-profiled, through the
//! exact `backend::build_runtime` path the harnesses themselves use -- so CI
//! prebuilds precisely what the golden suite's Dynamic linkage and the e2e
//! suite's builds will ask for, instead of paying those cargo builds lazily
//! inside the first test processes (serialized behind the runtime-build
//! flock).

use std::path::Path;
use std::process::ExitCode;
use zeo::backend::{Linkage, Profile, Runtime, build_runtime};

pub fn main(_root: &Path, _args: &[String]) -> ExitCode {
    for (runtime, linkage) in [
        (Runtime::Lean, Linkage::Static),
        (Runtime::Eval, Linkage::Static),
        (Runtime::Lean, Linkage::Dynamic),
        (Runtime::Eval, Linkage::Dynamic),
    ] {
        if let Err(e) = build_runtime(Profile::Release, runtime, linkage) {
            eprintln!("prebuild-runtimes: {e}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

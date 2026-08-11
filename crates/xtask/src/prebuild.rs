//! `cargo run -p xtask -- prebuild-runtimes [--profile debug|release]`: build
//! every (runtime, linkage) zeo-rt combination the test suites link, through
//! the exact `backend::build_runtime` path the harnesses themselves use -- so
//! CI prebuilds precisely what the golden suite's Dynamic linkage and the e2e
//! suite's builds will ask for, instead of paying those cargo builds lazily
//! inside the first test processes (serialized behind the runtime-build
//! flock).
//!
//! Debug is the default because it is what the harnesses link
//! (`Profile::from_env_or(Profile::Debug)` with nothing setting
//! `ZEO_RUNTIME_PROFILE`); a release prebuild here used to produce four
//! artifacts no test ever loaded.

use std::path::Path;
use std::process::ExitCode;
use zeo::backend::{Linkage, Profile, Runtime, build_runtime};

pub fn main(_root: &Path, args: &[String]) -> ExitCode {
    let profile = match parse_profile(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("prebuild-runtimes: {e}");
            return ExitCode::FAILURE;
        }
    };
    for (runtime, linkage) in [
        (Runtime::Lean, Linkage::Static),
        (Runtime::Eval, Linkage::Static),
        (Runtime::Lean, Linkage::Dynamic),
        (Runtime::Eval, Linkage::Dynamic),
    ] {
        if let Err(e) = build_runtime(profile, runtime, linkage) {
            eprintln!("prebuild-runtimes: {e}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

fn parse_profile(args: &[String]) -> Result<Profile, String> {
    let mut it = args.iter();
    let mut profile = Profile::Debug;
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--profile" => {
                profile = match it.next().map(String::as_str) {
                    Some("debug") => Profile::Debug,
                    Some("release") => Profile::Release,
                    Some(other) => return Err(format!("unknown profile `{other}`")),
                    None => return Err("--profile needs a value".into()),
                }
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(profile)
}

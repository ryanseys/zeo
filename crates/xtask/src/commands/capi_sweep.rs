//! `cargo xtask capi-sweep bless [<gem>]` -- record ruby's answer for the
//! C-gem sweep's smoke programs.
//!
//! The sweep (`crates/zeo/tests/e2e/capi_sweep.rs`) runs each
//! `crates/zeo/tests/fixtures/capi_sweep/<gem>/smoke.rb` through zeo's C-API
//! route and compares stdout with `smoke.expected`. This records that file
//! from the oracle ruby resolving the same `Gemfile.lock`, so both sides
//! run the same gem version's own C extension.
//!
//! Every bless names its gems: with no argument every smoke is re-recorded,
//! which is a deliberate, visible choice rather than a default of some
//! other command.

use std::path::PathBuf;

use crate::exec::{self, Capture};
use crate::ruby::Oracle;
use crate::{Error, root, root_join};

const USAGE: &str = "\
usage: cargo xtask capi-sweep bless [<gem>]

Record ruby's stdout for crates/zeo/tests/fixtures/capi_sweep/<gem>/smoke.rb
into smoke.expected beside it. With no <gem>, every smoke is recorded.
";

pub fn run(args: &[String]) -> Result<(), Error> {
    match args.first().map(String::as_str) {
        Some("bless") => bless(args.get(1).map(String::as_str)),
        Some("--help") | Some("-h") => {
            print!("{USAGE}");
            Ok(())
        }
        _ => Err(Error::new(USAGE.to_string())),
    }
}

fn fixtures() -> PathBuf {
    root_join("crates/zeo/tests/fixtures/capi_sweep")
}

fn bless(gem: Option<&str>) -> Result<(), Error> {
    let oracle = Oracle::find()?;
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(fixtures())
        .map_err(|e| Error::new(format!("{}: {e}", fixtures().display())))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("smoke.rb").is_file())
        .filter(|p| gem.is_none_or(|g| p.file_name().is_some_and(|n| n == g)))
        .collect();
    dirs.sort();
    if dirs.is_empty() {
        return Err(Error::new(format!(
            "no smoke.rb under {} matches {gem:?}",
            fixtures().display()
        )));
    }
    for dir in dirs {
        let name = dir
            .file_name()
            .expect("a gem directory")
            .to_string_lossy()
            .into_owned();
        let smoke = dir.join("smoke.rb");
        let argv = oracle.argv(&[&smoke.display().to_string()]);
        let out = exec::run(&argv, root(), &oracle.env(), Capture::Both)?;
        if !out.success() {
            return Err(Error::new(format!(
                "ruby refused {}: exit {}\n{}",
                smoke.display(),
                out.code_text(),
                out.stderr_text()
            )));
        }
        let expected = dir.join("smoke.expected");
        let changed = std::fs::read(&expected).ok().as_deref() != Some(out.stdout.as_slice());
        std::fs::write(&expected, &out.stdout)
            .map_err(|e| Error::new(format!("{}: {e}", expected.display())))?;
        eprintln!(
            "capi-sweep: {name}: {}",
            if changed { "recorded" } else { "unchanged" }
        );
    }
    Ok(())
}

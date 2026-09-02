//! The repo's own chores, reachable with nothing but cargo.
//!
//! `cargo xtask <command>` -- the alias is in `.cargo/config.toml`. Every
//! command here runs against a checkout, never against an installed zeo, and
//! nothing here is shipped: this crate is `publish = false`.
//!
//! Argument parsing is hand-rolled, the same as the compiler's own CLI. The
//! flags are few and the alternative is a dependency the workspace does not
//! otherwise carry.

use std::path::{Path, PathBuf};

mod commands;
mod exec;
mod payload;
mod ruby;
mod scratch;
mod vendor;

/// A chore that could not finish, with the reason a person needs.
pub struct Error {
    message: String,
    /// The command already said what went wrong, in its own words and its own
    /// place. A verdict like "4 divergent rows" is a FINDING the report has
    /// already laid out row by row; repeating it under an `xtask:` prefix
    /// would read as a second, tool-level failure.
    reported: bool,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Error {
            message: message.into(),
            reported: false,
        }
    }

    /// Exit nonzero, printing nothing further.
    pub fn reported() -> Self {
        Error {
            message: String::new(),
            reported: true,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// The repo root: this crate's `../..`, baked in at compile time.
pub fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xtask sits two levels under the workspace root")
}

pub fn root_join(rel: impl AsRef<Path>) -> PathBuf {
    root().join(rel)
}

/// Build the release `zeo` unless `ZEO_BIN` names one, and answer its path.
/// Release, because that is what every ledger and golden was recorded
/// against.
pub fn build_zeo() -> Result<PathBuf, Error> {
    let bin = commands::bless::zeo_bin();
    if std::env::var_os("ZEO_BIN").is_some() {
        return Ok(bin);
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = exec::run(
        &[&cargo, "build", "--quiet", "--release", "-p", "zeo"],
        root(),
        &[],
        exec::Capture::Nothing,
    )?;
    if !out.success() {
        return Err(Error::new("cargo build -p zeo failed"));
    }
    Ok(bin)
}

/// Write only when the bytes differ, so an unchanged generated file keeps its
/// mtime and a build that keys on it does not redo itself.
pub fn write_if_changed(path: &Path, content: &[u8]) -> Result<(), Error> {
    if std::fs::read(path).is_ok_and(|old| old == content) {
        return Ok(());
    }
    std::fs::write(path, content).map_err(|e| Error::new(format!("writing {}: {e}", path.display())))
}

const USAGE: &str = "\
usage: cargo xtask <command> [options]

commands:
  bench           the compiler-cost instrument (runtime suite: cargo bench)
  bless           re-record golden `.expected` files from the ruby oracle
  capi-sweep      record ruby's answers for the C-gem sweep's smoke programs
  cext            the vendored MRI C API headers and the rb_* census
  diff            compare a snippet across ruby and zeo, and file a gap
  dist            assemble the relocatable distribution
  gem             build the platform gem from a dist staging
  linux           run the suites in the linux container
  promote-gap     move a fixed gap into the passing suite
  stage-publish   stage the artifacts the published crate ships

`cargo xtask <command> --help` describes one command.
";

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        eprint!("{USAGE}");
        return std::process::ExitCode::FAILURE;
    };
    if command == "--help" || command == "-h" {
        print!("{USAGE}");
        return std::process::ExitCode::SUCCESS;
    }
    let rest = &args[1..];
    let result = match command.as_str() {
        "bench" => commands::bench::run(rest),
        "bless" => commands::bless::run(rest),
        "capi-sweep" => commands::capi_sweep::run(rest),
        "cext" => commands::cext::run(rest),
        "diff" => commands::diff::run(rest),
        "dist" => commands::dist::run(rest),
        "gem" => commands::gem::run(rest),
        "linux" => commands::linux::run(rest),
        "promote-gap" => commands::promote_gap::run(rest),
        "stage-publish" => commands::stage_publish::run(rest),
        other => Err(Error::new(format!(
            "unknown command {other:?}\n\n{USAGE}"
        ))),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            if !e.reported {
                eprintln!("xtask: {e}");
            }
            std::process::ExitCode::FAILURE
        }
    }
}

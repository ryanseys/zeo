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

/// A chore that could not finish, with the reason a person needs.
pub struct Error(String);

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Error(message.into())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
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

const USAGE: &str = "\
usage: cargo xtask <command> [options]

commands:
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
        "stage-publish" => commands::stage_publish::run(rest),
        other => Err(Error::new(format!(
            "unknown command {other:?}\n\n{USAGE}"
        ))),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

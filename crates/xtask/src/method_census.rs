//! `cargo run -p xtask -- method-census [--check]`: records what CRuby's whole
//! reachable module tree owns, into `conformance/method-census.tsv`.
//!
//! The dump is checked in so `crates/zeo/tests/method_census.rs` can gate
//! coverage on a machine with no ruby -- that test compiles the SAME walker
//! (`tools/method_census.rb`) through zeo and diffs the two. Regenerating is an
//! explicit act; `--check` re-runs the oracle and fails if the committed copy
//! is stale, for a CI job that does have the pinned ruby.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const DUMP: &str = "conformance/method-census.tsv";
const WALKER: &str = "tools/method_census.rb";

pub fn main(root: &Path, args: &[String]) -> ExitCode {
    let check = args.iter().any(|a| a == "--check");

    let ruby = resolve_ruby(root);
    let out = match Command::new(&ruby)
        .arg("--disable-gems")
        .arg(root.join(WALKER))
        .current_dir(root)
        .output()
    {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            eprintln!("the ruby oracle exited with {}", o.status);
            eprint!("{}", String::from_utf8_lossy(&o.stderr));
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("cannot run the ruby oracle at {}: {e}", ruby.display());
            return ExitCode::FAILURE;
        }
    };
    let dump = String::from_utf8_lossy(&out.stdout).into_owned();

    let target = root.join(DUMP);
    if check {
        let committed = std::fs::read_to_string(&target).unwrap_or_default();
        if committed == dump {
            println!("{DUMP} is up to date");
            return ExitCode::SUCCESS;
        }
        eprintln!("{DUMP} is stale -- re-run `cargo run -p xtask -- method-census`");
        return ExitCode::FAILURE;
    }

    if let Some(parent) = target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&target, &dump) {
        eprintln!("cannot write {}: {e}", target.display());
        return ExitCode::FAILURE;
    }
    let modules = dump.lines().filter(|l| l.contains("\ti\t")).count();
    println!(
        "wrote {DUMP} ({modules} modules, {} lines)",
        dump.lines().count()
    );
    ExitCode::SUCCESS
}

/// `mise which ruby` (falling back to bare `ruby`), the same resolution the
/// golden harness uses, so both come from the `mise.toml`-pinned oracle.
fn resolve_ruby(cwd: &Path) -> PathBuf {
    if let Ok(out) = Command::new("mise")
        .arg("which")
        .arg("ruby")
        .current_dir(cwd)
        .output()
        && out.status.success()
    {
        let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    PathBuf::from("ruby")
}

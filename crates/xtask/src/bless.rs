//! `cargo xtask bless <filter>` -- re-record goldens from the ruby oracle, for
//! the tests you name and no others.
//!
//! Blessing used to be `ZEO_BLESS=1 cargo test`, and the missing word there is
//! WHICH. An unfiltered run rewrote, created and deleted goldens across the
//! whole suite in one go, and encoded one machine's gem store into the ones it
//! touched. Nothing about the spelling suggested that: `ZEO_BLESS=1` reads like
//! a mode, not like "and apply it to all 5000 of them".
//!
//! The guard cannot live in the test binary. `zeo-tests` sets `harness = false`
//! and its goldens are datatest-stable cases, so under nextest each case runs
//! in its own process -- a case cannot see whether the user narrowed the run,
//! only that it is itself running. So the filter has to be owned by whatever
//! spells the invocation, which is this command.
//!
//! There is deliberately no `--allow-delete`. Removing `<rb>.err.expected` is
//! part of a CORRECT bless -- an absent file is how a golden says "stderr must
//! be empty" -- so gating it would break the ordinary single-test case that
//! this command exists to make easy. The filter is what bounds the blast
//! radius; the summary below is what makes a deletion impossible to miss.

use std::path::Path;
use std::process::{Command, ExitCode};

/// The handshake `zeo-tests` looks for. Named for its only legitimate source,
/// so a bare `ZEO_BLESS=1 cargo test` no longer does anything and the spelling
/// says where to go instead.
pub const BLESS_VAR: &str = "ZEO_BLESS_FROM_XTASK";

/// Filters that would defeat the point. nextest's `test()` matcher is a
/// substring, so the empty string selects everything.
fn too_broad(filter: &str) -> Option<&'static str> {
    match filter.trim() {
        "" => Some("an empty filter selects every test"),
        "*" | "all" => Some("this is a substring, not a glob -- it selects every test"),
        _ => None,
    }
}

/// Goldens and generated conformance artifacts, which is everything a bless
/// can write.
const WATCHED: [&str; 2] = ["tests", "conformance"];

fn changed_goldens(root: &Path) -> Vec<(String, String)> {
    let out = Command::new("git")
        .arg("status")
        .arg("--porcelain")
        .arg("--")
        .args(WATCHED)
        .current_dir(root)
        .output();
    let Ok(out) = out else { return Vec::new() };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let (code, path) = l.split_at_checked(3)?;
            Some((code.trim().to_string(), path.to_string()))
        })
        .collect()
}

pub fn main(root: &Path, args: &[String]) -> ExitCode {
    let mut filter: Option<String> = None;
    let mut passthrough: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--" => passthrough.extend(it.by_ref().cloned()),
            other if other.starts_with('-') => passthrough.push(other.to_string()),
            other if filter.is_none() => filter = Some(other.to_string()),
            other => {
                eprintln!("bless: unexpected second filter {other:?}");
                return ExitCode::FAILURE;
            }
        }
    }

    let Some(filter) = filter else {
        eprintln!(
            "usage: cargo run -p xtask -- bless <filter> [-- <nextest args>]\n\
             \n\
             <filter> is a nextest substring match on the test name, and it is\n\
             required: blessing everything at once is what this command exists\n\
             to prevent. Examples:\n\
             \n  \
             cargo run -p xtask -- bless forward_args\n  \
             cargo run -p xtask -- bless spinel::yield_\n  \
             cargo run -p xtask -- bless builtin_arity"
        );
        return ExitCode::FAILURE;
    };
    if let Some(why) = too_broad(&filter) {
        eprintln!("bless: refusing {filter:?} -- {why}");
        return ExitCode::FAILURE;
    }

    let before = changed_goldens(root);

    eprintln!("bless: re-recording goldens matching {filter:?} from the ruby oracle");
    // `-p zeo-tests` is load-bearing for SPEED, not scope: every reader of
    // `BLESS_VAR` lives in that package. Unscoped, this nextest resolves
    // features across the whole workspace while the `cargo run` that got us
    // here resolved them for xtask alone, so the two disagree on
    // ring/rustls/ureq/xtask and each rebuild invalidates the other's -- ~17s
    // of recompiling per bless, every time.
    let status = Command::new("cargo")
        .arg("nextest")
        .arg("run")
        .arg("-p")
        .arg("zeo-tests")
        .arg("-E")
        .arg(format!("test({filter})"))
        .args(&passthrough)
        .env(BLESS_VAR, "1")
        .current_dir(root)
        .status();
    // A blessing run is EXPECTED to report failures: a case that rewrites its
    // golden and then asserts against the old one is not the contract here.
    // What matters is what changed on disk.
    match status {
        Ok(_) => {}
        Err(e) => {
            eprintln!("bless: running cargo nextest: {e}");
            return ExitCode::FAILURE;
        }
    }

    let after = changed_goldens(root);
    let new: Vec<_> = after.iter().filter(|c| !before.contains(c)).collect();
    if new.is_empty() {
        eprintln!("bless: no golden changed -- was {filter:?} the name you meant?");
        return ExitCode::SUCCESS;
    }
    let deleted: Vec<_> = new.iter().filter(|(c, _)| c.contains('D')).collect();
    eprintln!("bless: {} golden(s) changed:", new.len());
    for (code, path) in &new {
        eprintln!("  {code:>2}  {path}");
    }
    if !deleted.is_empty() {
        // An absent `.err.expected` is a real assertion ("stderr must be
        // empty"), so a deletion changes the contract as much as a rewrite --
        // and it is the one change `git diff` shows nothing for.
        eprintln!(
            "\nbless: {} golden(s) were DELETED. That asserts their stderr is now empty;\n\
             if that is not what you meant, `git checkout` them.",
            deleted.len()
        );
    }
    ExitCode::SUCCESS
}

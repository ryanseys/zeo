//! Every check CI runs that is not a test: one command, one report.
//!
//! The steps run in order and NONE of them stops the run. A red board should
//! say everything that is wrong in one go, because the next answer costs
//! another full build; the summary at the end names each step and its
//! verdict, and the exit status is the worst of them.
//!
//! What is NOT here: anything that runs the corpus. `cargo nextest run -P
//! full` is the test gate, and it says its own piece.

use std::path::Path;

use crate::exec::{self, Capture};
use crate::{Error, root};

const USAGE: &str = "\
usage: cargo xtask ci [--list] [--only <step>]...

Every non-test check CI runs, in order, none of them stopping the rest.

options:
  --list          name the steps and stop
  --only <step>   run just this step (repeatable)
";

/// One check: what it asks, and the command that asks it.
struct Step {
    name: &'static str,
    what: &'static str,
    argv: Vec<String>,
}

fn step(name: &'static str, what: &'static str, argv: &[&str]) -> Step {
    Step {
        name,
        what,
        argv: argv.iter().map(|s| s.to_string()).collect(),
    }
}

/// The `ext-*` features docs.rs builds, read out of zeo-rt's manifest so the
/// two cannot disagree. This is the set that SHIPS, and it has stopped
/// compiling on its own before.
fn docs_rs_features() -> Result<String, Error> {
    let manifest = root().join("crates/zeo-rt/Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .map_err(|e| Error::new(format!("{}: {e}", manifest.display())))?;
    let mut names = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with('[') {
            inside = line.starts_with("[package.metadata.docs.rs]");
            continue;
        }
        if inside {
            let name = line.trim().trim_matches(&[' ', '"', ','][..]);
            if name.starts_with("ext-") {
                names.push(name.to_string());
            }
        }
    }
    if names.is_empty() {
        return Err(Error::new(
            "no ext-* features under [package.metadata.docs.rs] in crates/zeo-rt/Cargo.toml",
        ));
    }
    Ok(names.join(","))
}

fn steps() -> Result<Vec<Step>, Error> {
    let docs_rs = docs_rs_features()?;
    Ok(vec![
        step(
            "clippy",
            "the workspace lints clean at CI's severity",
            &[
                "cargo",
                "clippy",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ],
        ),
        step(
            "deny",
            "permissive licenses only, and no yanked or advisory-flagged crate",
            &["cargo", "deny", "check"],
        ),
        // `cargo-machete`, not `cargo machete`: spawned rather than run from
        // a shell, the subcommand form reaches the tool with "machete" as an
        // argument, which it reads as a DIRECTORY to analyze. It then reports
        // success over a directory that does not exist. The binary is on PATH
        // wherever the subcommand is.
        step(
            "machete",
            "no unused dependency ships in a .crate",
            &["cargo-machete"],
        ),
        step(
            "cext-hunks",
            "zeo's edits still apply to the pinned MRI headers",
            &["cargo", "xtask", "cext", "hunks", "--check"],
        ),
        step(
            "cext-api",
            "the generated C API table matches those headers",
            &["cargo", "xtask", "cext", "api", "--check"],
        ),
        step(
            "cext-forward",
            "the forwarding shims match those headers",
            &["cargo", "xtask", "cext", "forward", "--check"],
        ),
        step(
            "cext-layout",
            "the measured object layout matches those headers",
            &["cargo", "xtask", "cext", "layout", "--check"],
        ),
        step(
            "doctests",
            "the documentation examples compile and run (nextest does not run them)",
            &["cargo", "test", "--workspace", "--doc"],
        ),
        // `check`, not `build`: the question is whether the cfgs are right,
        // not whether an artifact comes out. The empty set is the strictest
        // configuration and the docs.rs set is the one that ships.
        step(
            "features-bare",
            "zeo-rt compiles with every optional feature off",
            &["cargo", "check", "-p", "zeo-rt", "--no-default-features"],
        ),
        step(
            "features-capi",
            "the C API compiles on its own",
            &["cargo", "check", "-p", "zeo-capi"],
        ),
        step(
            "features-compiler",
            "the compiler compiles with every optional feature off",
            &["cargo", "check", "-p", "zeo", "--no-default-features"],
        ),
        step(
            "features-docsrs",
            "zeo-rt compiles in the feature set docs.rs builds",
            &[
                "cargo",
                "check",
                "-p",
                "zeo-rt",
                "--no-default-features",
                "--features",
                &docs_rs,
            ],
        ),
        step(
            "pure-stdlib",
            "the C-extension-free build still compiles",
            &[
                "cargo",
                "build",
                "-p",
                "zeo",
                "--no-default-features",
                "--features",
                "pure-stdlib",
            ],
        ),
    ])
}

pub fn run(args: &[String]) -> Result<(), Error> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return Ok(());
    }
    let all = steps()?;
    if args.iter().any(|a| a == "--list") {
        for s in &all {
            println!("  {:<18} {}", s.name, s.what);
        }
        return Ok(());
    }
    let mut only = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--only" => only.push(
                rest.next()
                    .ok_or_else(|| Error::new(format!("--only wants a step name\n\n{USAGE}")))?
                    .clone(),
            ),
            other => return Err(Error::new(format!("unknown option {other:?}\n\n{USAGE}"))),
        }
    }
    for name in &only {
        if !all.iter().any(|s| s.name == *name) {
            return Err(Error::new(format!(
                "no such step: {name:?}\n\nrun `cargo xtask ci --list` for the names"
            )));
        }
    }
    let chosen: Vec<&Step> = all
        .iter()
        .filter(|s| only.is_empty() || only.contains(&s.name.to_string()))
        .collect();

    tools(root());
    let mut verdicts = Vec::new();
    for (i, s) in chosen.iter().enumerate() {
        println!("\n=== [{}/{}] {}: {}", i + 1, chosen.len(), s.name, s.what);
        let argv: Vec<&str> = s.argv.iter().map(String::as_str).collect();
        let out = exec::run(&argv, root(), &[], Capture::Nothing)?;
        verdicts.push((s.name, out.success()));
    }

    println!("\n=== summary");
    for (name, ok) in &verdicts {
        println!("  {:<18} {}", name, if *ok { "ok" } else { "FAILED" });
    }
    let failed: Vec<&str> = verdicts
        .iter()
        .filter(|(_, ok)| !ok)
        .map(|(n, _)| *n)
        .collect();
    if failed.is_empty() {
        println!("\n{} step(s) ok", verdicts.len());
        return Ok(());
    }
    eprintln!("\n{} step(s) failed: {}", failed.len(), failed.join(", "));
    Err(Error::reported())
}

/// What is on PATH, to read against the pins. A drift here is the usual
/// reason "it passed on my machine".
fn tools(dir: &Path) {
    println!("=== tools");
    for argv in [
        vec!["rustc", "--version"],
        vec!["cargo", "clippy", "--version"],
        vec!["cargo", "nextest", "--version"],
        vec!["cargo", "deny", "--version"],
        vec!["cargo-machete", "--version"],
    ] {
        let name = argv.join(" ");
        match exec::run(&argv, dir, &[], Capture::Both) {
            Ok(out) if out.success() => {
                let text = out.stdout_text();
                println!("  {}", text.lines().next().unwrap_or("").trim());
            }
            _ => println!("  {name}: not installed"),
        }
    }
}

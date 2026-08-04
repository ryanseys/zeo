//! `cargo run -p xtask -- arity-oracle [--check]`: records what CRuby reports
//! for every class zeo declares, into `conformance/builtin-arity.tsv`.
//!
//! The dump is checked in so `crates/zeo/tests/builtin_arity.rs` can gate arity
//! drift on a machine with no ruby. Regenerating it is an explicit act; `--check`
//! re-runs the oracle and fails if the committed copy is stale, for a CI job
//! that does have the pinned ruby.
//!
//! The join key is the `ClassId` const ident. It is the only token the DSL
//! header and `zeo-abi`'s `BUILTINS` table share -- the header spells the class
//! `Stat`, while the Ruby name is `File::Stat` and only `BUILTINS` knows that,
//! along with the `require` that exposes it.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use zeo_dsl::scan as dsl_scan;

const DUMP: &str = "conformance/builtin-arity.tsv";

pub fn main(root: &Path, args: &[String]) -> ExitCode {
    let check = args.iter().any(|a| a == "--check");

    let abi = dsl_scan::scan_abi(root);
    let decls = dsl_scan::scan_decls(root);

    // Only classes zeo actually declares methods on are worth asking about.
    let mut wanted: Vec<&String> = decls.iter().map(|d| &d.class_const).collect();
    wanted.sort();
    wanted.dedup();

    let mut manifest = String::new();
    let mut unknown = Vec::new();
    for class_const in &wanted {
        match abi.get(*class_const) {
            Some(c) => manifest.push_str(&format!(
                "C\t{}\t{}\t{}\t{}\n",
                class_const,
                c.ruby_name,
                c.is_module,
                c.feature.as_deref().unwrap_or("-"),
            )),
            // A header whose ClassId has no BUILTINS row. The consistency check
            // in zeo's build.rs is what will make this an error; here it is
            // only a reason the class cannot be looked up.
            None => unknown.push((*class_const).clone()),
        }
    }

    let ruby = resolve_ruby(root);
    let script = root.join("tools/builtin_arity_oracle.rb");
    let mut child = match Command::new(&ruby)
        .arg(&script)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cannot run the ruby oracle at {}: {e}", ruby.display());
            return ExitCode::FAILURE;
        }
    };
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().expect("stdin was piped");
        if let Err(e) = stdin.write_all(manifest.as_bytes()) {
            eprintln!("cannot feed the class manifest to the oracle: {e}");
            return ExitCode::FAILURE;
        }
    }
    let out = match child.wait_with_output() {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            eprintln!("the ruby oracle exited with {}", o.status);
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("the ruby oracle failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut dump = String::from_utf8_lossy(&out.stdout).into_owned();
    if !unknown.is_empty() {
        // Recorded in the dump rather than only on stderr, so the set is
        // reviewable in the diff.
        let mut extra = String::new();
        for class_const in &unknown {
            extra.push_str(&format!("!\t{class_const}\tno-abi-row\t-\n"));
        }
        dump.push_str(&extra);
    }

    let target = root.join(DUMP);
    if check {
        let committed = std::fs::read_to_string(&target).unwrap_or_default();
        if committed == dump {
            println!("{DUMP} is up to date");
            return ExitCode::SUCCESS;
        }
        eprintln!("{DUMP} is stale -- re-run `cargo run -p xtask -- arity-oracle`");
        return ExitCode::FAILURE;
    }

    if let Some(parent) = target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&target, &dump) {
        eprintln!("cannot write {}: {e}", target.display());
        return ExitCode::FAILURE;
    }
    println!(
        "wrote {DUMP} ({} classes asked, {} declarations, {} lines)",
        wanted.len(),
        decls.len(),
        dump.lines().count(),
    );
    if !unknown.is_empty() {
        println!("  {} header(s) have no zeo-abi BUILTINS row", unknown.len());
    }
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
        && out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            if !path.is_empty() {
                return PathBuf::from(path);
            }
        }
    PathBuf::from("ruby")
}

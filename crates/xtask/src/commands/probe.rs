//! Run a differential PROBE MATRIX through both engines and report only the
//! rows that disagree.
//!
//! `xtask diff` compiles one snippet per invocation, which is the wrong shape
//! for a matrix of hundreds of rows. A probe is one Ruby program holding the
//! whole matrix, each row printing `name<TAB>result`: one compile, hundreds of
//! rows, and the divergent ones come back grouped and named.
//!
//! The matrices live in `tools/probes/`. Adding a row costs one line, and a
//! row that AGREES is regression cover the moment someone breaks it.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use crate::exec::{self, Capture};
use crate::ruby::Oracle;
use crate::{Error, root, root_join};

const USAGE: &str = "\
usage: cargo xtask probe [<name>...]

  cargo xtask probe                 # every matrix in tools/probes/
  cargo xtask probe arguments       # one of them
  cargo xtask probe --all-rows      # show agreeing rows too
";

pub fn run(args: &[String]) -> Result<(), Error> {
    let mut all_rows = false;
    let mut names = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--all-rows" => all_rows = true,
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(());
            }
            other if other.starts_with('-') => {
                return Err(Error::new(format!("unknown option {other:?}\n\n{USAGE}")));
            }
            other => names.push(other.trim_end_matches(".rb").to_string()),
        }
    }
    let available = available()?;
    if names.is_empty() {
        names = available.clone();
    }
    let missing: Vec<&String> = names.iter().filter(|n| !available.contains(n)).collect();
    if !missing.is_empty() {
        return Err(Error::new(format!(
            "no such probe: {} (have: {})",
            missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
            available.join(", ")
        )));
    }

    let zeo = crate::build_zeo()?;
    let oracle = Oracle::find();
    let mut total = 0;
    for name in &names {
        total += run_one(name, &zeo, &oracle, all_rows)?;
    }
    println!();
    if total == 0 {
        println!("no divergent rows");
        return Ok(());
    }
    println!("{total} divergent row(s)");
    Err(Error::reported())
}

fn probes_dir() -> PathBuf {
    root_join("tools/probes")
}

fn available() -> Result<Vec<String>, Error> {
    let dir = probes_dir();
    let entries =
        std::fs::read_dir(&dir).map_err(|e| Error::new(format!("reading {}: {e}", dir.display())))?;
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| Error::new(format!("reading {}: {e}", dir.display())))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(stem) = name.strip_suffix(".rb") {
            out.push(stem.to_string());
        }
    }
    out.sort();
    Ok(out)
}

fn run_one(name: &str, zeo: &Path, oracle: &Oracle, all_rows: bool) -> Result<usize, Error> {
    let file = probes_dir().join(format!("{name}.rb"));
    println!("=== {name} ===");
    let mut argv = oracle.argv(&[]);
    argv.push(file.display().to_string());
    let ruby = exec::run(&argv, root(), &oracle.env(), Capture::Both)?;
    let mine = exec::run(
        &[zeo, Path::new("-W0"), &file],
        root(),
        &[],
        Capture::Both,
    )?;
    report_engine_failure("ruby", &ruby);
    report_engine_failure("zeo", &mine);

    let rows_r = rows(&ruby.stdout_text());
    let rows_z = rows(&mine.stdout_text());
    // The union, ruby's rows first, so a matrix reads top to bottom.
    let mut order: Vec<&String> = rows_r.keys().collect();
    order.extend(rows_z.keys().filter(|k| !rows_r.contains_key(*k)));

    let diverged: Vec<&&String> = order
        .iter()
        .filter(|k| rows_r.get(**k) != rows_z.get(**k))
        .collect();
    for key in &order {
        let is_bad = diverged.contains(&key);
        if !all_rows && !is_bad {
            continue;
        }
        let missing = "<no row>".to_string();
        println!("{} {key}", if is_bad { "!" } else { " " });
        println!("    ruby: {}", rows_r.get(*key).unwrap_or(&missing));
        println!("    zeo : {}", rows_z.get(*key).unwrap_or(&missing));
    }
    println!("  {} of {} rows diverge", diverged.len(), order.len());
    Ok(diverged.len())
}

/// A probe that dies part way through still reports what it printed, so the
/// rows before the crash stay usable -- but the crash itself is the most
/// important finding on the page, so it is never swallowed, and every
/// `<no row>` below it is a CONSEQUENCE rather than a finding of its own. Say
/// that out loud: a truncated sweep reads exactly like a large genuine
/// cluster, and has been mistaken for one.
fn report_engine_failure(label: &str, out: &exec::Output) {
    if out.code == Some(0) && out.stderr.is_empty() {
        return;
    }
    println!(
        "  !! {label} exited {} -- rows after the failure are MISSING, not divergent",
        out.code_text()
    );
    for line in out.stderr_text().lines().take(6) {
        println!("     {line}");
    }
}

/// `name<TAB>result` per line, insertion-ordered so a matrix reads in the
/// order it printed. The address scrub is the golden harness's, and without it
/// every row whose text embeds an object address reads as a divergence, which
/// buries the real ones.
fn rows(text: &str) -> IndexMap<String, String> {
    use regex::Regex;
    use std::sync::OnceLock;
    static ADDRESS: OnceLock<Regex> = OnceLock::new();
    let address =
        ADDRESS.get_or_init(|| Regex::new(r"0x[0-9a-f]{8,16}").expect("a valid pattern"));

    let mut out = IndexMap::new();
    for line in text.lines() {
        let Some((name, result)) = line.split_once('\t') else {
            continue;
        };
        if result.is_empty() {
            continue;
        }
        out.insert(
            name.to_string(),
            address.replace_all(result, "0xADDR").into_owned(),
        );
    }
    out
}

//! One corpus program, run and held to its trailer.
//!
//! The suite says who recorded the trailer and whether zeo must match it;
//! the leg says how zeo runs the program; the directives say what the
//! program needs. Everything else is comparison.

use std::path::{Path, PathBuf};

use crate::case::{Answer, Case, Platform};
use crate::compare::{mismatch, norm_answer, split_gccheck};
use crate::legs::{Leg, zeo_command};
use crate::run::{Bounds, run_bounded};
use crate::suites::{RUN_CWD, Suite, Verdict};

pub fn run_cwd() -> PathBuf {
    crate::common::workspace_root().join(RUN_CWD)
}

/// The stdin bytes a directive names.
fn stdin_of(case: &Case, rb: &Path) -> Result<Option<Vec<u8>>, String> {
    let Some(rel) = &case.directives.stdin else {
        return Ok(None);
    };
    let path = rb.parent().unwrap_or(rb).join(rel);
    std::fs::read(&path)
        .map(Some)
        .map_err(|e| format!("{}: stdin {}: {e}", rb.display(), path.display()))
}

/// Compile and run `rb` on `leg`. `Ok` is what the program said; `Err` is a
/// harness failure (a spawn error, or a tripped bound).
fn run_once(leg: Leg, case: &Case, rb: &Path, suite: &Suite, cwd: &Path) -> Result<Answer, String> {
    if leg.is_aot() {
        // A real link needs the archive, which a test run does not build.
        crate::common::runtime_archive()?;
    }
    let mut cmd = zeo_command(leg, case, rb, suite, cwd)?;
    let bounds = match (suite.whole_graph, suite.verdict) {
        (true, _) => Bounds::WHOLE_GRAPH,
        (false, Verdict::Xfail) => Bounds::GAP,
        (false, Verdict::Match) => Bounds::ORDINARY,
    };
    let stdin = stdin_of(case, rb)?;
    run_bounded(&mut cmd, stdin.as_deref(), "zeo", bounds)
}

/// Whether this leg on this host should run `case` at all.
fn skipped(case: &Case, leg: Leg, rb: &Path) -> bool {
    if let Some(only) = case.directives.only
        && only != Platform::host()
    {
        eprintln!("{}: {only:?}-only, skipped", rb.display());
        return true;
    }
    if case.directives.backend.is_some() && leg.is_aot() {
        eprintln!("{}: needs the JIT, skipped on the AOT leg", rb.display());
        return true;
    }
    false
}

pub fn run(rb: &Path, suite: &Suite, leg: Leg) -> datatest_stable::Result<()> {
    // datatest hands a path relative to the crate manifest dir; absolutize
    // it so the child finds it after `current_dir`, and so the source-path
    // scrub matches.
    let rb = &std::fs::canonicalize(rb).unwrap_or_else(|_| rb.to_path_buf());
    let cwd = run_cwd();
    let case = Case::read(rb)?;
    if skipped(&case, leg, rb) {
        return Ok(());
    }
    let Some(trailer) = &case.trailer else {
        return Err(format!(
            "{}: no recorded answer under `__END__`. Record it: `cargo xtask bless {}`",
            rb.display(),
            rb.strip_prefix(&cwd).unwrap_or(rb).display()
        )
        .into());
    };
    let expected = trailer.for_host();

    let actual = run_once(leg, &case, rb, suite, &cwd);

    // Every run carries the exit cycle census on stderr. Take it out of the
    // ordinary comparison and hold it to the `#@ gccheck` directive on its
    // own: a cycle alive at exit is not a defect (`a << a` is supposed to
    // build one), so this gates CHANGE, never zero.
    let mut census_changed = None;
    let actual = match actual {
        Ok(mut a) => {
            let (err, census) = split_gccheck(&a.stderr);
            a.stderr = err;
            let want = case.directives.gccheck.as_deref().unwrap_or("").trim_end();
            if census != want {
                census_changed = Some(format!(
                    "{}: the exit cycle census changed.\n  expected: {}\n  actual:   {}\n\
                     Record it as `#@ gccheck: <line>` at the top of the program (say which cycle it builds), \
                     or find what stopped the collector seeing it.",
                    rb.display(),
                    if want.is_empty() { "<no cycle>" } else { want },
                    if census.is_empty() {
                        "<no cycle>"
                    } else {
                        &census
                    },
                ));
            }
            Ok(a)
        }
        other => other,
    };

    let expected = norm_answer(expected, rb, &cwd);
    let agrees = census_changed.is_none()
        && matches!(&actual, Ok(a) if norm_answer(a, rb, &cwd) == expected);

    // A gap's verdict is inverted: it is filed BECAUSE it differs, so the
    // failure worth reporting is that it stopped differing.
    match (suite.verdict, agrees) {
        (Verdict::Match, true) | (Verdict::Xfail, false) => Ok(()),
        (Verdict::Match, false) => Err(match (census_changed, &actual) {
            (Some(census), _) => census,
            (None, Ok(a)) => mismatch(
                rb,
                "output differs from the recording",
                &expected,
                &norm_answer(a, rb, &cwd),
            ),
            (None, Err(e)) => format!("{}: zeo failed to run it: {e}", rb.display()),
        }
        .into()),
        (Verdict::Xfail, true) => {
            let stem = rb.file_stem().unwrap_or_default().to_string_lossy();
            Err(format!(
                "GAP FIXED: {stem} now matches ruby. \
                 Promote it: `cargo xtask promote-gap {stem} <topic/area>`"
            )
            .into())
        }
    }
}

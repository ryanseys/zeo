//! One corpus program, run and held to its trailer.
//!
//! The suite says who recorded the trailer and whether zeo must match it;
//! the leg says how zeo runs the program; the directives say what the
//! program needs. Everything else is comparison.

use std::path::{Path, PathBuf};

use crate::case::{Answer, Case, Platform};
use crate::compare::{mismatch, norm_answer, split_gccheck};
use crate::legs::{Leg, Road, road_for, zeo_command};
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
fn run_once(
    leg: Leg,
    road: Road<'_>,
    case: &Case,
    rb: &Path,
    suite: &Suite,
    cwd: &Path,
    extra_debug: Option<&str>,
) -> Result<Answer, String> {
    if !matches!(road, Road::Backend("jit")) {
        // The AOT and cache roads link the archive, which a test run does
        // not build.
        crate::common::runtime_archive()?;
    }
    let mut cmd = zeo_command(leg, road, case, rb, suite, cwd, extra_debug)?;
    let bounds = if suite.whole_graph {
        Bounds::WHOLE_GRAPH
    } else {
        Bounds::ORDINARY
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

    let actual = run_once(leg, road_for(leg), &case, rb, suite, &cwd, None);

    // The zeo-vs-zeo differentials: the same program again with one
    // emission mode changed, and the answers must agree byte for byte.
    // Ruby is not consulted, so a gap's divergence must be the SAME
    // divergence in every mode. A program zeo cannot run at all is skipped
    // here: that failure is the case's own verdict below.
    if leg == Leg::Differential
        && let Ok(plain) = &actual
    {
        for (flag, label) in [
            ("no-typed-calls", "TYPED-EMISSION"),
            ("packaged-ids", "PACKAGED-ID"),
        ] {
            let other =
                run_once(leg, road_for(leg), &case, rb, suite, &cwd, Some(flag)).map_err(|e| {
                    format!(
                        "{}: the {flag} run failed where the plain one ran: {e}",
                        rb.display()
                    )
                })?;
            let (a, b) = (norm_answer(plain, rb, &cwd), norm_answer(&other, rb, &cwd));
            if a != b {
                return Err(mismatch(
                    rb,
                    &format!("{label} DIVERGENCE: the same program answers differently with `{flag}` on vs off (a miscompile in one mode)"),
                    &a,
                    &b,
                )
                .into());
            }
        }
        let (programs, packages) = crate::legs::packaged_cache()?;
        let road = Road::PackagedCache {
            programs: &programs,
            packages: &packages,
        };
        let packaged = run_once(leg, road, &case, rb, suite, &cwd, None);
        let a = norm_answer(plain, rb, &cwd);
        match packaged {
            Ok(p) if norm_answer(&p, rb, &cwd) == a => {}
            Ok(p) if !case.directives.pkggap => {
                return Err(mismatch(
                    rb,
                    "PACKAGED DIVERGENCE: the same program answers differently spliced vs linked against packaged gems (a miscompile in one road)",
                    &a,
                    &norm_answer(&p, rb, &cwd),
                )
                .into());
            }
            Ok(_) => {}
            Err(_) if case.directives.pkggap => {}
            Err(e) => {
                return Err(format!(
                    "{}: the packaged run failed where the plain one ran: {e}",
                    rb.display()
                )
                .into());
            }
        }
        if case.directives.pkggap
            && let Ok(p) = run_once(
                leg,
                Road::PackagedCache {
                    programs: &programs,
                    packages: &packages,
                },
                &case,
                rb,
                suite,
                &cwd,
                None,
            )
            && norm_answer(&p, rb, &cwd) == a
        {
            return Err(format!(
                "PACKAGED GAP FIXED: {}'s packaged road agrees with the spliced one now. Drop `#@ pkggap` and promote it.",
                rb.display()
            )
            .into());
        }
    }

    // Under the memcheck leg the runtime writes one census line to stderr at
    // exit. Take it out of the ordinary comparison and hold it to the
    // `#@ gccheck` directive on its own: a cycle alive at exit is not a
    // defect (`a << a` is supposed to build one), so the leg gates CHANGE,
    // never zero.
    let actual = match (leg, actual) {
        (Leg::Memcheck, Ok(mut a)) => {
            let (err, census) = split_gccheck(&a.stderr);
            a.stderr = err;
            let want = case.directives.gccheck.as_deref().unwrap_or("").trim_end();
            if census != want {
                return Err(format!(
                    "{}: the exit cycle census changed.\n  expected: {}\n  actual:   {}\n\
                     Record it as `#@ gccheck: <line>` at the top of the program (say which cycle it builds), \
                     or find what stopped the collector seeing it.",
                    rb.display(),
                    if want.is_empty() { "<no cycle>" } else { want },
                    if census.is_empty() { "<no cycle>" } else { &census },
                )
                .into());
            }
            Ok(a)
        }
        (_, other) => other,
    };

    let expected = norm_answer(expected, rb, &cwd);
    let matched = match &actual {
        Ok(a) => norm_answer(a, rb, &cwd) == expected,
        // zeo could not run it at all: it diverges.
        Err(_) => false,
    };
    match (suite.verdict, matched) {
        (Verdict::Match, true) => Ok(()),
        (Verdict::Match, false) => Err(match &actual {
            Ok(a) => mismatch(rb, "output differs from the recording", &expected, &norm_answer(a, rb, &cwd)),
            Err(e) => format!("{}: zeo failed to run it: {e}", rb.display()),
        }
        .into()),
        (Verdict::Xfail, true) if case.directives.pkggap => Ok(()),
        (Verdict::Xfail, true) => Err(format!(
            "GAP FIXED: {stem} now matches ruby. Promote it: `cargo xtask promote-gap {stem} <topic/area>`",
            stem = rb.file_stem().unwrap_or(rb.as_os_str()).to_string_lossy()
        )
        .into()),
        // A `#@ pkggap` gap must still match ruby on the spliced road; the
        // packaged road is what diverges.
        (Verdict::Xfail, false) if case.directives.pkggap => Err(match &actual {
            Ok(a) => mismatch(rb, "a pkggap must match ruby on the spliced road", &expected, &norm_answer(a, rb, &cwd)),
            Err(e) => format!("{}: zeo failed to run it: {e}", rb.display()),
        }
        .into()),
        (Verdict::Xfail, false) => Ok(()),
    }
}

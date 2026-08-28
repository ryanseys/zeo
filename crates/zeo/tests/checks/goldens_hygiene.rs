//! Golden-sidecar hygiene for the datatest suites (`tests/`, `tests/spinel/`,
//! `tests/gaps/`).
//!
//! Invariants that have each broken silently before, since none of them fails
//! loudly on its own -- they degrade into non-hermetic or unprotected tests.

use std::path::{Path, PathBuf};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/zeo sits two levels under the workspace root")
}

fn suite_dirs() -> Vec<PathBuf> {
    let root = repo_root().join("tests");
    let mut dirs = vec![root.clone(), root.join("spinel"), root.join("gaps")];
    // One subdirectory per gem under tests/gemtests/.
    if let Ok(entries) = std::fs::read_dir(root.join("gemtests")) {
        dirs.extend(
            entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_dir()),
        );
    }
    dirs
}

fn goldens() -> impl Iterator<Item = PathBuf> {
    suite_dirs().into_iter().flat_map(|dir| {
        std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.to_string_lossy().ends_with(".expected"))
            .collect::<Vec<_>>()
    })
}

#[test]
fn every_golden_uses_the_rb_sidecar_convention() {
    let misnamed: Vec<String> = goldens()
        .filter(|p| {
            let name = p.to_string_lossy();
            !name.ends_with(".rb.expected") && !name.ends_with(".rb.err.expected")
        })
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        misnamed.is_empty(),
        "goldens the harness will never find (rename to <stem>.rb.expected):\n{}",
        misnamed.join("\n")
    );
}

#[test]
fn no_golden_embeds_a_machine_specific_path() {
    // `/home/user` is a fixture constant some tests PRINT (a stubbed HOME),
    // so only genuinely machine-specific prefixes are flagged.
    const MARKERS: &[&str] = &["/Users/", ".local/share/mise"];
    let mut offenders = Vec::new();
    for p in goldens() {
        let content = std::fs::read_to_string(&p).unwrap_or_default();
        for m in MARKERS {
            if content.contains(m) {
                offenders.push(format!("{}: contains {m:?}", p.display()));
                break;
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "goldens with unportable absolute paths (re-record or scrub in-test):\n{}",
        offenders.join("\n")
    );
}

/// `tests/bench/` holds compile-side INPUT programs (whole-gem require
/// graphs), deliberately outside every datatest pattern: a golden appearing
/// there would silently never run, and a bench input gaining a golden means
/// someone thinks it is a test again -- whole-gem coverage belongs to the gem
/// probe, which is why `tests/gems/` was retired.
#[test]
fn bench_inputs_carry_no_goldens() {
    let dir = repo_root().join("tests").join("bench");
    let stray: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".expected"))
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        stray.is_empty(),
        "tests/bench/ is not a datatest suite; these goldens would never run:\n{}",
        stray.join("\n")
    );
}

/// A `.divergence` sidecar says `.expected` records ZEO's own output rather
/// than the oracle's, so it must have an `.expected` to describe, and it must
/// not sit in `tests/gaps/`: a gap is work, and a decided divergence is not.
/// Both mistakes are silent -- the first leaves `bless` recording zeo into a
/// file nothing reads, and the second leaves an XFAIL whose fix nobody
/// intends.
#[test]
fn every_divergence_sidecar_describes_a_committed_zeo_golden() {
    let mut wrong: Vec<String> = Vec::new();
    for dir in suite_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for path in entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.to_string_lossy().ends_with(".divergence"))
        {
            let name = path.to_string_lossy().to_string();
            if !name.ends_with(".rb.divergence") {
                wrong.push(format!("{name}: not a `.rb.divergence` sidecar"));
                continue;
            }
            if !Path::new(&name.replace(".divergence", ".expected")).exists() {
                wrong.push(format!("{name}: no `.expected` beside it"));
            }
            if dir.ends_with("gaps") {
                wrong.push(format!(
                    "{name}: a decided divergence belongs in tests/, not tests/gaps/"
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "divergence sidecars: {wrong:#?}");
}

/// The reverse direction of the check above: a golden whose HEADER claims a
/// deliberate divergence must carry the `.divergence` sidecar, or `bless`
/// records the oracle over zeo's own answers and destroys the golden. Found
/// the hard way: `singleton_body_class_and_self_path.rb` said "DELIBERATE
/// DIVERGENCE" for weeks with no sidecar protecting it.
#[test]
fn every_divergence_claim_carries_its_sidecar() {
    let mut unprotected: Vec<String> = Vec::new();
    for dir in suite_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for path in entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "rb"))
        {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            if text.contains("DELIBERATE DIVERGENCE")
                && !Path::new(&format!("{}.divergence", path.display())).exists()
            {
                unprotected.push(path.display().to_string());
            }
        }
    }
    assert!(
        unprotected.is_empty(),
        "goldens claiming a deliberate divergence with no `.divergence` sidecar \
         (bless would record the oracle over zeo's answers):\n{}",
        unprotected.join("\n")
    );
}

/// `tests/macos/` and `tests/jit/` silently REMOVE their goldens from every
/// other leg. Pin the exact membership so moving a file in is a deliberate
/// two-place change, never an accident that hides a red on the legs that no
/// longer run it.
#[test]
fn every_leg_skip_sidecar_is_acknowledged_here() {
    const MACOS_ONLY: &[&str] = &[
        "an_ffi_type_is_one_object_per_canonical_name.rb",
        "core_long_tail_rows.rb",
        "errno_full_surface.rb",
        "ffi_lib_defers_runtime_candidates.rb",
        "ffi_platform_reports_the_hosts_c_abi.rb",
        "process_identity_rows.rb",
        "socket_carries_its_exception_hierarchy.rb",
        "fiddle.rb",
        "float_pow_negative_fractional.rb",
        "io_file_stat_rows.rb",
    ];
    const JIT_ONLY: &[&str] = &["an_ffi_type_crosses_between_snippets.rb"];
    let mut found: Vec<(String, &'static str)> = Vec::new();
    for (sub, kind) in [("macos", "macos"), ("jit", "jit")] {
        let Ok(entries) = std::fs::read_dir(repo_root().join("tests").join(sub)) else {
            continue;
        };
        for path in entries.filter_map(Result::ok).map(|e| e.path()) {
            if path.extension().is_some_and(|e| e == "rb") {
                found.push((path.file_name().unwrap().to_string_lossy().to_string(), kind));
            }
        }
    }
    let mut unlisted: Vec<String> = found
        .iter()
        .filter(|(stem, kind)| {
            let list = if *kind == "macos" {
                MACOS_ONLY
            } else {
                JIT_ONLY
            };
            !list.contains(&stem.as_str())
        })
        .map(|(stem, kind)| format!("{stem} ({kind})"))
        .collect();
    let mut missing: Vec<String> = MACOS_ONLY
        .iter()
        .map(|s| (*s, "macos"))
        .chain(JIT_ONLY.iter().map(|s| (*s, "jit")))
        .filter(|(s, kind)| !found.iter().any(|(f, k)| f == s && k == kind))
        .map(|(s, kind)| format!("{s} ({kind})"))
        .collect();
    unlisted.sort();
    missing.sort();
    assert!(
        unlisted.is_empty() && missing.is_empty(),
        "every golden under tests/macos/ and tests/jit/ must be acknowledged in \
         this test's lists.\n\
         unacknowledged on disk: {unlisted:?}\nlisted but gone: {missing:?}"
    );
}

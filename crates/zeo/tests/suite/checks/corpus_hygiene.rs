//! What every program under `test/` must hold to, beyond passing.
//!
//! Each rule here once broke silently, since none fails loudly on its own:
//! they degrade into non-hermetic, self-referential or misread tests.

use std::path::{Path, PathBuf};

use crate::case::Case;
use crate::suites::{Depth, SUITES, Suite};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/zeo sits two levels under the workspace root")
}

/// Every program the harness runs, with its suite.
fn programs() -> Vec<(PathBuf, &'static Suite)> {
    let mut out = Vec::new();
    for suite in SUITES {
        for rel in suite.cases(repo_root()).expect("read the corpus") {
            out.push((repo_root().join(suite.root).join(rel), suite));
        }
    }
    out
}

fn program_text(path: &Path) -> String {
    let case = Case::read(path).unwrap_or_else(|e| panic!("{e}"));
    String::from_utf8_lossy(&case.program).into_owned()
}

/// Every program parses: its directives are known and its trailer is well
/// formed. The harness fails such a program too, but one at a time.
#[test]
fn every_program_parses() {
    let bad: Vec<String> = programs()
        .iter()
        .filter_map(|(p, _)| Case::read(p).err())
        .collect();
    assert!(
        bad.is_empty(),
        "programs the harness cannot read:\n{}",
        bad.join("\n")
    );
}

/// A program may not touch `DATA`: under ruby it is the trailer, under zeo it
/// is undefined, and a program that printed it would record its own answer.
/// A program that needs `__END__` data keeps it in a fixture it requires.
#[test]
fn no_program_reads_its_own_data_section() {
    let bad: Vec<String> = programs()
        .iter()
        .filter(|(p, _)| {
            program_text(p)
                .lines()
                .filter(|l| !l.trim_start().starts_with('#'))
                .any(names_data)
        })
        .map(|(p, _)| p.display().to_string())
        .collect();
    assert!(
        bad.is_empty(),
        "programs that name DATA (move the __END__ data into a required fixture):\n{}",
        bad.join("\n")
    );
}

/// `DATA` as its own word: not `SEEK_DATA`, not `IO::DATA`, not `DATA_X`.
fn names_data(line: &str) -> bool {
    let bytes = line.as_bytes();
    line.match_indices("DATA").any(|(i, _)| {
        let before = i.checked_sub(1).map(|j| bytes[j]);
        let after = bytes.get(i + 4).copied();
        let joins =
            |b: Option<u8>| b.is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b':');
        !joins(before) && !joins(after)
    })
}

/// A recorded answer that names this machine is not portable: another
/// machine records a different one and the case fails there. The markers
/// are the home directory, one version manager's install root, and the
/// ruby-install layout it and others share.
#[test]
fn no_answer_embeds_a_machine_specific_path() {
    let home = std::env::var("HOME").unwrap_or_default();
    let markers: Vec<&str> = [home.as_str(), ".local/share/mise", "/installs/ruby/"]
        .into_iter()
        .filter(|m| m.len() > 1)
        .collect();
    let bad: Vec<String> = programs()
        .iter()
        .filter_map(|(p, _)| {
            let case = Case::read(p).ok()?;
            let t = case.trailer?;
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&t.answer.stdout),
                String::from_utf8_lossy(&t.answer.stderr)
            );
            markers
                .iter()
                .any(|m| text.contains(m))
                .then(|| p.display().to_string())
        })
        .collect();
    assert!(
        bad.is_empty(),
        "answers naming a machine-specific path (the oracle's own files leak through a backtrace; \
         rescue the error in the program and print its class instead):\n{}",
        bad.join("\n")
    );
}

/// A suite's depth is its contract: a `.rb` one level deeper than the suite
/// reads is a fixture the harness never runs. That is right for a file a
/// program requires, and wrong for a test someone filed in a new
/// subdirectory. A directory at case depth must therefore be named after a
/// sibling program, or be `fixtures/`.
#[test]
fn every_directory_at_case_depth_belongs_to_a_program() {
    let mut bad = Vec::new();
    for suite in SUITES {
        let root = repo_root().join(suite.root);
        let case_dirs: Vec<PathBuf> = match suite.depth {
            Depth::One => vec![root.clone()],
            Depth::Two => std::fs::read_dir(&root)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect(),
        };
        for dir in case_dirs {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let allowed = name == "fixtures" || dir.join(format!("{name}.rb")).is_file();
                if !allowed {
                    bad.push(path.display().to_string());
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "directories the harness reads as fixtures but no program owns:\n{}",
        bad.join("\n")
    );
}

/// Two programs with identical bytes test one thing twice, under two names,
/// and both cost a compile and a run on every board. A rename pass left
/// nineteen such pairs: a descriptive twin beside the terse original it was
/// meant to replace.
///
/// `programs()` yields cases only, so a fixture two programs deliberately
/// share is not a duplicate here.
#[test]
fn no_two_cases_share_a_body() {
    let mut seen: std::collections::HashMap<Vec<u8>, PathBuf> = std::collections::HashMap::new();
    let mut bad = Vec::new();
    for (path, _) in programs() {
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        match seen.get(&bytes) {
            Some(first) => bad.push(format!(
                "{}\n    == {}",
                first.strip_prefix(repo_root()).unwrap_or(first).display(),
                path.strip_prefix(repo_root()).unwrap_or(&path).display()
            )),
            None => {
                seen.insert(bytes, path);
            }
        }
    }
    assert!(
        bad.is_empty(),
        "programs whose bytes are identical -- keep the one whose name says \
         what it checks, in the topic it belongs to:\n  {}",
        bad.join("\n  ")
    );
}

/// `cargo xtask promote-gap` moves a gap to `<topic>/<area>/<stem>.rb` and
/// refuses when that file already exists, so a gap sharing a basename with a
/// passing case cannot be promoted at all -- and `-E 'test(<stem>)'` matches
/// both, one of which must fail. A gap is named after the behaviour that
/// still differs, which no passing case is named after.
#[test]
fn no_gap_shares_a_basename_with_a_case() {
    let cases: std::collections::HashMap<String, PathBuf> = programs()
        .into_iter()
        .filter(|(_, suite)| suite.depth == Depth::Two)
        .map(|(p, _)| {
            (
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                p,
            )
        })
        .collect();
    let root = repo_root();
    let bad: Vec<String> = crate::suites::GAPS
        .cases(root)
        .expect("read the gaps")
        .iter()
        .filter_map(|rel| {
            let name = rel.file_name()?.to_string_lossy().into_owned();
            let case = cases.get(&name)?;
            Some(format!(
                "test/gaps/{name} -- {}",
                case.strip_prefix(root).unwrap_or(case).display()
            ))
        })
        .collect();
    assert!(
        bad.is_empty(),
        "gaps named like a passing case -- rename the gap after what still \
         differs:\n  {}",
        bad.join("\n  ")
    );
}

/// The `bundle_*` series: multi-section grab-bag programs whose names are an
/// index because no one behaviour describes them. Splitting them is separate
/// work; until then the naming rule allows them by name so it can hold every
/// other program.
const NUMBERED_BY_EXCEPTION: &str = "bundle_";

/// A tracker id is not a test name. Four hundred and seventy programs were
/// called `issue_NNNN.rb`, three hundred and fifty of them with no comment
/// either, so a red case named a number and nothing else.
///
/// The number is not lost: it moves into the header, where it can sit beside
/// what the program actually checks.
#[test]
fn no_case_is_named_by_a_number() {
    let mut bad = Vec::new();
    for (path, _) in programs() {
        let stem = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if stem.starts_with(NUMBERED_BY_EXCEPTION) {
            continue;
        }
        let rel = path.strip_prefix(repo_root()).unwrap_or(&path).display();
        if stem.starts_with("issue_")
            || stem.starts_with("issue") && stem[5..].starts_with(|c: char| c.is_ascii_digit())
        {
            bad.push(format!("{rel} -- an issue id is not a name"));
            continue;
        }
        // A trailing `_<digits>` is an index unless what precedes it is also
        // a number or a single letter: `_1_1` is a version, `_x_0212` a
        // standard's number.
        let Some((head, tail)) = stem.rsplit_once('_') else {
            continue;
        };
        if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) {
            let prev = head.rsplit('_').next().unwrap_or("");
            let excused = prev.len() == 1 || prev.chars().all(|c| c.is_ascii_digit());
            if !excused {
                bad.push(format!(
                    "{rel} -- a trailing number is an index, not a name"
                ));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "programs named by a number rather than by what they check:\n  {}",
        bad.join("\n  ")
    );
}

/// Programs that name a literal `/tmp` path on purpose: each one's answer
/// depends on the path itself, so a scratch directory would change what it
/// records. Everything else writes under `Dir.mktmpdir`.
const LITERAL_TMP_BY_DESIGN: &[&str] = &[
    // The errno message names both paths of a rename that fails, so nothing
    // is created and the message has to stay stable.
    "lang/exceptions/errno_messages_name_crubys_call_sites.rb",
];

/// A program that writes under a literal `/tmp/name` shares that name with
/// every case running beside it -- the golden group is fourteen wide -- and
/// leaves the file behind afterwards. `Dir.mktmpdir` honours the `TMPDIR` the
/// harness sets, so the writes land in the run's own scratch root and go with
/// it.
///
/// Naming a `/tmp` path without writing to it is fine: it is data.
#[test]
fn no_program_writes_under_a_literal_tmp_path() {
    const WRITERS: &[&str] = &[
        "File.write(",
        "File.open(",
        "Dir.mkdir(",
        "File.mkfifo(",
        "FileUtils.",
        "IO.write(",
        "File.new(",
        "File.symlink(",
    ];
    let mut bad = Vec::new();
    for (path, suite) in programs() {
        let rel = format!(
            "{}/{}",
            suite.root.strip_prefix("test/").unwrap_or(suite.root),
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        let text = program_text(&path);
        let code: String = text
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        if !code.contains("\"/tmp/") && !code.contains("'/tmp/") {
            continue;
        }
        if !WRITERS.iter().any(|w| code.contains(w)) {
            continue;
        }
        if LITERAL_TMP_BY_DESIGN.iter().any(|p| rel.ends_with(p)) {
            continue;
        }
        bad.push(
            path.strip_prefix(repo_root())
                .unwrap_or(&path)
                .display()
                .to_string(),
        );
    }
    assert!(
        bad.is_empty(),
        "programs that write under a literal /tmp path -- put the files under \
         `Dir.mktmpdir` (require \"tmpdir\") and never print the directory:\n  {}",
        bad.join("\n  ")
    );
}

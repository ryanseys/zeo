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
                .any(|l| names_data(l))
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
/// machine records a different one and the case fails there.
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
            Depth::One | Depth::OneAndPending => vec![root.clone()],
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
                let allowed = name == "fixtures"
                    || name == "compile"
                    || (suite.depth == Depth::OneAndPending && name == "pending")
                    || dir.join(format!("{name}.rb")).is_file()
                    || name.ends_with("_fixture")
                    || name == "lib";
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

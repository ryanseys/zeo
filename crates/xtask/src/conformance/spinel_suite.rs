//! Adapter for the C spinel project's golden corpus, encoding the conventions
//! of its Makefile's `RUN_ONE_TEST` recipe:
//!
//! - `test/*.rb` (top level only) are the tests; subdirectories other than
//!   `analyze_fail/` hold fixtures reached via relative `require`.
//! - `foo.rb.expected` = stdout snapshot; absent => live `ruby` oracle.
//! - `foo.rb.err.expected` = stderr snapshot; absent => stderr must be empty.
//! - `foo.rb.args` = whitespace-split ARGV (paths are repo-root-relative, so
//!   tests run with cwd = the corpus dir's parent).
//! - `foo.rb.stdin` = bytes fed to stdin.
//! - `test/analyze_fail/*.rb` must be rejected by the compiler.
//! - `test/rbs*` (RBS extraction) is out of scope and not discovered; a
//!   standing skiplist entry documents the decision.

use std::path::Path;

use super::suite::{Expectation, Suite, TestCase};

pub struct SpinelSuite;

impl Suite for SpinelSuite {
    fn name(&self) -> &'static str {
        "spinel"
    }

    fn root_env_var(&self) -> &'static str {
        "SPINEL_TEST_DIR"
    }

    fn discover(&self, root: &Path, _work_dir: &Path) -> Result<Vec<TestCase>, String> {
        if !root.join("analyze_fail").is_dir() {
            return Err(format!(
                "{} doesn't look like the spinel test corpus (no analyze_fail/ subdir); \
                 point --dir or $SPINEL_TEST_DIR at the spinel test corpus",
                root.display()
            ));
        }
        // `.args` files hold repo-root-relative paths, mirroring `make test`
        // running from the repo root.
        let run_cwd = root
            .parent()
            .ok_or_else(|| format!("{} has no parent directory", root.display()))?
            .to_path_buf();

        let mut cases = Vec::new();
        for entry in list_rb(root)? {
            let id = stem(&entry);
            let sidecar = |suffix: &str| {
                let p = root.join(format!("{id}.rb{suffix}"));
                p.exists().then_some(p)
            };
            let args = match sidecar(".args") {
                Some(p) => std::fs::read_to_string(&p)
                    .map_err(|e| format!("reading {}: {e}", p.display()))?
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect(),
                None => Vec::new(),
            };
            cases.push(TestCase {
                id: id.clone(),
                source: entry,
                args,
                stdin: sidecar(".stdin"),
                run_cwd: run_cwd.clone(),
                expectation: Expectation::Snapshot {
                    stdout: sidecar(".expected"),
                    stderr: sidecar(".err.expected"),
                },
            });
        }

        for entry in list_rb(&root.join("analyze_fail"))? {
            cases.push(TestCase {
                id: format!("analyze_fail/{}", stem(&entry)),
                source: entry,
                args: Vec::new(),
                stdin: None,
                run_cwd: run_cwd.clone(),
                expectation: Expectation::CompileFail,
            });
        }

        cases.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(cases)
    }
}

fn list_rb(dir: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("reading {}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "rb"))
        .collect();
    files.sort();
    Ok(files)
}

fn stem(path: &Path) -> String {
    path.file_stem().unwrap().to_string_lossy().into_owned()
}

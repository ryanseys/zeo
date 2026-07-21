//! Per-test result stamps under `target/conformance/<suite>/stamps/`, so warm
//! runs replay cached verdicts in seconds and `triage`/`--update-scoreboard`
//! never re-execute anything. A stamp is invalidated by any change to the
//! test's inputs (source + sidecars + args), the toolchain (zeo binary,
//! zeo-rt rlib), or the oracle's `ruby -v`.
//!
//! Format: one `key<TAB>escaped-value` pair per line -- trivially diffable,
//! no serde dependency.

use std::path::{Path, PathBuf};

use super::suite::{TestCase, TestResult, Verdict};
use super::util::{escape_line, fnv1a64, sanitize_id, unescape_line};

pub struct StampStore {
    dir: PathBuf,
    /// Combined toolchain fingerprint (zeo + rlib mtime/len + ruby -v).
    toolchain: String,
}

impl StampStore {
    pub fn new(dir: PathBuf, workspace_root: &Path, ruby_version: &str) -> Result<Self, String> {
        std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        let mut toolchain = String::new();
        for rel in ["target/debug/zeo", "target/debug/libzeo_rt.rlib"] {
            let p = workspace_root.join(rel);
            let meta = std::fs::metadata(&p)
                .map_err(|e| format!("stat {} (build zeo first): {e}", p.display()))?;
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            toolchain.push_str(&format!("{rel}:{}:{mtime};", meta.len()));
        }
        toolchain.push_str(ruby_version);
        Ok(StampStore { dir, toolchain })
    }

    /// Hash of everything that should invalidate this test's cached result.
    pub fn inputs_hash(&self, case: &TestCase) -> Result<u64, String> {
        let mut key = Vec::new();
        for path in inputs(case) {
            key.extend_from_slice(
                &std::fs::read(&path).map_err(|e| format!("reading {}: {e}", path.display()))?,
            );
            key.push(0);
        }
        for a in &case.args {
            key.extend_from_slice(a.as_bytes());
            key.push(0);
        }
        key.extend_from_slice(self.toolchain.as_bytes());
        Ok(fnv1a64(&key))
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{}.stamp", sanitize_id(id)))
    }

    /// A cached result, if the stamp exists and matches `hash`.
    pub fn load(&self, id: &str, hash: u64) -> Option<TestResult> {
        let (result, stored_hash) = self.parse(id)?;
        (stored_hash == format!("{hash:016x}")).then_some(result)
    }

    /// Load a stamp without freshness checking (for `triage`/`show`, which
    /// only read).
    pub fn load_any(&self, id: &str) -> Option<TestResult> {
        self.parse(id).map(|(result, _)| result)
    }

    fn parse(&self, id: &str) -> Option<(TestResult, String)> {
        let text = std::fs::read_to_string(self.path(id)).ok()?;
        let mut fields = std::collections::HashMap::new();
        for line in text.lines() {
            let (k, v) = line.split_once('\t')?;
            fields.insert(k.to_owned(), unescape_line(v));
        }
        let mut result = TestResult {
            id: id.to_owned(),
            verdict: Verdict::from_str(fields.get("verdict")?)?,
            stage: stage_str(fields.get("stage")?),
            bucket: fields.get("bucket")?.clone(),
            cluster: fields.get("cluster")?.clone(),
            stderr_tail: fields.get("stderr_tail").cloned().unwrap_or_default(),
            compile_ms: fields.get("compile_ms")?.parse().ok()?,
            run_ms: fields.get("run_ms")?.parse().ok()?,
            cached: true,
        };
        // Re-bucket from the recorded stderr at read time, so growing the
        // curated triage table retroactively reclassifies existing stamps
        // without re-running the corpus. (FAIL_RUSTC keeps its dedicated
        // bucket; skips keep their reason.)
        if matches!(
            result.verdict,
            Verdict::FailCompile | Verdict::FailOutput | Verdict::FailRun
        ) && !result.stderr_tail.is_empty()
        {
            let t = super::triage::classify(&result.stderr_tail);
            result.bucket = t.bucket;
            result.cluster = t.cluster;
        }
        Some((result, fields.remove("hash")?))
    }

    pub fn save(&self, result: &TestResult, hash: u64) -> Result<(), String> {
        let mut out = String::new();
        for (k, v) in [
            ("verdict", result.verdict.as_str().to_owned()),
            ("stage", result.stage.to_owned()),
            ("bucket", result.bucket.clone()),
            ("cluster", result.cluster.clone()),
            ("hash", format!("{hash:016x}")),
            ("compile_ms", result.compile_ms.to_string()),
            ("run_ms", result.run_ms.to_string()),
            ("stderr_tail", result.stderr_tail.clone()),
        ] {
            out.push_str(&format!("{k}\t{}\n", escape_line(&v)));
        }
        let path = self.path(&result.id);
        std::fs::write(&path, out).map_err(|e| format!("writing {}: {e}", path.display()))
    }
}

/// Every file whose content should invalidate the stamp.
fn inputs(case: &TestCase) -> Vec<PathBuf> {
    let mut v = vec![case.source.clone()];
    if let super::suite::Expectation::Snapshot { stdout, stderr } = &case.expectation {
        v.extend(stdout.clone());
        v.extend(stderr.clone());
    }
    v.extend(case.stdin.clone());
    v
}

fn stage_str(s: &str) -> &'static str {
    match s {
        "compile" => "compile",
        "run" => "run",
        "expect" => "expect",
        _ => "-",
    }
}

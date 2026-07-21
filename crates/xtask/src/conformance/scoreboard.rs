//! The committed conformance artifacts: `conformance/scoreboard.tsv` (one
//! diffable row per test -- rows change only when a verdict changes, so git
//! history shows exactly which tests flipped), `conformance/SCOREBOARD.md`
//! (human summary), and `conformance/TRIAGE.md` (gap buckets ranked by how
//! many tests they block -- the ordering signal for the implementation plan).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::suite::{TestResult, Verdict};

pub struct RunMeta<'a> {
    pub suite: &'a str,
    pub corpus: usize,
    pub ruby_version: &'a str,
    pub git_sha: &'a str,
}

/// The reference-material for one test, pulled from its `TestCase` so the
/// failures document can name the exact `.rb` source and `.expected` snapshot
/// (or say it's diffed live against the oracle). Keyed by test id.
pub struct CaseMeta {
    pub source: PathBuf,
    pub expected_stdout: Option<PathBuf>,
    pub expected_stderr: Option<PathBuf>,
    /// How the reference output is obtained: `snapshot`, `live-oracle`,
    /// `compile-fail`, or `self-report`.
    pub reference: &'static str,
}

pub fn write_all(
    dir: &Path,
    meta: &RunMeta,
    results: &[TestResult],
    case_meta: &BTreeMap<String, CaseMeta>,
    diff_dir: &Path,
) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    // Each suite gets its own committed artifacts so they don't clobber each
    // other; the original `zeo` suite keeps the historical unprefixed names.
    let prefix = if meta.suite == "zeo" { String::new() } else { format!("{}-", meta.suite) };
    write(dir.join(format!("{prefix}scoreboard.tsv")), tsv(meta, results))?;
    write(dir.join(format!("{prefix}SCOREBOARD.md")), summary_md(meta, results))?;
    write(dir.join(format!("{prefix}TRIAGE.md")), triage_md(meta, results))?;
    write(
        dir.join(format!("{prefix}FAILURES.md")),
        failures_md(meta, results, case_meta, diff_dir),
    )?;
    Ok(())
}

fn tsv(meta: &RunMeta, results: &[TestResult]) -> String {
    let mut out = format!(
        "# suite={} corpus={} ruby={:?} zeo-rs={}\n",
        meta.suite, meta.corpus, meta.ruby_version, meta.git_sha
    );
    out.push_str("# ");
    out.push_str(
        &verdict_counts(results)
            .iter()
            .map(|(v, n)| format!("{v}={n}"))
            .collect::<Vec<_>>()
            .join(" "),
    );
    out.push_str(&format!(" TOTAL={}\n", results.len()));
    for r in results {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            r.id,
            r.verdict.as_str(),
            r.stage,
            r.bucket
        ));
    }
    out
}

fn summary_md(meta: &RunMeta, results: &[TestResult]) -> String {
    let counts = verdict_counts(results);
    let total = results.len();
    let passed = counts
        .iter()
        .find(|(v, _)| *v == "PASS")
        .map_or(0, |(_, n)| *n);
    let pct = if total > 0 {
        passed as f64 / total as f64 * 100.0
    } else {
        0.0
    };
    let mut out = format!(
        "# Conformance scoreboard\n\n\
         Suite `{}` — **{passed}/{total} passing ({pct:.1}%)** — oracle `{}` — zeo-rs `{}`\n\n\
         | verdict | count |\n|---|---|\n",
        meta.suite, meta.ruby_version, meta.git_sha
    );
    for (v, n) in &counts {
        out.push_str(&format!("| {v} | {n} |\n"));
    }
    out.push_str(&format!("| **TOTAL** | **{total}** |\n"));

    out.push_str("\n## Top failure categories\n\n");
    let ranked = ranked_buckets(results);
    if ranked.is_empty() {
        out.push_str("(none)\n");
    } else {
        out.push_str(
            "| blocked | bucket | cluster | sample test | sample message |\n|---|---|---|---|---|\n",
        );
        for b in ranked.iter().take(10) {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                b.count,
                b.bucket,
                b.cluster,
                b.sample_id,
                b.sample_message.replace('|', "\\|")
            ));
        }
    }

    out.push_str("\n## Skipped tests\n\n");
    let skips: Vec<_> = results
        .iter()
        .filter(|r| r.verdict == Verdict::Skip)
        .collect();
    if skips.is_empty() {
        out.push_str("(none)\n");
    } else {
        out.push_str("| test | reason |\n|---|---|\n");
        for r in skips {
            out.push_str(&format!("| {} | {} |\n", r.id, r.stderr_tail));
        }
    }
    out.push_str(
        "\nRegenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.\n",
    );
    out
}

fn triage_md(meta: &RunMeta, results: &[TestResult]) -> String {
    let ranked = ranked_buckets(results);

    let mut out = format!(
        "# Gap triage\n\n\
         Failing tests grouped by normalized failure message, ranked by how many\n\
         tests each gap blocks. Clusters refer to the implementation plan's gap\n\
         families. Oracle `{}`.\n\n\
         | cluster | bucket | blocked | sample tests | sample message |\n|---|---|---|---|---|\n",
        meta.ruby_version
    );
    for b in ranked {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            b.cluster,
            b.bucket,
            b.count,
            b.sample_ids.join(", "),
            b.sample_message.replace('|', "\\|")
        ));
    }
    out.push_str(
        "\nList one bucket's tests: `cargo run -p xtask -- conformance triage --bucket <name>`.\n",
    );
    out
}

/// The full per-failure debug dump: every non-passing test with its `.rb`
/// source path, the reference it was diffed against (`.expected` snapshot or the
/// live oracle), verdict/bucket, captured stderr, and the complete
/// expected-vs-actual diff. This is the document you read to understand a
/// failure end-to-end without re-running `conformance show <id>` for each one.
///
/// The diff bodies come from the per-test `.diff` files the runner already
/// persists under `diff_dir`; a verdict with no diff file (a compile failure,
/// a run timeout) still gets its source path, verdict, and stderr.
fn failures_md(
    meta: &RunMeta,
    results: &[TestResult],
    case_meta: &BTreeMap<String, CaseMeta>,
    diff_dir: &Path,
) -> String {
    let failures: Vec<&TestResult> = results
        .iter()
        .filter(|r| !matches!(r.verdict, Verdict::Pass | Verdict::Skip))
        .collect();

    let mut out = format!(
        "# Conformance failures — full detail\n\n\
         Suite `{}` — **{} failing test(s)** — oracle `{}` — zeo-rs `{}`\n\n\
         Every non-passing test with its source path, the reference it is diffed\n\
         against, verdict/bucket, captured stderr, and the full expected-vs-actual\n\
         diff — enough to understand each failure without re-running the harness.\n\
         Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.\n\n",
        meta.suite,
        failures.len(),
        meta.ruby_version,
        meta.git_sha,
    );

    if failures.is_empty() {
        out.push_str("(no failing tests)\n");
        return out;
    }

    for r in &failures {
        out.push_str(&format!("## `{}` — {}\n\n", r.id, r.verdict.as_str()));
        if let Some(cm) = case_meta.get(&r.id) {
            out.push_str(&format!("- source: `{}`\n", cm.source.display()));
            match cm.reference {
                "snapshot" => {
                    if let Some(p) = &cm.expected_stdout {
                        out.push_str(&format!("- expected stdout: `{}`\n", p.display()));
                    }
                    match &cm.expected_stderr {
                        Some(p) => out.push_str(&format!("- expected stderr: `{}`\n", p.display())),
                        None => out.push_str("- expected stderr: *(must be empty)*\n"),
                    }
                }
                "live-oracle" => {
                    out.push_str(&format!("- reference: live oracle `{}`\n", meta.ruby_version))
                }
                "compile-fail" => {
                    out.push_str("- reference: zeo must reject the program (compile-fail)\n")
                }
                "self-report" => {
                    out.push_str("- reference: self-reported pass/fail (mspec_lite summary)\n")
                }
                _ => {}
            }
        }
        out.push_str(&format!(
            "- verdict: {} (stage `{}`) · bucket `{}` (cluster `{}`)\n\n",
            r.verdict.as_str(),
            r.stage,
            r.bucket,
            r.cluster
        ));

        if !r.stderr_tail.trim().is_empty() {
            push_fenced(&mut out, "stderr", r.stderr_tail.trim_end());
        }

        let diff_path = diff_dir.join(format!("{}.diff", super::util::sanitize_id(&r.id)));
        match std::fs::read_to_string(&diff_path) {
            Ok(diff) if !diff.trim().is_empty() => push_fenced(&mut out, "diff", diff.trim_end()),
            _ => {}
        }

        out.push_str("---\n\n");
    }
    out
}

/// Emit a labelled fenced code block whose fence is guaranteed longer than any
/// backtick run inside `body`, so arbitrary program output (which can itself
/// contain backticks) never breaks the block.
fn push_fenced(out: &mut String, label: &str, body: &str) {
    let fence = "`".repeat(longest_backtick_run(body).max(2) + 1);
    out.push_str(&format!("{label}:\n{fence}\n{body}\n{fence}\n\n"));
}

fn longest_backtick_run(s: &str) -> usize {
    let (mut max, mut cur) = (0usize, 0usize);
    for c in s.chars() {
        if c == '`' {
            cur += 1;
            max = max.max(cur);
        } else {
            cur = 0;
        }
    }
    max
}

pub struct BucketStat {
    pub cluster: String,
    pub bucket: String,
    pub count: usize,
    /// The representative test for this bucket -- the first of `sample_ids`.
    pub sample_id: String,
    /// Up to three member tests, spread across the (sorted) member list rather
    /// than always the alphabetically-first, so the table shows the bucket's
    /// variety. Deterministic (stable across runs given the same members) so
    /// the committed report only churns when the underlying set changes.
    pub sample_ids: Vec<String>,
    /// The ACTIONABLE failure message of `sample_id` -- the real panic body /
    /// clean rejection, never the `note: run with RUST_BACKTRACE=1` trailer.
    pub sample_message: String,
}

/// Failure buckets ranked by how many tests each blocks (ties broken by name).
pub fn ranked_buckets(results: &[TestResult]) -> Vec<BucketStat> {
    let mut ranked: Vec<BucketStat> = bucket_stats(results).into_values().collect();
    ranked.sort_by(|a, b| b.count.cmp(&a.count).then(a.bucket.cmp(&b.bucket)));
    ranked
}

/// One member of a failure bucket: its test id and extracted message.
struct Member {
    id: String,
    message: String,
}

pub fn bucket_stats(results: &[TestResult]) -> BTreeMap<String, BucketStat> {
    // First gather every failing test's (id, real message) under its bucket,
    // keeping the cluster label (same for all members of a bucket).
    let mut members: BTreeMap<String, (String, Vec<Member>)> = BTreeMap::new();
    for r in results {
        if r.bucket == "-" || r.verdict == Verdict::Pass || r.verdict == Verdict::Skip {
            continue;
        }
        let entry = members
            .entry(r.bucket.clone())
            .or_insert_with(|| (r.cluster.clone(), Vec::new()));
        entry.1.push(Member {
            id: r.id.clone(),
            message: super::triage::extract_message(&r.stderr_tail),
        });
    }

    members
        .into_iter()
        .map(|(bucket, (cluster, mut mem))| {
            mem.sort_by(|a, b| a.id.cmp(&b.id));
            let count = mem.len();
            let sample_ids = spread_samples(&bucket, &mem, 3);
            // The representative (first shown) drives the one-line message.
            let sample_id = sample_ids[0].clone();
            let sample_message = mem
                .iter()
                .find(|m| m.id == sample_id)
                .map(|m| m.message.clone())
                .unwrap_or_default();
            (
                bucket.clone(),
                BucketStat {
                    cluster,
                    bucket,
                    count,
                    sample_id,
                    sample_ids,
                    sample_message,
                },
            )
        })
        .collect()
}

/// Pick up to `n` member ids spread across the sorted list, starting at a
/// bucket-name-seeded offset (so the pick isn't always the alphabetically
/// first, giving a "random" feel) while staying fully deterministic.
fn spread_samples(bucket: &str, members: &[Member], n: usize) -> Vec<String> {
    let count = members.len();
    if count == 0 {
        return Vec::new();
    }
    let start = (super::util::fnv1a64(bucket.as_bytes()) as usize) % count;
    let take = n.min(count);
    let stride = (count / take).max(1);
    let mut picked = Vec::with_capacity(take);
    for i in 0..take {
        let idx = (start + i * stride) % count;
        let id = members[idx].id.clone();
        if !picked.contains(&id) {
            picked.push(id);
        }
    }
    picked
}

/// Every verdict with its count, in `Verdict::ALL` order -- categories with
/// zero tests still appear (as `0`) so the report never hides a category.
pub fn verdict_counts(results: &[TestResult]) -> Vec<(&'static str, usize)> {
    Verdict::ALL
        .iter()
        .map(|v| {
            let n = results.iter().filter(|r| r.verdict == *v).count();
            (v.as_str(), n)
        })
        .collect()
}

fn write(path: std::path::PathBuf, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| format!("writing {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(id: &str, verdict: Verdict, stderr: &str) -> TestResult {
        TestResult {
            id: id.to_owned(),
            verdict,
            stage: "expect",
            bucket: "auto-x".to_owned(),
            cluster: "?".to_owned(),
            stderr_tail: stderr.to_owned(),
            compile_ms: 0,
            run_ms: 0,
            cached: false,
        }
    }

    #[test]
    fn fence_outgrows_backtick_runs_in_the_body() {
        // A body containing a ``` run must be wrapped in a longer (````) fence,
        // or the block would terminate early and corrupt the document.
        assert_eq!(longest_backtick_run("a ``` b"), 3);
        let mut out = String::new();
        push_fenced(&mut out, "diff", "before ``` after");
        assert!(out.contains("````\nbefore ``` after\n````"), "{out}");
    }

    #[test]
    fn failures_md_embeds_source_reference_and_persisted_diff() {
        let dir = std::env::temp_dir().join(format!("sb_fail_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("t1.diff"), "=== stdout diff ===\nexpected X, got Y\n").unwrap();

        let meta = RunMeta { suite: "zeo", corpus: 2, ruby_version: "ruby 4.0.5", git_sha: "abc123" };
        let results = vec![result("t1", Verdict::FailOutput, "boom"), result("p1", Verdict::Pass, "")];
        let mut case_meta = BTreeMap::new();
        case_meta.insert(
            "t1".to_owned(),
            CaseMeta {
                source: PathBuf::from("/corpus/t1.rb"),
                expected_stdout: Some(PathBuf::from("/corpus/t1.rb.expected")),
                expected_stderr: None,
                reference: "snapshot",
            },
        );

        let md = failures_md(&meta, &results, &case_meta, &dir);
        std::fs::remove_dir_all(&dir).ok();

        assert!(md.contains("1 failing test"), "{md}");
        assert!(md.contains("## `t1` — FAIL_OUTPUT"), "{md}");
        assert!(md.contains("- source: `/corpus/t1.rb`"), "{md}");
        assert!(md.contains("- expected stdout: `/corpus/t1.rb.expected`"), "{md}");
        assert!(md.contains("expected X, got Y"), "{md}");
        // Passing tests never appear in the failures document.
        assert!(!md.contains("p1"), "{md}");
    }
}

//! The committed conformance artifacts: `conformance/scoreboard.tsv` (one
//! diffable row per test -- rows change only when a verdict changes, so git
//! history shows exactly which tests flipped), `conformance/SCOREBOARD.md`
//! (human summary), and `conformance/TRIAGE.md` (gap buckets ranked by how
//! many tests they block -- the ordering signal for the implementation plan).

use std::collections::BTreeMap;
use std::path::Path;

use super::suite::{TestResult, Verdict};

pub struct RunMeta<'a> {
    pub suite: &'a str,
    pub corpus: usize,
    pub ruby_version: &'a str,
    pub git_sha: &'a str,
}

pub fn write_all(dir: &Path, meta: &RunMeta, results: &[TestResult]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    write(dir.join("scoreboard.tsv"), tsv(meta, results))?;
    write(dir.join("SCOREBOARD.md"), summary_md(meta, results))?;
    write(dir.join("TRIAGE.md"), triage_md(meta, results))?;
    Ok(())
}

fn tsv(meta: &RunMeta, results: &[TestResult]) -> String {
    let mut out = format!(
        "# suite={} corpus={} ruby={:?} spinel-rs={}\n",
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
         Suite `{}` — **{passed}/{total} passing ({pct:.1}%)** — oracle `{}` — spinel-rs `{}`\n\n\
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
        out.push_str("| blocked | bucket | cluster | sample test |\n|---|---|---|---|\n");
        for b in ranked.iter().take(10) {
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                b.count, b.bucket, b.cluster, b.sample_id
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
         | cluster | bucket | blocked | sample test | sample message |\n|---|---|---|---|---|\n",
        meta.ruby_version
    );
    for b in ranked {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            b.cluster,
            b.bucket,
            b.count,
            b.sample_id,
            b.sample_message.replace('|', "\\|")
        ));
    }
    out.push_str(
        "\nList one bucket's tests: `cargo run -p xtask -- conformance triage --bucket <name>`.\n",
    );
    out
}

pub struct BucketStat {
    pub cluster: String,
    pub bucket: String,
    pub count: usize,
    pub sample_id: String,
    pub sample_message: String,
}

/// Failure buckets ranked by how many tests each blocks (ties broken by name).
pub fn ranked_buckets(results: &[TestResult]) -> Vec<BucketStat> {
    let mut ranked: Vec<BucketStat> = bucket_stats(results).into_values().collect();
    ranked.sort_by(|a, b| b.count.cmp(&a.count).then(a.bucket.cmp(&b.bucket)));
    ranked
}

pub fn bucket_stats(results: &[TestResult]) -> BTreeMap<String, BucketStat> {
    let mut buckets: BTreeMap<String, BucketStat> = BTreeMap::new();
    for r in results {
        if r.bucket == "-" || r.verdict == Verdict::Pass || r.verdict == Verdict::Skip {
            continue;
        }
        buckets
            .entry(r.bucket.clone())
            .and_modify(|b| b.count += 1)
            .or_insert_with(|| BucketStat {
                cluster: r.cluster.clone(),
                bucket: r.bucket.clone(),
                count: 1,
                sample_id: r.id.clone(),
                sample_message: r.stderr_tail.lines().last().unwrap_or("").to_owned(),
            });
    }
    buckets
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

//! What the last test run cost, read back out of its own JUnit.
//!
//! `.config/nextest.toml` writes `target/nextest/<profile>/junit.xml` on
//! every run. The numbers the suite is tuned against (the `golden` group
//! width, the slowest tests) go stale in place unless something reads it
//! back; a comment saying "re-measure" is not a way to re-measure.
//!
//! Nothing here collects anything. It parses the file nextest already wrote.

use std::path::PathBuf;

use crate::{Error, root_join};

const USAGE: &str = "\
usage: cargo xtask timings [--profile <name>] [--slowest <n>] [--json]

Reads target/nextest/<profile>/junit.xml -- written by every `cargo nextest
run` -- and reports what the run cost.

  --profile <name>   which run to read (default, full). Default: default
  --slowest <n>      how many of the heaviest cases to list. Default: 20
  --json             machine-readable, for comparing two runs

A group is <binary>::<first segment of the case name>, which is the suite for
a corpus case and the module for a unit test.
";

/// One case: what it cost, and where it lives.
struct Case {
    group: String,
    name: String,
    secs: f64,
}

/// A group's roll-up. `secs` is sorted on the way out, for the median.
struct Group {
    name: String,
    secs: Vec<f64>,
}

impl Group {
    fn total(&self) -> f64 {
        self.secs.iter().sum()
    }

    fn mean(&self) -> f64 {
        self.total() / self.secs.len() as f64
    }

    /// The lower of the two middles on an even count: a case time, never an
    /// average of two, so the number names a real case.
    fn median(&self) -> f64 {
        self.secs[(self.secs.len() - 1) / 2]
    }
}

pub fn run(args: &[String]) -> Result<(), Error> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return Ok(());
    }
    let mut profile = "default".to_string();
    let mut slowest = 20usize;
    let json = args.iter().any(|a| a == "--json");
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--profile" => {
                profile = rest
                    .next()
                    .ok_or_else(|| Error::new(format!("--profile needs a name\n\n{USAGE}")))?
                    .clone();
            }
            "--slowest" => {
                slowest = rest
                    .next()
                    .and_then(|n| n.parse().ok())
                    .ok_or_else(|| Error::new(format!("--slowest needs a number\n\n{USAGE}")))?;
            }
            "--json" => {}
            other => return Err(Error::new(format!("unexpected {other:?}\n\n{USAGE}"))),
        }
    }

    let path = junit_path(&profile);
    let xml = std::fs::read_to_string(&path).map_err(|e| {
        Error::new(format!(
            "{}: {e}\nRun `cargo nextest run{}` first -- the report is a by-product of a run.",
            path.display(),
            if profile == "default" {
                String::new()
            } else {
                format!(" -P {profile}")
            }
        ))
    })?;

    let wall = attr(&xml, "time")
        .and_then(|t| t.parse().ok())
        .unwrap_or(0.0);
    let cases = parse_cases(&xml);
    if cases.is_empty() {
        return Err(Error::new(format!("{}: no cases in it", path.display())));
    }
    if json {
        print_json(&path, wall, &cases);
    } else {
        report(&path, wall, cases, slowest);
    }
    Ok(())
}

fn junit_path(profile: &str) -> PathBuf {
    root_join("target/nextest").join(profile).join("junit.xml")
}

/// The first `key="value"` in `xml`. Enough for the root element's own
/// attributes, which is all this reads outside a `<testcase>`.
fn attr<'a>(xml: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!(" {key}=\"");
    let start = xml.find(&needle)? + needle.len();
    let end = xml[start..].find('"')? + start;
    Some(&xml[start..end])
}

/// Every `<testcase>`, by its own attributes.
///
/// Hand-rolled rather than an XML crate: a `<testcase>` element carries
/// `name`, `classname` and `time` on one line and nests nothing this needs,
/// so the parse is a scan and the alternative is a dependency the workspace
/// does not otherwise carry.
fn parse_cases(xml: &str) -> Vec<Case> {
    let mut out = Vec::new();
    for tag in xml.split("<testcase").skip(1) {
        let head = &tag[..tag.find('>').unwrap_or(tag.len())];
        let (Some(name), Some(class), Some(secs)) = (
            attr(head, "name"),
            attr(head, "classname"),
            attr(head, "time"),
        ) else {
            continue;
        };
        let Ok(secs) = secs.parse::<f64>() else {
            continue;
        };
        let head = name.split("::").next().unwrap_or(name);
        out.push(Case {
            group: format!("{class}::{head}"),
            name: name.to_string(),
            secs,
        });
    }
    out
}

fn report(path: &std::path::Path, wall: f64, mut cases: Vec<Case>, slowest: usize) {
    let cpu: f64 = cases.iter().map(|c| c.secs).sum();
    let threads = std::thread::available_parallelism().map_or(0, |n| n.get());

    println!("{}", path.display());
    print!("  {} cases   CPU {}", cases.len(), secs(cpu));
    if wall > 0.0 {
        print!(
            "   wall {}   {:.1}x parallel",
            secs(wall),
            cpu / wall.max(0.001)
        );
    }
    if threads > 0 {
        print!(" of {threads} threads");
    }
    println!("\n");

    let mut groups = group(&cases);
    groups.sort_by(|a, b| b.total().total_cmp(&a.total()));
    println!(
        "{:<44} {:>6} {:>9} {:>8} {:>8}",
        "group", "cases", "CPU", "mean", "median"
    );
    for g in &groups {
        println!(
            "{:<44} {:>6} {:>9} {:>8} {:>8}",
            g.name,
            g.secs.len(),
            secs(g.total()),
            millis(g.mean()),
            millis(g.median()),
        );
    }

    cases.sort_by(|a, b| b.secs.total_cmp(&a.secs));
    println!("\nslowest {}:", slowest.min(cases.len()));
    for c in cases.iter().take(slowest) {
        println!("  {:>9}  {}", secs(c.secs), c.name);
    }

    // Where the mass is. A run whose slowest 1% is a third of its CPU is a
    // different problem from one that is flat, and the two want opposite
    // fixes: a handful of programs, or the cost of a case.
    println!();
    for pct in [1.0, 5.0, 25.0] {
        let k = ((cases.len() as f64 * pct / 100.0) as usize).max(1);
        let share: f64 = cases[..k].iter().map(|c| c.secs).sum();
        println!(
            "  the slowest {pct:>4.0}% ({k:>4} cases) is {:>4.1}% of CPU",
            share / cpu * 100.0
        );
    }
}

fn print_json(path: &std::path::Path, wall: f64, cases: &[Case]) {
    let cpu: f64 = cases.iter().map(|c| c.secs).sum();
    let groups: Vec<_> = group(cases)
        .into_iter()
        .map(|g| {
            serde_json::json!({
                "group": g.name,
                "cases": g.secs.len(),
                "cpu": g.total(),
                "mean": g.mean(),
                "median": g.median(),
            })
        })
        .collect();
    let out = serde_json::json!({
        "junit": path.display().to_string(),
        "cases": cases.len(),
        "cpu": cpu,
        "wall": wall,
        "groups": groups,
    });
    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
}

fn group(cases: &[Case]) -> Vec<Group> {
    let mut by_name: std::collections::BTreeMap<&str, Vec<f64>> = std::collections::BTreeMap::new();
    for c in cases {
        by_name.entry(&c.group).or_default().push(c.secs);
    }
    by_name
        .into_iter()
        .map(|(name, mut secs)| {
            secs.sort_by(f64::total_cmp);
            Group {
                name: name.to_string(),
                secs,
            }
        })
        .collect()
}

fn secs(v: f64) -> String {
    format!("{v:.1}s")
}

fn millis(v: f64) -> String {
    format!("{:.0}ms", v * 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0"?>
<testsuites name="nextest-run" tests="3" time="10.5">
    <testsuite name="zeo::corpus" tests="3">
        <testcase name="core::string/upcase.rb" classname="zeo::corpus" time="0.100">
        </testcase>
        <testcase name="core::io/read.rb" classname="zeo::corpus" time="2.000">
        </testcase>
        <testcase name="lang::blocks/yield.rb" classname="zeo::corpus" time="0.300">
        </testcase>
    </testsuite>
</testsuites>"#;

    #[test]
    fn a_case_carries_its_binary_and_its_first_name_segment() {
        let cases = parse_cases(XML);
        assert_eq!(cases.len(), 3);
        assert_eq!(cases[0].group, "zeo::corpus::core");
        assert_eq!(cases[2].group, "zeo::corpus::lang");
        assert_eq!(cases[1].name, "core::io/read.rb");
        assert_eq!(cases[1].secs, 2.0);
    }

    #[test]
    fn the_median_is_a_real_case_time_not_an_average() {
        let groups = group(&parse_cases(XML));
        let core = groups.iter().find(|g| g.name.ends_with("::core")).unwrap();
        assert_eq!(core.secs.len(), 2);
        // 0.1 and 2.0: the lower middle, not 1.05.
        assert_eq!(core.median(), 0.1);
    }

    #[test]
    fn the_root_elements_time_is_the_runs_wall_clock() {
        assert_eq!(attr(XML, "time"), Some("10.5"));
    }
}

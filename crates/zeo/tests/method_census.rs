//! Gates zeo's method and constant coverage against CRuby, and ratchets it.
//!
//! `tools/method_census.rb` walks every `Module` reachable from `Object`'s
//! constant tree and dumps what each one owns. It runs unchanged under both
//! engines: `cargo run -p xtask -- method-census` records the oracle side into
//! `conformance/method-census.tsv`, and this test compiles the same walker
//! through zeo and diffs the two, so no ruby is needed at test time.
//!
//! Every gap it finds must have a row in `conformance/method-census-gaps.tsv`.
//! A NEW gap fails; a row that no longer diverges must be DELETED, so the file
//! can only shrink. `ZEO_BLESS=1` rewrites it from the current state, the same
//! convention the golden corpus and the arity ledger use.
//!
//! Four tags, because the four need completely different work:
//!
//! - `absent-module` -- CRuby reaches a module zeo does not have at all. Its
//!   methods are NOT listed on top of this; the module row implies them.
//! - `unreachable` -- a shared module owns a method zeo cannot answer from
//!   anywhere. This is the only tag a user meets as `NoMethodError`.
//! - `owner` -- zeo answers the call, but off a different class than CRuby, so
//!   only `.owner`/`instance_methods(false)` disagree. Splitting this off is
//!   the whole reason the dump carries the inherit-true sets.
//! - `constant` -- a shared module is missing one of CRuby's constants.
//!
//! Methods zeo has and CRuby does not are deliberately NOT gated here. Every
//! exception class registers the shared `Exception` natives on its own id (flat
//! dispatch), so that direction is dominated by a design choice rather than by
//! bugs, and would bury the rows that matter.
//!
//! PUBLIC and PRIVATE instance methods are both gated. Only the public sets
//! were, at first, and a private row drifts precisely because nothing calling
//! it can tell: `Exception` declares its own `method_missing` and
//! `respond_to_missing?`, zeo answered both off `BasicObject`/`Kernel`, and the
//! whole suite stayed green. Adding the private column found 114 more rows on
//! the day it landed, of which 56 are two patterns -- a private `initialize`
//! (39 classes) and `initialize_copy` (17), each owned one class off.

mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const ORACLE: &str = "conformance/method-census.tsv";
const GAPS: &str = "conformance/method-census-gaps.tsv";
const WALKER: &str = "tools/method_census.rb";

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// `module -> kind -> sorted names`, the dump's own shape.
type Census = BTreeMap<String, BTreeMap<String, Vec<String>>>;

fn parse(dump: &str) -> Census {
    let mut out = Census::new();
    for line in dump.lines() {
        let mut cols = line.splitn(3, '\t');
        let (Some(module), Some(kind)) = (cols.next(), cols.next()) else {
            continue;
        };
        let names = cols
            .next()
            .unwrap_or("")
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        out.entry(module.to_owned())
            .or_default()
            .insert(kind.to_owned(), names);
    }
    out
}

/// One row of the ledger. `detail` is the module for `absent-module` and the
/// method or constant name otherwise; `kind` is the dump column it came from.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Gap {
    tag: &'static str,
    module: String,
    kind: &'static str,
    name: String,
}

fn diff(oracle: &Census, zeo: &Census) -> Vec<Gap> {
    let mut gaps = Vec::new();
    for (module, kinds) in oracle {
        let Some(mine) = zeo.get(module) else {
            gaps.push(Gap {
                tag: "absent-module",
                module: module.clone(),
                kind: "-",
                name: "-".to_owned(),
            });
            continue;
        };
        let empty = Vec::new();
        let names = |src: &BTreeMap<String, Vec<String>>, k: &str| -> BTreeSet<String> {
            src.get(k).unwrap_or(&empty).iter().cloned().collect()
        };
        // `own` is what the module itself declares, `all` what it can answer
        // from anywhere. A name missing from BOTH raises; missing from `own`
        // alone means zeo files it under a different class.
        //
        // PRIVATE rows are gated too. They are invisible to a program that only
        // calls methods, which is exactly why they drift: `Exception`'s own
        // `method_missing`/`respond_to_missing?` sat on `BasicObject`/`Kernel`
        // for as long as this compared public sets alone, and nothing failed.
        for (own, all) in [("i", "I"), ("s", "S"), ("p", "P")] {
            let reachable = names(mine, all);
            for name in names(oracle.get(module).unwrap(), own).difference(&names(mine, own)) {
                gaps.push(Gap {
                    tag: if reachable.contains(name) {
                        "owner"
                    } else {
                        "unreachable"
                    },
                    module: module.clone(),
                    kind: own,
                    name: name.clone(),
                });
            }
        }
        for name in names(kinds, "c").difference(&names(mine, "c")) {
            gaps.push(Gap {
                tag: "constant",
                module: module.clone(),
                kind: "c",
                name: name.clone(),
            });
        }
    }
    gaps.sort();
    gaps
}

fn load_gaps(root: &Path) -> BTreeSet<(String, String, String, String)> {
    std::fs::read_to_string(root.join(GAPS))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let c: Vec<&str> = l.splitn(4, '\t').collect();
            (c.len() == 4).then(|| {
                (
                    c[0].to_owned(),
                    c[1].to_owned(),
                    c[2].to_owned(),
                    c[3].to_owned(),
                )
            })
        })
        .collect()
}

fn write_gaps(root: &Path, gaps: &[Gap]) {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for g in gaps {
        *counts.entry(g.tag).or_default() += 1;
    }
    let mut out = String::from(
        "# Coverage gaps against ruby 4.0.6, gated by crates/zeo/tests/method_census.rs.\n\
         # Columns: tag <TAB> module <TAB> kind <TAB> name. Regenerate with ZEO_BLESS=1.\n\
         # This file may only SHRINK -- a new gap fails the test, a closed one must be deleted.\n",
    );
    for (tag, n) in &counts {
        out.push_str(&format!("# {tag}: {n}\n"));
    }
    for g in gaps {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            g.tag, g.module, g.kind, g.name
        ));
    }
    std::fs::write(root.join(GAPS), out).expect("cannot write the gap ledger");
}

#[test]
fn method_coverage_only_shrinks() {
    let root = root();
    let oracle = parse(
        &std::fs::read_to_string(root.join(ORACLE))
            .unwrap_or_else(|e| panic!("cannot read {ORACLE}: {e}")),
    );
    let walker = std::fs::read_to_string(root.join(WALKER))
        .unwrap_or_else(|e| panic!("cannot read {WALKER}: {e}"));

    let run = support::run_ruby(&walker);
    assert!(
        run.status.success(),
        "the census walker did not run under zeo:\n{}",
        run.stderr
    );
    let found = diff(&oracle, &parse(&run.stdout));

    if std::env::var_os("ZEO_BLESS").is_some() {
        write_gaps(&root, &found);
        eprintln!("blessed {GAPS}: {} rows", found.len());
        return;
    }

    let accepted = load_gaps(&root);
    let key = |g: &Gap| {
        (
            g.tag.to_owned(),
            g.module.clone(),
            g.kind.to_owned(),
            g.name.clone(),
        )
    };

    let unlisted: Vec<&Gap> = found
        .iter()
        .filter(|g| !accepted.contains(&key(g)))
        .collect();
    assert!(
        unlisted.is_empty(),
        "{} new coverage gap(s) with no accepted row:\n{}\n\
         Implement the method, or add a row to {GAPS}.",
        unlisted.len(),
        unlisted
            .iter()
            .take(25)
            .map(|g| format!("  [{}] {}.{} ({})", g.tag, g.module, g.name, g.kind))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    let live: BTreeSet<_> = found.iter().map(key).collect();
    let closed: Vec<&(String, String, String, String)> =
        accepted.iter().filter(|k| !live.contains(*k)).collect();
    assert!(
        closed.is_empty(),
        "{} row(s) in {GAPS} no longer diverge -- re-bless with ZEO_BLESS=1:\n{}",
        closed.len(),
        closed
            .iter()
            .take(25)
            .map(|(t, m, k, n)| format!("  [{t}] {m}.{n} ({k})"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

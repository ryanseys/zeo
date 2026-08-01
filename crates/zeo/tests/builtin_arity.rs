//! Gates `Method#arity` against CRuby.
//!
//! zeo declares an arity per builtin in the `ruby_class!` DSL. CRuby computes
//! one from the method's signature. This test joins the two through
//! `conformance/builtin-arity.tsv` -- a checked-in dump of the pinned 4.0.6
//! oracle -- and fails when a declaration drifts away from it.
//!
//! It reads the DSL with the same `zeo_dsl` grammar the proc-macro expands, so
//! what it checks is definitionally what the runtime registers, and it needs no
//! ruby at test time. Regenerate the dump with
//! `cargo run -p xtask -- arity-oracle`.
//!
//! Accepted disagreements live in `conformance/builtin-arity-divergences.tsv`,
//! one tagged row each. The `baseline` tag is the migration backlog and is a
//! ratchet: `BASELINE_LIMIT` may only go down. `ZEO_BLESS=1` rewrites the file
//! from the current state, the same convention the golden corpus uses.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use zeo_dsl::scan::{self, Kind};

/// The number of `baseline` rows -- declarations that disagree with the oracle
/// and have not been migrated yet. Lower this as phases land; never raise it.
const BASELINE_LIMIT: usize = 131;

const DUMP: &str = "conformance/builtin-arity.tsv";
const DIVERGENCES: &str = "conformance/builtin-arity-divergences.tsv";

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// One `M` row: what CRuby reports for a method it actually owns.
struct OracleRow {
    arity: i64,
    /// `c` or `ruby`. A C function cannot express "1 required plus 1 optional",
    /// so its `parameters` carries no shape information and must not be read as
    /// if it did.
    implementation: String,
}

#[derive(Default)]
struct Oracle {
    /// `(class, kind, name)` -> row, for methods that class owns.
    own: BTreeMap<(String, String, String), OracleRow>,
    /// `(class, kind)` -> ancestor tokens (`i:Enumerable`, `s:IO`), in MRO
    /// order, so resolution mirrors CRuby's `instance_method` and zeo's own
    /// `method_meta::builtin_arity` ancestry walk.
    ancestry: BTreeMap<(String, String), Vec<String>>,
    /// Classes the oracle could not produce at all (a gem, or a platform the
    /// dump was not taken on). Distinct from zeo having invented the method.
    unavailable: BTreeSet<String>,
}

fn load_oracle(root: &Path) -> Oracle {
    let text = std::fs::read_to_string(root.join(DUMP)).unwrap_or_else(|e| {
        panic!("cannot read {DUMP}: {e}\nrun `cargo run -p xtask -- arity-oracle`")
    });
    let mut oracle = Oracle::default();
    for line in text.lines() {
        if line.starts_with("#!") {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        match f.first().copied() {
            Some("!") if f.len() >= 2 => {
                oracle.unavailable.insert(f[1].to_owned());
            }
            Some("A") if f.len() >= 3 => {
                oracle.ancestry.insert(
                    (f[1].to_owned(), f[2].to_owned()),
                    f[3..].iter().map(|s| (*s).to_owned()).collect(),
                );
            }
            Some("M") if f.len() >= 8 => {
                oracle.own.insert(
                    (f[1].to_owned(), f[2].to_owned(), f[3].to_owned()),
                    OracleRow {
                        arity: f[4].parse().unwrap_or(-1),
                        implementation: f[6].to_owned(),
                    },
                );
            }
            _ => {}
        }
    }
    oracle
}

impl Oracle {
    /// Walk the MRO for `name`, returning the first ancestor that owns it.
    fn resolve(&self, class: &str, kind: Kind, name: &str) -> Option<&OracleRow> {
        let chain = self.ancestry.get(&(class.to_owned(), kind.tag().to_owned()))?;
        for token in chain {
            let (tag, owner) = token.split_once(':')?;
            if let Some(row) = self
                .own
                .get(&(owner.to_owned(), tag.to_owned(), name.to_owned()))
            {
                return Some(row);
            }
        }
        None
    }
}

/// Why a declaration is allowed to disagree with the oracle.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Tag {
    /// zeo defines it; CRuby has no such method anywhere in the chain.
    ZeoOnly,
    /// The oracle could not produce the class (a gem, or another platform).
    OracleMissing,
    /// A real, intentional difference. Needs prose.
    Deliberate,
    /// The unmigrated backlog. Ratcheted by `BASELINE_LIMIT`.
    Baseline,
}

impl Tag {
    fn as_str(self) -> &'static str {
        match self {
            Tag::ZeoOnly => "zeo-only",
            Tag::OracleMissing => "oracle-missing",
            Tag::Deliberate => "deliberate",
            Tag::Baseline => "baseline",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "zeo-only" => Some(Tag::ZeoOnly),
            "oracle-missing" => Some(Tag::OracleMissing),
            "deliberate" => Some(Tag::Deliberate),
            "baseline" => Some(Tag::Baseline),
            _ => None,
        }
    }
}

/// One declaration that does not match the oracle.
struct Divergence {
    tag: Tag,
    class: String,
    kind: Kind,
    name: String,
    reason: String,
}

fn key(d: &Divergence) -> (String, String, String) {
    (
        d.class.clone(),
        d.kind.tag().to_owned(),
        d.name.clone(),
    )
}

fn load_divergences(root: &Path) -> BTreeMap<(String, String, String), Tag> {
    let Ok(text) = std::fs::read_to_string(root.join(DIVERGENCES)) else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for line in text.lines() {
        if line.starts_with("#!") || line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 4 {
            continue;
        }
        if let Some(tag) = Tag::parse(f[0]) {
            out.insert(
                (f[1].to_owned(), f[2].to_owned(), f[3].to_owned()),
                tag,
            );
        }
    }
    out
}

fn write_divergences(root: &Path, rows: &[Divergence]) {
    let mut out = String::new();
    out.push_str("#! Accepted divergences from conformance/builtin-arity.tsv.\n");
    out.push_str("#! Every row needs a reason. `baseline` is the unmigrated backlog and is\n");
    out.push_str("#! ratcheted by BASELINE_LIMIT in crates/zeo/tests/builtin_arity.rs -- that\n");
    out.push_str("#! number may only go down. Regenerate with `ZEO_BLESS=1 cargo nextest run\n");
    out.push_str("#! -p zeo --test builtin_arity`.\n");
    out.push_str("#! tag\tclass\tkind\tname\treason\n");
    for d in rows {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            d.tag.as_str(),
            d.class,
            d.kind.tag(),
            d.name,
            d.reason
        ));
    }
    let path = root.join(DIVERGENCES);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, out).expect("cannot write the divergences file");
}

#[test]
fn builtin_arity_matches_the_oracle() {
    let root = root();
    let oracle = load_oracle(&root);
    let abi = scan::scan_abi(&root);
    let decls = scan::scan_decls(&root);
    assert!(
        decls.len() > 2000,
        "the DSL scan found only {} declarations -- the walk is broken",
        decls.len()
    );

    let mut found = Vec::new();
    for decl in &decls {
        let Some(class) = abi.get(&decl.class_const) else {
            // No ABI row means no Ruby name to ask the oracle about. zeo's
            // build.rs consistency check is what makes this an error.
            continue;
        };
        let declared = decl.arity;

        let (tag, reason) = if oracle.unavailable.contains(&class.ruby_name) {
            (
                Tag::OracleMissing,
                format!(
                    "{} is absent from the pinned oracle{}",
                    class.ruby_name,
                    decl.cfg
                        .as_deref()
                        .map(|c| format!(" ({c})"))
                        .unwrap_or_default()
                ),
            )
        } else {
            match oracle.resolve(&class.ruby_name, decl.kind, &decl.name) {
                None => (
                    Tag::ZeoOnly,
                    format!("no {} in CRuby's chain", decl.name),
                ),
                Some(row) if row.arity == declared => continue,
                Some(row) => (
                    Tag::Baseline,
                    format!(
                        "declared {declared}, oracle {} ({}) at {}:{}",
                        row.arity, row.implementation, decl.file, decl.line
                    ),
                ),
            }
        };
        found.push(Divergence {
            tag,
            class: class.ruby_name.clone(),
            kind: decl.kind,
            name: decl.name.clone(),
            reason,
        });
    }
    found.sort_by_key(|d| (d.tag, key(d)));

    if std::env::var_os("ZEO_BLESS").is_some() {
        write_divergences(&root, &found);
        eprintln!(
            "blessed {DIVERGENCES}: {} rows ({} baseline)",
            found.len(),
            found.iter().filter(|d| d.tag == Tag::Baseline).count()
        );
        return;
    }

    let accepted = load_divergences(&root);
    let baseline = found.iter().filter(|d| d.tag == Tag::Baseline).count();

    // A declaration that newly disagrees, with no row accepting it.
    let unlisted: Vec<&Divergence> = found
        .iter()
        .filter(|d| !accepted.contains_key(&key(d)))
        .collect();
    assert!(
        unlisted.is_empty(),
        "{} builtin arity declaration(s) drifted with no accepted row:\n{}\n\
         Fix the declaration, or add a row to {DIVERGENCES} with a reason.",
        unlisted.len(),
        unlisted
            .iter()
            .take(25)
            .map(|d| format!("  {}#{} [{}] {}", d.class, d.name, d.kind.tag(), d.reason))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    // A row that no longer diverges must be deleted, so the file cannot rot.
    let live: BTreeSet<(String, String, String)> = found.iter().map(key).collect();
    let stale: Vec<&(String, String, String)> =
        accepted.keys().filter(|k| !live.contains(*k)).collect();
    assert!(
        stale.is_empty(),
        "{} row(s) in {DIVERGENCES} no longer diverge -- delete them:\n{}",
        stale.len(),
        stale
            .iter()
            .take(25)
            .map(|(c, k, n)| format!("  {c}#{n} [{k}]"))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    assert!(
        baseline <= BASELINE_LIMIT,
        "the arity backlog grew to {baseline}, above BASELINE_LIMIT ({BASELINE_LIMIT}). \
         This ratchet may only go down."
    );
}

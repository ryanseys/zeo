//! `cargo run -p xtask -- arity-annotate [--check]`: writes the `cfunc` markers
//! from the oracle, so nobody has to know which builtins CRuby implements as a
//! signature-less C function.
//!
//! A def's parameter list gives its arity by CRuby's equation. The one thing
//! the signature cannot say is that CRuby declared this particular method
//! `argc = -1` and checked its arguments by hand, which makes it report -1
//! whatever shape it really accepts. That fact lives in
//! `conformance/builtin-arity.tsv`, so it is read from there rather than
//! remembered.
//!
//! Edits splice bytes on the def's own line instead of re-emitting the file
//! through syn, which would reformat everything around it.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use zeo_dsl::scan;

/// What the oracle implies for one Ruby name.
struct Verdict {
    name: String,
    /// CRuby reports -1 but the signature says otherwise.
    wants_cfunc: bool,
    /// An `arity N` is written where the signature already lands correctly.
    redundant_override: bool,
}

struct Edit {
    line: usize,
    kind: EditKind,
}

enum EditKind {
    InsertCfunc,
    StripArity,
}

pub fn main(root: &Path, args: &[String]) -> ExitCode {
    let check = args.iter().any(|a| a == "--check");

    let oracle = load_oracle(root);
    let abi = scan::scan_abi(root);
    let decls = scan::scan_decls(root);

    // A marker is per-def, but the oracle answers per-name, so collect the
    // verdicts by source line and only act where they agree.
    let mut per_line: BTreeMap<(String, usize), Vec<Verdict>> = BTreeMap::new();
    for decl in &decls {
        let Some(class) = abi.get(&decl.class_const) else {
            continue;
        };
        let Some(&oracle_arity) = oracle.get(&(
            class.ruby_name.clone(),
            decl.kind.tag().to_owned(),
            decl.name.clone(),
        )) else {
            continue;
        };
        if decl.line == 0 {
            continue;
        }
        per_line
            .entry((decl.file.clone(), decl.line))
            .or_default()
            .push(Verdict {
                name: decl.name.clone(),
                wants_cfunc: oracle_arity == -1 && decl.arity != -1,
                redundant_override: decl.override_written && decl.derived == oracle_arity,
            });
    }

    let mut wanted: BTreeMap<String, Vec<Edit>> = BTreeMap::new();
    let mut split = Vec::new();
    for ((file, line), verdicts) in per_line {
        // An override the signature already agrees with is noise, whichever way
        // it was written; drop it before considering a marker.
        if verdicts.iter().all(|v| v.redundant_override) {
            wanted.entry(file.clone()).or_default().push(Edit {
                line,
                kind: EditKind::StripArity,
            });
            continue;
        }
        let needs = verdicts.iter().filter(|v| v.wants_cfunc).count();
        if needs == 0 {
            continue;
        }
        if needs != verdicts.len() {
            // The def's names genuinely report different numbers. One marker
            // cannot serve them all, and the honest fix is usually to split the
            // def rather than to write an override.
            split.push((file, line, verdicts));
            continue;
        }
        wanted.entry(file).or_default().push(Edit {
            line,
            kind: EditKind::InsertCfunc,
        });
    }

    let (mut files, mut marks, mut strips) = (0usize, 0usize, 0usize);
    for (file, lines) in &wanted {
        let path = root.join(file);
        let Ok(source) = std::fs::read_to_string(&path) else {
            eprintln!("cannot read {file}");
            return ExitCode::FAILURE;
        };
        let mut out: Vec<String> = source.split_inclusive('\n').map(str::to_owned).collect();
        let mut hits = 0usize;
        for edit in lines {
            let Some(text) = out.get(edit.line - 1) else {
                continue;
            };
            let rewritten = match edit.kind {
                EditKind::InsertCfunc => insert_cfunc(text),
                EditKind::StripArity => strip_arity(text),
            };
            if let Some(new) = rewritten {
                out[edit.line - 1] = new;
                hits += 1;
                match edit.kind {
                    EditKind::InsertCfunc => marks += 1,
                    EditKind::StripArity => strips += 1,
                }
            }
        }
        if hits == 0 {
            continue;
        }
        files += 1;
        if check {
            println!("{file}: {hits} edit(s)");
        } else if let Err(e) = std::fs::write(&path, out.concat()) {
            eprintln!("cannot write {file}: {e}");
            return ExitCode::FAILURE;
        }
    }

    println!(
        "{} {marks} `cfunc` marker(s) and dropped {strips} redundant override(s) across {files} file(s)",
        if check { "would write" } else { "wrote" }
    );
    for (file, line, verdicts) in &split {
        let names: Vec<&str> = verdicts.iter().map(|v| v.name.as_str()).collect();
        println!("  {file}:{line}: names report different arities ({names:?}) -- split the def");
    }
    ExitCode::SUCCESS
}

/// `(class, kind, name)` -> the arity CRuby reports, resolved through the MRO.
fn load_oracle(root: &Path) -> BTreeMap<(String, String, String), i64> {
    let text = std::fs::read_to_string(root.join("conformance/builtin-arity.tsv"))
        .unwrap_or_else(|e| panic!("cannot read the oracle dump: {e}"));

    let mut own: BTreeMap<(String, String, String), i64> = BTreeMap::new();
    let mut ancestry: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.first().copied() {
            Some("A") if f.len() >= 3 => {
                ancestry.insert(
                    (f[1].to_owned(), f[2].to_owned()),
                    f[3..].iter().map(|s| (*s).to_owned()).collect(),
                );
            }
            Some("M") if f.len() >= 5 => {
                own.insert(
                    (f[1].to_owned(), f[2].to_owned(), f[3].to_owned()),
                    f[4].parse().unwrap_or(-1),
                );
            }
            _ => {}
        }
    }

    // Flatten each class's chain so a caller can ask by (class, kind, name)
    // without walking it again.
    let mut out = own.clone();
    for ((class, kind), chain) in &ancestry {
        for token in chain {
            let Some((tag, owner)) = token.split_once(':') else {
                continue;
            };
            for ((o_class, o_kind, name), arity) in &own {
                if o_class == owner && o_kind == tag {
                    out.entry((class.clone(), kind.clone(), name.clone()))
                        .or_insert(*arity);
                }
            }
        }
    }
    out
}

/// Strip a per-name `arity N` override from a def line.
///
/// An override is worth keeping only where a def's `|`-joined names genuinely
/// report different numbers. Everywhere else it is either restating what the
/// parameter list already says or, worse, contradicting it.
fn strip_arity(line: &str) -> Option<String> {
    let at = line.find(" arity ")?;
    let rest = &line[at + " arity ".len()..];
    let digits = rest
        .find(|c: char| !c.is_ascii_digit() && c != '-')
        .unwrap_or(rest.len());
    if digits == 0 {
        return None;
    }
    Some(format!("{}{}", &line[..at], &rest[digits..]))
}

/// Place `cfunc` between a def's names and its parameter list. Returns `None`
/// if the marker is already there or the line has no parameter list.
fn insert_cfunc(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let def_at = line.find("def ")?;
    let mut i = def_at;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'(' {
            let head = line[..i].trim_end();
            if head.ends_with("cfunc") {
                return None;
            }
            return Some(format!("{head} cfunc {}", &line[i..]));
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::insert_cfunc;

    #[test]
    fn places_the_marker_before_the_parameter_list() {
        assert_eq!(
            insert_cfunc("    def \"upcase\"(recv) {\n").unwrap(),
            "    def \"upcase\" cfunc (recv) {\n"
        );
        assert_eq!(
            insert_cfunc("    def self.\"entries\" | \"to_a\"(recv) {\n").unwrap(),
            "    def self.\"entries\" | \"to_a\" cfunc (recv) {\n"
        );
    }

    /// A method NAMED `"()"` must not be mistaken for the parameter list.
    #[test]
    fn skips_parentheses_inside_a_name() {
        assert_eq!(
            insert_cfunc("    def \"call\" | \"()\" (recv, *args, &blk) {\n").unwrap(),
            "    def \"call\" | \"()\" cfunc (recv, *args, &blk) {\n"
        );
    }

    #[test]
    fn is_idempotent() {
        assert!(insert_cfunc("    def \"upcase\" cfunc (recv) {\n").is_none());
    }
}

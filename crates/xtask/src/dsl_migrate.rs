//! `cargo run -p xtask -- dsl-migrate [--check]`: rewrites the legacy
//! `(recv, args, block)` triple in every `ruby_class!` def header to its
//! explicit spelling, `(recv, *args, &block)`.
//!
//! The two mean the same thing -- min 0, unbounded, no guard, arity -1 -- so
//! this is a provable no-op. It exists so the grammar can then drop the
//! transitional branch that reads three bare idents as a rest-plus-block, which
//! is what frees `(recv, a, b)` to mean two required parameters.
//!
//! Edits are line-local and anchored on the `def` keyword rather than parsed,
//! because rewriting through `syn` would re-emit whole files and destroy the
//! comments the runtime modules carry. `--check` reports what would change
//! without writing.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub fn main(root: &Path, args: &[String]) -> ExitCode {
    let check = args.iter().any(|a| a == "--check");

    // The whole runtime, not just builtins/ and ext/: `Ractor` is declared in
    // `ractor.rs` at the top level. `lib.rs` hosts a same-named `macro_rules!`
    // whose defs take typed Rust parameters, and those fall out naturally --
    // they are not three bare idents.
    let mut files = Vec::new();
    collect(&root.join("crates/zeo-rt/src"), &mut files);
    files.sort();

    let (mut changed_files, mut changed_defs) = (0usize, 0usize);
    for path in &files {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let mut out = String::with_capacity(source.len());
        let mut hits = 0usize;
        for line in source.split_inclusive('\n') {
            match rewrite_header(line) {
                Some(new) => {
                    hits += 1;
                    out.push_str(&new);
                }
                None => out.push_str(line),
            }
        }
        if hits == 0 {
            continue;
        }
        changed_files += 1;
        changed_defs += hits;
        if check {
            println!("{}: {hits} def(s)", rel(root, path));
        } else if let Err(e) = std::fs::write(path, &out) {
            eprintln!("cannot write {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    }

    println!(
        "{} {changed_defs} def header(s) across {changed_files} file(s)",
        if check { "would rewrite" } else { "rewrote" }
    );
    ExitCode::SUCCESS
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.flatten().map(|e| e.path()) {
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Rewrite the first `(a, b, c)` on a `def` line to `(a, *b, &c)`.
///
/// Returns `None` when the line is not a def header, or its parameter list is
/// already in the new form -- so the pass is idempotent.
fn rewrite_header(line: &str) -> Option<String> {
    let def_at = find_def_keyword(line)?;

    // A def has exactly one parameter list, and it is the first parenthesised
    // group after the keyword. Scanning forward from `def` keeps a `(` inside a
    // one-line body from being mistaken for it, and string literals are skipped
    // because a Ruby method may be NAMED `"()"` (Method#call's alias).
    let bytes = line.as_bytes();
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
            let close = matching_paren(bytes, i)?;
            let inner = &line[i + 1..close];
            let idents = three_bare_idents(inner)?;
            return Some(format!(
                "{}({}, *{}, &{}){}",
                &line[..i],
                idents.0,
                idents.1,
                idents.2,
                &line[close + 1..]
            ));
        }
        i += 1;
    }
    None
}

/// The byte offset just past a `def` keyword that opens a method definition,
/// allowing the `private`/`protected`/`module_function` prefixes.
fn find_def_keyword(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    let rest = ["private ", "protected ", "module_function "]
        .iter()
        .find_map(|p| trimmed.strip_prefix(p))
        .unwrap_or(trimmed);
    let offset = line.len() - rest.len();
    let after = rest.strip_prefix("def ")?;
    debug_assert!(offset >= indent);
    Some(line.len() - after.len())
}

fn matching_paren(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// `"recv, args, _block"` -> the three names, or `None` if the list is anything
/// else (already migrated, a different arity, a sigil).
fn three_bare_idents(inner: &str) -> Option<(String, String, String)> {
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() != 3 {
        return None;
    }
    for p in &parts {
        if p.is_empty()
            || !p
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
            || p.starts_with(|c: char| c.is_ascii_digit())
        {
            return None;
        }
    }
    Some((
        parts[0].to_owned(),
        parts[1].to_owned(),
        parts[2].to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::rewrite_header;

    #[test]
    fn rewrites_a_plain_header() {
        assert_eq!(
            rewrite_header("    def \"length\"(recv, args, _block) {\n").unwrap(),
            "    def \"length\"(recv, *args, &_block) {\n"
        );
    }

    #[test]
    fn keeps_everything_else_on_the_line() {
        assert_eq!(
            rewrite_header("    def self.\"utc\" arity 0 | \"gm\"(r, a, b) { body() }\n").unwrap(),
            "    def self.\"utc\" arity 0 | \"gm\"(r, *a, &b) { body() }\n"
        );
        assert_eq!(
            rewrite_header("    module_function def \"sqrt\"(recv, args, blk) {\n").unwrap(),
            "    module_function def \"sqrt\"(recv, *args, &blk) {\n"
        );
    }

    /// `Method#call` is aliased to `"()"`, whose parentheses are part of the
    /// NAME. The scan must not mistake them for the parameter list.
    #[test]
    fn skips_parentheses_inside_a_method_name() {
        assert_eq!(
            rewrite_header("    def \"call\" | \"()\" | \"[]\" (recv, args, blk) {\n").unwrap(),
            "    def \"call\" | \"()\" | \"[]\" (recv, *args, &blk) {\n"
        );
    }

    #[test]
    fn leaves_non_headers_and_migrated_headers_alone() {
        assert!(rewrite_header("        foo(a, b, c);\n").is_none());
        assert!(rewrite_header("    def \"length\"(recv) {\n").is_none());
        assert!(rewrite_header("    def \"x\"(recv, *args, &blk) {\n").is_none());
        assert!(rewrite_header("    def \"x\"(recv, a = nil, b?) {\n").is_none());
    }
}

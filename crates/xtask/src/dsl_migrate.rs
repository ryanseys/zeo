//! `cargo run -p xtask -- dsl-migrate --phase <p> [--check]`: mechanical passes
//! that move a def's shape out of its body and into its parameter list.
//!
//! - `params` rewrote the legacy `(recv, args, block)` triple to its explicit
//!   spelling, `(recv, *args, &block)`. The two mean the same thing -- min 0,
//!   unbounded, no guard, arity -1 -- so it was a provable no-op, and it freed
//!   `(recv, a, b)` to mean two required arguments.
//! - `fixed` converts a def whose body opens with `arity!(args, N)` into the
//!   signature that says so, deleting the guard. The macro then emits an
//!   equivalent check and reports the matching arity.
//!
//! Edits are line-local and anchored on the `def` keyword rather than parsed,
//! because rewriting through `syn` would re-emit whole files and destroy the
//! comments the runtime modules carry. `--check` reports what would change
//! without writing.
//!
//! Nothing here has to be careful about being wrong in the direction of doing
//! too much: dropping `*args` from a signature makes every surviving use of it
//! a compile error, so rustc, not this tool, is what proves a conversion.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub fn main(root: &Path, args: &[String]) -> ExitCode {
    let check = args.iter().any(|a| a == "--check");
    let phase = args
        .iter()
        .position(|a| a == "--phase")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        .unwrap_or("params");
    if !matches!(phase, "params" | "fixed") {
        eprintln!("unknown --phase {phase}; expected `params` or `fixed`");
        return ExitCode::FAILURE;
    }

    // The whole runtime, not just builtins/ and ext/: `Ractor` is declared in
    // `ractor.rs` at the top level. `lib.rs` hosts a same-named `macro_rules!`
    // whose defs take typed Rust parameters, and those fall out naturally --
    // they are not three bare idents.
    let mut files = Vec::new();
    collect(&root.join("crates/zeo-rt/src"), &mut files);
    files.sort();

    let (mut changed_files, mut changed_defs, mut skipped) = (0usize, 0usize, 0usize);
    for path in &files {
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let (out, hits, skips) = match phase {
            "params" => {
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
                (out, hits, 0)
            }
            _ => convert_fixed(&source),
        };
        skipped += skips;
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
        "{} {changed_defs} def(s) across {changed_files} file(s)",
        if check { "would convert" } else { "converted" }
    );
    if skipped > 0 {
        println!("  left {skipped} for hand work (the body still uses `args`)");
    }
    ExitCode::SUCCESS
}

/// Convert defs whose body opens with a fixed `arity!(args, N)` guard into the
/// signature that declares it.
///
/// Returns the new source, how many defs changed, and how many were left alone
/// because the body still needs the raw slice.
fn convert_fixed(source: &str) -> (String, usize, usize) {
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut out = String::with_capacity(source.len());
    let (mut hits, mut skipped) = (0usize, 0usize);

    let mut i = 0usize;
    while i < lines.len() {
        let Some(header) = parse_header(lines[i]) else {
            out.push_str(lines[i]);
            i += 1;
            continue;
        };
        let Some(end) = body_end(&lines, i) else {
            out.push_str(lines[i]);
            i += 1;
            continue;
        };
        // The guard need not be the first statement -- a comment or a frozen
        // check may sit above it -- and hoisting it into the generated preamble
        // is more correct than where it sits: CRuby checks arity in the VM
        // before the body runs, so an ArgumentError beats a FrozenError.
        //
        // But it MUST be at the body's top level. A guard inside a branch is
        // that branch's precondition, not the method's arity, and hoisting one
        // would silently narrow a variadic method (`Class#new` guards a
        // sub-case with `arity!(args, 0)` and accepts anything otherwise).
        let Some((guard_at, (lo, hi))) = top_level_guard(&lines, i, end, &header.args) else {
            out.push_str(lines[i]);
            i += 1;
            continue;
        };
        let Some(end) = body_end(&lines, i) else {
            out.push_str(lines[i]);
            i += 1;
            continue;
        };
        let names = param_names(&header.before, hi);

        // Everything between the header and the guard, then everything after
        // it: the guard itself is what this pass deletes.
        let mut body: Vec<&str> = Vec::new();
        body.extend_from_slice(&lines[i + 1..guard_at]);
        body.extend_from_slice(&lines[guard_at + 1..end]);

        // Every `args` the body still needs must be reachable through the
        // named parameters; anything else wants the raw slice for something
        // the signature cannot express.
        let rewritten: Option<Vec<String>> = body
            .iter()
            .map(|l| rewrite_body_line(l, &header.args, &names, lo))
            .collect();
        let Some(rewritten) = rewritten else {
            skipped += 1;
            out.push_str(lines[i]);
            i += 1;
            continue;
        };

        let keeps_block = body.iter().any(|l| mentions_ident(l, &header.block));
        let mut params = vec![header.recv.clone()];
        for (idx, name) in names.iter().enumerate() {
            // Past `lo` the argument may be absent, and `Option` is how the
            // body tells that from an explicit nil.
            params.push(if idx < lo {
                name.clone()
            } else {
                format!("{name}?")
            });
        }
        if keeps_block {
            params.push(format!("&{}", header.block));
        }
        out.push_str(&format!(
            "{}({}){}",
            header.before,
            params.join(", "),
            header.after
        ));
        for line in rewritten {
            out.push_str(&line);
        }
        hits += 1;
        // Past the header, the guard, and the body just re-emitted.
        i = end;
    }
    (out, hits, skipped)
}

/// Names for `n` positional parameters.
///
/// Ruby names a binary operator's operand `other`, and CRuby's own C sources do
/// the same, so an operator def reads correctly with no further thought. Nothing
/// better is derivable for the rest.
fn param_names(header: &str, n: usize) -> Vec<String> {
    const OPERATORS: [&str; 20] = [
        "==", "!=", "<=>", "<", ">", "<=", ">=", "===", "+", "-", "*", "/", "%", "**", "&", "|",
        "^", "<<", ">>", "=~",
    ];
    if n == 1 {
        let is_operator = OPERATORS
            .iter()
            .any(|op| header.contains(&format!("\"{op}\"")));
        return vec![if is_operator { "other" } else { "arg" }.to_owned()];
    }
    (0..n).map(|i| format!("arg{}", i + 1)).collect()
}

/// Rewrite one body line so it reads the named parameters instead of the slice.
///
/// Returns `None` when the line uses `args` in a way the parameters cannot
/// serve -- `args.len()`, passing the slice on, an index this def does not
/// name -- which is the signal to leave the whole def alone.
fn rewrite_body_line(line: &str, args: &str, names: &[String], required: usize) -> Option<String> {
    if !mentions_ident(line, args) {
        return Some(line.to_owned());
    }
    // A nested binding of the same name -- a closure parameter, a `let` that
    // reshapes the slice -- is a different variable that happens to share the
    // spelling. Rewriting through it would corrupt the inner scope.
    if line.contains(&format!("|{args}"))
        || line.contains(&format!("let {args} "))
        || line.contains(&format!("let ({args}, "))
    {
        return None;
    }
    let mut out = line.to_owned();

    // An optional parameter binds `Option<&RubyValue>`, which is exactly what
    // the slice lookups it replaces already produced.
    for (i, name) in names.iter().enumerate().skip(required) {
        out = out.replace(&format!("{args}.get({i})"), name);
        if i == 0 {
            out = out.replace(&format!("{args}.first()"), name);
        }
    }
    // The coercion guards take either a slice-plus-index or a value, so a
    // named parameter drops straight in.
    for (i, name) in names.iter().enumerate().take(required) {
        for macro_name in ["arg_int", "arg_str"] {
            out = out.replace(
                &format!("{macro_name}!({args}, {i})"),
                &format!("{macro_name}!({name})"),
            );
        }
    }
    // A def that accepts nothing has a provably empty slice, so the places that
    // pass it on -- `block_or_enum!` rebuilding an enumerator, mostly -- can
    // take the empty slice directly.
    if names.is_empty() {
        out = replace_ident(&out, args, "&[]");
    }
    // A single trailing optional makes `args.is_empty()` a question about that
    // one parameter.
    if names.len() == required + 1 && required == 0 {
        out = out.replace(
            &format!("{args}.is_empty()"),
            &format!("{}.is_none()", names[0]),
        );
    }
    for (i, name) in names.iter().enumerate().take(required) {
        // `&args[0]` is already a borrow, so the name substitutes directly --
        // but NOT before a method call, where the `&` binds to the call's
        // result (`&args[0].to_s()` is `&(args[0].to_s())`) and dropping it
        // changes the type.
        let borrowed = format!("&{args}[{i}]");
        let mut from = 0usize;
        while let Some(rel) = out[from..].find(&borrowed) {
            let at = from + rel;
            let end = at + borrowed.len();
            if out[end..].starts_with('.') {
                from = end;
                continue;
            }
            out.replace_range(at..end, name);
            from = at + name.len();
        }
        // Anywhere else the index is a value, so it needs the deref.
        out = out.replace(&format!("{args}[{i}]"), &format!("(*{name})"));
    }
    (!mentions_ident(&out, args)).then_some(out)
}

/// A def header split around its parameter list.
struct Header {
    before: String,
    after: String,
    recv: String,
    args: String,
    block: String,
}

/// Match a header already in the `(recv, *args, &block)` shape.
fn parse_header(line: &str) -> Option<Header> {
    let def_at = find_def_keyword(line)?;
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
            let parts: Vec<&str> = line[i + 1..close].split(',').map(str::trim).collect();
            if parts.len() != 3 {
                return None;
            }
            let recv = parts[0];
            let args = parts[1].strip_prefix('*')?;
            let block = parts[2].strip_prefix('&')?;
            return Some(Header {
                before: line[..i].to_owned(),
                after: line[close + 1..].to_owned(),
                recv: recv.to_owned(),
                args: args.to_owned(),
                block: block.to_owned(),
            });
        }
        i += 1;
    }
    None
}

/// `arity!(args, 0)` -> `(0, 0)`, `arity!(args, 1..=2)` -> `(1, 2)`, for the
/// def's own `args` name only.
fn guard(line: &str, args: &str) -> Option<(usize, usize)> {
    let t = line.trim();
    let t = t.strip_suffix(';')?;
    let t = t
        .strip_prefix("arity!")
        .or_else(|| t.strip_prefix("crate::builtins::arity!"))?;
    let inner = t.strip_prefix('(')?.strip_suffix(')')?;
    let (lhs, rhs) = inner.split_once(',')?;
    if lhs.trim() != args {
        return None;
    }
    let rhs = rhs.trim();
    match rhs.split_once("..=") {
        Some((lo, hi)) => Some((lo.trim().parse().ok()?, hi.trim().parse().ok()?)),
        None => {
            let n = rhs.parse().ok()?;
            Some((n, n))
        }
    }
}

/// Find the def's own arity guard: the first one sitting directly in the body,
/// never one nested inside a branch.
fn top_level_guard(
    lines: &[&str],
    header: usize,
    end: usize,
    args: &str,
) -> Option<(usize, (usize, usize))> {
    // The header opens the body, so every line of it starts at depth 1.
    let mut depth = 1i32;
    for n in header + 1..end {
        let line = lines[n];
        if depth == 1 {
            if let Some(g) = guard(line, args) {
                return Some((n, g));
            }
        }
        for b in line.bytes() {
            match b {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
        }
    }
    None
}

/// The line index just past a def's closing brace, by brace depth from its
/// header. Returns `None` for a one-line def, which has nothing to scan.
fn body_end(lines: &[&str], header: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (offset, line) in lines[header..].iter().enumerate() {
        for b in line.bytes() {
            match b {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
        }
        if depth == 0 && offset > 0 {
            return Some(header + offset);
        }
        if depth == 0 && offset == 0 {
            return None;
        }
    }
    None
}

/// Whether `line` uses `name` as a whole identifier.
fn mentions_ident(line: &str, name: &str) -> bool {
    let bytes = line.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = line[from..].find(name) {
        let at = from + rel;
        let before_ok = at == 0 || !is_ident_byte(bytes[at - 1]);
        let end = at + name.len();
        let after_ok = end >= bytes.len() || !is_ident_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = at + name.len();
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Replace whole-identifier occurrences of `name`, leaving `args_len` and the
/// like alone.
fn replace_ident(line: &str, name: &str, with: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < line.len() {
        if line[i..].starts_with(name) {
            let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let end = i + name.len();
            let after_ok = end >= bytes.len() || !is_ident_byte(bytes[end]);
            if before_ok && after_ok {
                out.push_str(with);
                i = end;
                continue;
            }
        }
        let ch = line[i..].chars().next().expect("in bounds");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
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

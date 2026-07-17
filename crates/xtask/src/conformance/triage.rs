//! Failure triage: normalize a failing test's compiler/runtime stderr into a
//! stable message, then map it onto the implementation plan's gap clusters
//! (a)-(m) via a curated substring table. Messages that match nothing get an
//! auto bucket keyed by the normalized message's hash -- those are the "new
//! gap discovered" signal, surfaced in TRIAGE.md so the table can grow.

use super::util::fnv1a64;

/// Curated mapping: first matching substring wins. Substrings are matched
/// against the *normalized* message (backtick spans and digit runs
/// collapsed), so keep them free of identifiers and numbers.
const CLUSTERS: &[(&str, &str, &str)] = &[
    // (substring, cluster, bucket)
    // -- compile-time rejections/panics ---------------------------------
    ("unexpected top-level-only node in expression position", "?", "toplevel-node-in-expr"),
    ("unsupported implicit-self call", "?", "implicit-self-call"),
    ("receiver's class isn't statically known", "?", "dynamic-receiver-call"),
    ("unknown class", "?", "unknown-class"),
    ("must already be defined earlier", "?", "alias-inherited"),
    ("keyword arguments isn't supported", "b", "kwargs-call-shape"),
    ("forwarding isn't supported", "a", "arg-forwarding"),
    ("double-splat", "a", "double-splat"),
    ("splat", "a", "splat"),
    ("only plain required parameters", "a", "param-shapes"),
    ("keyword parameter", "a", "kw-params"),
    ("block parameter", "a", "block-params"),
    ("multi-assignment", "c", "multi-assign"),
    ("compound assignment", "c", "compound-assign"),
    ("no exception channel", "g", "no-exception-channel"),
    ("wrong number of arguments", "g", "arity-panic"),
    ("class method", "d", "class-level-state"),
    ("class << ", "d", "singleton-class"),
    ("reopening a built-in", "e", "reopen-builtin"),
    ("subclassing a built-in", "e", "subclass-builtin"),
    ("operator method", "e", "builtin-operator"),
    ("`eval`", "f", "dynamic-eval"),
    ("require", "f", "dynamic-require"),
    ("autoload", "f", "autoload"),
    ("beginless", "h", "range-shapes"),
    ("endless", "h", "range-shapes"),
    ("would return an Enumerator", "h", "enumerator-forms"),
    ("named-capture", "j", "regex-sugar"),
    ("pattern", "k", "pattern-shapes"),
    ("`for`", "l", "for-eachable"),
    ("Struct.new", "m", "struct-new"),
    ("super", "g", "super-arity"),
    ("string interpolation", "a", "interpolation-shapes"),
    // -- runtime failures (FAIL_OUTPUT with a recognizable message) -----
    ("uninitialized constant", "P", "missing-core-const"),
    ("undefined method", "P", "missing-builtin-method"),
    // -- catch-alls (note: some messages end "(spike scope, ...") -------
    ("(spike scope", "?", "spike-misc"),
];

pub struct Triage {
    pub cluster: String,
    pub bucket: String,
}

/// Extract and normalize the salient message from a failing stage's stderr.
///
/// Panic output looks like:
/// ```text
/// thread 'main' panicked at crates/spinelc/src/codegen/call.rs:1299:21:
/// dynamic dispatch of `foo` with keyword arguments isn't supported yet ...
/// note: run with `RUST_BACKTRACE=1` ...
/// ```
/// Clean rejections look like `spinelc: <message>`.
pub fn classify(stderr: &str) -> Triage {
    let message = extract_message(stderr);
    let normalized = normalize(&message);
    for (needle, cluster, bucket) in CLUSTERS {
        if normalized.contains(needle) {
            return Triage {
                cluster: (*cluster).to_owned(),
                bucket: (*bucket).to_owned(),
            };
        }
    }
    let bucket = format!("auto-{:08x}", fnv1a64(normalized.as_bytes()) as u32);
    Triage {
        cluster: "?".to_owned(),
        bucket,
    }
}

fn extract_message(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().collect();
    // Prefer the message body after the last `panicked at` header.
    if let Some(pos) = lines.iter().rposition(|l| l.contains("panicked at")) {
        let body: Vec<&str> = lines[pos + 1..]
            .iter()
            .copied()
            .take_while(|l| !l.starts_with("note:"))
            .collect();
        if !body.is_empty() {
            return body.join(" ");
        }
    }
    // Then a clean `spinelc: <message>` rejection.
    if let Some(line) = lines.iter().rev().find(|l| l.starts_with("spinelc: ")) {
        return line["spinelc: ".len()..].to_owned();
    }
    // Otherwise the last non-empty line.
    lines
        .iter()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_owned())
        .unwrap_or_default()
}

/// Collapse identifiers (`` `foo` `` -> `` `_` ``) and digit runs (-> `N`) so
/// per-test variation folds into one bucket.
fn normalize(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut chars = message.chars().peekable();
    let mut in_backticks = false;
    while let Some(c) = chars.next() {
        match c {
            '`' if !in_backticks => {
                in_backticks = true;
                out.push_str("`_");
            }
            '`' if in_backticks => {
                in_backticks = false;
                out.push('`');
            }
            _ if in_backticks => {}
            c if c.is_ascii_digit() => {
                out.push('N');
                while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
                    chars.next();
                }
            }
            c => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_panic() {
        let stderr = "thread 'main' panicked at crates/spinelc/src/codegen/call.rs:1305:17:\n\
                      `super(**h)` (double-splat into super) isn't supported yet (spike scope)\n\
                      note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace";
        let t = classify(stderr);
        assert_eq!(t.cluster, "a");
        assert_eq!(t.bucket, "double-splat");
    }

    #[test]
    fn classifies_clean_rejection() {
        let t = classify("spinelc: only plain required parameters are supported in a method definition (spike scope)");
        assert_eq!(t.cluster, "a");
        assert_eq!(t.bucket, "param-shapes");
    }

    #[test]
    fn auto_bucket_is_stable() {
        let a = classify("spinelc: something entirely novel happened with `x` at 42");
        let b = classify("spinelc: something entirely novel happened with `y` at 7");
        assert_eq!(a.bucket, b.bucket);
        assert!(a.bucket.starts_with("auto-"));
    }
}

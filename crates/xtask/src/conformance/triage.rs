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
/// thread 'main' panicked at crates/zeo/src/codegen/call.rs:1299:21:
/// dynamic dispatch of `foo` with keyword arguments isn't supported yet ...
/// note: run with `RUST_BACKTRACE=1` ...
/// ```
/// Clean rejections look like `zeo: <message>`.
pub fn classify(stderr: &str) -> Triage {
    let message = extract_message(stderr);
    // High-value patterns get a bucket keyed on the SPECIFIC missing name, so
    // the triage ranks individual methods/constants (the actionable worklist)
    // instead of collapsing every "undefined method" into one 100+ pile.
    if let Some((cluster, bucket)) = specific_bucket(&message) {
        return Triage { cluster: cluster.to_owned(), bucket };
    }
    let normalized = normalize(&message);
    for (needle, cluster, bucket) in CLUSTERS {
        if normalized.contains(needle) {
            return Triage {
                cluster: (*cluster).to_owned(),
                bucket: (*bucket).to_owned(),
            };
        }
    }
    Triage {
        cluster: "?".to_owned(),
        bucket: auto_bucket(&normalized),
    }
}

/// A self-describing bucket name for a message no curated cluster matched: a
/// kebab slug of the (normalized) message plus a short stable hash. The slug
/// makes the bucket readable at a glance in the scoreboard/triage ("what is
/// this gap") -- e.g. `auto-cant-convert-string-into-complex-a1b2` -- while the
/// hash keeps two messages that share a leading phrase in distinct buckets and
/// preserves the stability the auto scheme has always guaranteed (same
/// normalized message -> same bucket). Single-character tokens (apostrophe
/// fragments like the `t` in "can't", collapsed-identifier/`N` noise) are
/// dropped so the slug reads cleanly.
fn auto_bucket(normalized: &str) -> String {
    let hash = fnv1a64(normalized.as_bytes()) as u32;
    // Strip the boilerplate lead-in shared by every runtime failure / internal
    // abort so the slug starts at the informative part; the hash still folds
    // over the FULL message, so stability is unchanged.
    let core = normalized
        .trim_start_matches("uncaught exception: ")
        .trim_start_matches("internal error: ");
    let slug = core
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .filter(|w| w.len() > 1)
        .take(8)
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        format!("auto-{hash:08x}")
    } else {
        format!("auto-{slug}-{:04x}", hash & 0xffff)
    }
}

/// Map the highest-value runtime failures onto a bucket keyed by the SPECIFIC
/// missing method or constant (`missing-method:transfer`,
/// `missing-const:SizedQueue`), so the triage report ranks individual gaps --
/// the direct worklist for closing them -- rather than one giant pile.
fn specific_bucket(message: &str) -> Option<(&'static str, String)> {
    // `undefined method 'X' for <receiver>` -- take the quoted name.
    if let Some(rest) = message.split_once("undefined method '") {
        if let Some(name) = rest.1.split('\'').next() {
            if !name.is_empty() {
                return Some(("P", format!("missing-method:{name}")));
            }
        }
    }
    // `uninitialized constant Z` (possibly a `A::B` path) -- take the token.
    if let Some(rest) = message.split_once("uninitialized constant ") {
        let name = rest.1.split_whitespace().next().unwrap_or("").trim();
        if !name.is_empty() {
            return Some(("P", format!("missing-const:{name}")));
        }
    }
    None
}

/// The salient one-line failure message from a stage's stderr -- the Rust
/// PANIC body (not the useless `note: run with RUST_BACKTRACE=1` trailer), a
/// clean `zeo: <msg>` rejection, or the last non-empty line. Public so the
/// scoreboard shows the SAME actionable text triage clusters on, instead of
/// whatever line happened to be last.
pub fn extract_message(stderr: &str) -> String {
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
    // Then a clean `zeo: <message>` rejection.
    if let Some(line) = lines.iter().rev().find(|l| l.starts_with("zeo: ")) {
        return line["zeo: ".len()..].to_owned();
    }
    // Otherwise the last meaningful line -- skipping the `note: run with
    // RUST_BACKTRACE=1` trailer, which is noise on its own (e.g. an allocation
    // abort, `memory allocation of N bytes failed`, prints the real reason on
    // the line just above it with no `panicked at` header).
    lines
        .iter()
        .rev()
        .find(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with("note: run with `RUST_BACKTRACE")
        })
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
        let stderr = "thread 'main' panicked at crates/zeo/src/codegen/call.rs:1305:17:\n\
                      `super(**h)` (double-splat into super) isn't supported yet (spike scope)\n\
                      note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace";
        let t = classify(stderr);
        assert_eq!(t.cluster, "a");
        assert_eq!(t.bucket, "double-splat");
    }

    #[test]
    fn classifies_clean_rejection() {
        let t = classify("zeo: only plain required parameters are supported in a method definition (spike scope)");
        assert_eq!(t.cluster, "a");
        assert_eq!(t.bucket, "param-shapes");
    }

    #[test]
    fn extracts_message_from_an_abort_without_a_panic_header() {
        // A `memory allocation of N bytes failed` abort (or any abort) has no
        // `panicked at` line -- the real reason is the line above the
        // RUST_BACKTRACE note, which must not be reported on its own.
        let stderr = "memory allocation of 1152921504606846976 bytes failed\n\
                      note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace";
        assert_eq!(
            extract_message(stderr),
            "memory allocation of 1152921504606846976 bytes failed"
        );
    }

    #[test]
    fn auto_bucket_is_stable() {
        // The same NORMALIZED message (identifiers and digits collapse) yields
        // the same bucket regardless of the specific names/numbers.
        let a = classify("zeo: something entirely novel happened with `x` at 42");
        let b = classify("zeo: something entirely novel happened with `y` at 7");
        assert_eq!(a.bucket, b.bucket);
        assert!(a.bucket.starts_with("auto-"));
    }

    #[test]
    fn auto_bucket_is_human_readable() {
        // The bucket name spells out the gap instead of an opaque hash, so the
        // scoreboard/triage lists are debuggable at a glance.
        let t = classify("can't convert String into Complex");
        assert!(
            t.bucket.starts_with("auto-can-convert-string-into-complex-"),
            "bucket was {}",
            t.bucket
        );
        // The apostrophe fragment `t` (from "can't") is dropped, not left as a
        // bare `-t-` token.
        assert!(!t.bucket.contains("-t-"), "bucket was {}", t.bucket);
    }

    #[test]
    fn distinct_messages_sharing_a_prefix_stay_in_distinct_buckets() {
        let a = classify("can't convert Hash into an exact number");
        let b = classify("can't convert Rational into an exact number");
        // Same readable slug prefix, but the hash suffix keeps them apart.
        assert_ne!(a.bucket, b.bucket);
    }
}

//! Making two answers comparable, and saying how they differ.

use std::path::Path;

use crate::case::{Answer, Exit};
use crate::normalize::{normalize_addresses, normalize_thread_ids};

/// Strip a `\r` before every `\n` so answers compare byte-exactly across OSes.
fn normalize_crlf(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn replace_bytes(haystack: &[u8], needle: &[u8], repl: &[u8]) -> Vec<u8> {
    if needle.is_empty() {
        return haystack.to_vec();
    }
    let mut out = Vec::with_capacity(haystack.len());
    let mut i = 0;
    while i < haystack.len() {
        if haystack[i..].starts_with(needle) {
            out.extend_from_slice(repl);
            i += needle.len();
        } else {
            out.push(haystack[i]);
            i += 1;
        }
    }
    out
}

/// Both engines embed the absolute source path in `__FILE__` and
/// backtraces; the recorded form is `run_cwd`-relative so a trailer is
/// portable. The program is given the ABSOLUTE path and this rewrites it
/// back: handing it the relative path looks equivalent and is not, because
/// what a program derives from `__FILE__` changes with it.
fn normalize_source_path(bytes: Vec<u8>, source: &Path, run_cwd: &Path) -> Vec<u8> {
    let abs = source.to_string_lossy();
    let rel = source
        .strip_prefix(run_cwd)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .or_else(|| source.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| abs.clone().into_owned());
    replace_bytes(&bytes, abs.as_bytes(), rel.as_bytes())
}

/// A fixture a program requires is named by its absolute path in a
/// backtrace (`require_relative` resolves to one); the recorded form is
/// relative to the run directory like the program's own.
fn normalize_run_cwd(bytes: Vec<u8>, run_cwd: &Path) -> Vec<u8> {
    let prefix = format!("{}/", run_cwd.display());
    replace_bytes(&bytes, prefix.as_bytes(), b"")
}

/// Every scrub, in order: CRLF, the source path, the run directory, object
/// addresses, thread ids.
pub fn norm(bytes: &[u8], source: &Path, run_cwd: &Path) -> Vec<u8> {
    normalize_thread_ids(normalize_addresses(normalize_run_cwd(
        normalize_source_path(normalize_crlf(bytes), source, run_cwd),
        run_cwd,
    )))
}

pub fn norm_answer(a: &Answer, source: &Path, run_cwd: &Path) -> Answer {
    Answer {
        stdout: norm(&a.stdout, source, run_cwd),
        stderr: norm(&a.stderr, source, run_cwd),
        exit: a.exit,
    }
}

/// The census line `ZEO_RT_GCCHECK=1` writes at exit, split out of stderr
/// so it does not break every other comparison.
pub fn split_gccheck(err: &[u8]) -> (Vec<u8>, String) {
    let text = String::from_utf8_lossy(err).into_owned();
    let mut census = String::new();
    let mut kept = String::new();
    for line in text.split_inclusive('\n') {
        if line.starts_with("cycle leak: ") {
            census = line.trim_end().to_string();
        } else {
            kept.push_str(line);
        }
    }
    (kept.into_bytes(), census)
}

const SHOW_LIMIT: usize = 8 << 10;

fn show(b: &[u8]) -> String {
    let clipped = &b[..b.len().min(SHOW_LIMIT)];
    let mut s = String::from_utf8_lossy(clipped).into_owned();
    if b.len() > SHOW_LIMIT {
        s.push_str(&format!("\n[... {} more bytes]", b.len() - SHOW_LIMIT));
    }
    s
}

fn exit_text(e: Exit) -> String {
    e.to_string()
}

/// Two normalized answers, side by side.
pub fn mismatch(rb: &Path, what: &str, expected: &Answer, actual: &Answer) -> String {
    let mut out = format!("{}: {what}\n", rb.display());
    if expected.exit != actual.exit {
        out.push_str(&format!(
            "--- expected {} ---\n--- actual {} ---\n",
            exit_text(expected.exit),
            exit_text(actual.exit)
        ));
    }
    out.push_str(&format!(
        "--- expected stdout ---\n{}\n--- actual stdout ---\n{}\n--- expected stderr ---\n{}\n--- actual stderr ---\n{}",
        show(&expected.stdout),
        show(&actual.stdout),
        show(&expected.stderr),
        show(&actual.stderr),
    ));
    out
}

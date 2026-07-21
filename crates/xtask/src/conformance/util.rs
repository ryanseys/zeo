//! Small dependency-free helpers shared across the conformance modules.

/// FNV-1a 64-bit -- a cache/bucketing hash, not cryptographic (nothing here
/// is adversarial).
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Strip trailing `\r` from every line (the C Makefile's CRLF normalization)
/// so snapshots compare byte-exactly across platforms.
pub fn normalize_crlf(bytes: &[u8]) -> Vec<u8> {
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

/// A test id as a flat filename (`analyze_fail/foo` -> `analyze_fail__foo`).
pub fn sanitize_id(id: &str) -> String {
    id.replace('/', "__")
}

/// Escape a string onto one line for the stamp format.
pub fn escape_line(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}

pub fn unescape_line(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => break,
        }
    }
    out
}

/// The trailing `n` lines of possibly-non-UTF-8 process output, lossily.
pub fn tail_lines(bytes: &[u8], n: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crlf() {
        assert_eq!(normalize_crlf(b"a\r\nb\n"), b"a\nb\n");
        assert_eq!(normalize_crlf(b"a\rb"), b"a\rb"); // lone \r untouched
    }

    #[test]
    fn escape_roundtrip() {
        let s = "line1\nline2\twith\\slash";
        assert_eq!(unescape_line(&escape_line(s)), s);
    }
}

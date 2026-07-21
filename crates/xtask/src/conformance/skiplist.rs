//! The committed skip list: `conformance/skiplist.tsv`, one
//! `suite<TAB>pattern<TAB>reason` row per line, `#` comments allowed.
//! Patterns are test-id globs (`*` wildcards). A skip is visible debt: the
//! runner records skipped tests in the scoreboard with the reason, and a
//! pattern matching zero tests is an error (stale entries rot loudly).

use std::path::Path;

pub struct SkipEntry {
    pub suite: String,
    pub pattern: String,
    pub reason: String,
}

pub fn load(path: &Path) -> Result<Vec<SkipEntry>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("reading {}: {e}", path.display())),
    };
    let mut entries = Vec::new();
    for (lineno, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let (Some(suite), Some(pattern), Some(reason)) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(format!(
                "{}:{}: expected `suite<TAB>pattern<TAB>reason`",
                path.display(),
                lineno + 1
            ));
        };
        entries.push(SkipEntry {
            suite: suite.to_owned(),
            pattern: pattern.to_owned(),
            reason: reason.to_owned(),
        });
    }
    Ok(entries)
}

/// The reason the id is skipped, if any entry for this suite matches it.
pub fn skip_reason<'a>(entries: &'a [SkipEntry], suite: &str, id: &str) -> Option<&'a str> {
    entries
        .iter()
        .find(|e| e.suite == suite && glob_match(&e.pattern, id))
        .map(|e| e.reason.as_str())
}

/// `*`-only glob matching (no `?`/classes -- test ids don't need them).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    fn inner(p: &[u8], t: &[u8]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some(b'*') => (0..=t.len()).any(|i| inner(&p[1..], &t[i..])),
            Some(&c) => t.first() == Some(&c) && inner(&p[1..], &t[1..]),
        }
    }
    inner(pattern.as_bytes(), text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::glob_match;

    #[test]
    fn globs() {
        assert!(glob_match("rbs/*", "rbs/arrays"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("exact", "exact"));
        assert!(glob_match("a*c", "abc"));
        assert!(glob_match("a*c", "ac"));
        assert!(!glob_match("a*c", "ab"));
        assert!(!glob_match("exact", "exact_no"));
    }
}

//! Case mapping per encoding family: full Unicode for UTF-8, ASCII-only for
//! US-ASCII/BINARY, ASCII plus the accented-letter ranges for Latin-1.

#[derive(Clone, Copy)]
pub(crate) enum CaseMode {
    Up,
    Down,
    Swap,
    Cap,
}

/// ASCII-only case fold (US-ASCII / ASCII-8BIT): only `a-z`/`A-Z` flip.
pub(crate) fn ascii_case_byte(b: u8, up: bool) -> u8 {
    if up && b.is_ascii_lowercase() {
        b - 32
    } else if !up && b.is_ascii_uppercase() {
        b + 32
    } else {
        b
    }
}

/// Case mapping for a 1-byte-per-character encoding through FULL Unicode
/// case rules -- CRuby's own model for the Latin/Windows/ISO single-byte
/// families (oracle-verified: Windows-1252 `\xE9` (é) upcases to `\xC9` (É),
/// Latin-1 `\xDF` (ß) upcases to `"SS"`, and a character whose cased form
/// the encoding can't represent -- `µ` -> `Μ` -- stays UNCHANGED).
///
/// Per byte: decode to its scalar (an unmapped byte stays as-is), apply the
/// mode's Unicode mapping (which may expand, ß -> SS), then re-encode every
/// resulting scalar; if ANY result scalar has no byte, the original byte is
/// kept verbatim.
pub(crate) fn case_single_byte(
    bytes: &[u8],
    mode: CaseMode,
    decode: &dyn Fn(u8) -> Option<char>,
    encode: &dyn Fn(char) -> Option<u8>,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for (i, &b) in bytes.iter().enumerate() {
        let Some(c) = decode(b) else {
            out.push(b);
            continue;
        };
        let up = match mode {
            CaseMode::Up => true,
            CaseMode::Down => false,
            CaseMode::Swap => !c.is_uppercase(),
            CaseMode::Cap => i == 0,
        };
        let cased: Vec<char> = if up {
            c.to_uppercase().collect()
        } else {
            c.to_lowercase().collect()
        };
        match cased
            .iter()
            .map(|cc| encode(*cc))
            .collect::<Option<Vec<u8>>>()
        {
            Some(bs) => out.extend(bs),
            None => out.push(b),
        }
    }
    out
}

pub(crate) fn case_bytes(bytes: &[u8], mode: CaseMode, fold: fn(u8, bool) -> u8) -> Vec<u8> {
    match mode {
        CaseMode::Up => bytes.iter().map(|b| fold(*b, true)).collect(),
        CaseMode::Down => bytes.iter().map(|b| fold(*b, false)).collect(),
        CaseMode::Swap => bytes
            .iter()
            .map(|b| {
                let up = fold(*b, true);
                if up != *b { up } else { fold(*b, false) }
            })
            .collect(),
        CaseMode::Cap => bytes
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if i == 0 {
                    fold(*b, true)
                } else {
                    fold(*b, false)
                }
            })
            .collect(),
    }
}

pub(crate) fn case_unicode(s: &str, mode: CaseMode) -> String {
    match mode {
        CaseMode::Up => s.to_uppercase(),
        CaseMode::Down => s.to_lowercase(),
        CaseMode::Swap => s
            .chars()
            .flat_map(|c| {
                if c.is_uppercase() {
                    c.to_lowercase().collect::<Vec<_>>()
                } else {
                    c.to_uppercase().collect()
                }
            })
            .collect(),
        CaseMode::Cap => {
            let mut chars = s.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
                None => String::new(),
            }
        }
    }
}

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

/// Latin-1 case fold: ASCII plus the `À`-`Þ` / `à`-`þ` accented-letter ranges
/// (skipping the `×`/`÷` math signs at 0xD7/0xF7).
pub(crate) fn latin1_case_byte(b: u8, up: bool) -> u8 {
    if up {
        if b.is_ascii_lowercase() || ((0xE0..=0xFE).contains(&b) && b != 0xF7) {
            b - 32
        } else {
            b
        }
    } else if b.is_ascii_uppercase() || ((0xC0..=0xDE).contains(&b) && b != 0xD7) {
        b + 32
    } else {
        b
    }
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

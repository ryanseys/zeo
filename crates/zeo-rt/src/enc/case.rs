//! Case mapping per encoding family: full Unicode for UTF-8, ASCII-only for
//! US-ASCII/BINARY, ASCII plus the accented-letter ranges for Latin-1.

#[derive(Clone, Copy)]
pub(crate) enum CaseMode {
    Up,
    Down,
    Swap,
    Cap,
}

/// The `:ascii` / `:turkic` / `:lithuanian` / `:fold` options ruby's
/// case-mapping rows take (CRuby's `check_case_options`).
///
/// `lithuanian` is accepted and carried but changes nothing here, exactly as
/// in CRuby: Onigmo's tables implement the Turkish/Azeri rules and leave the
/// Lithuanian accent rules to the ordinary map, so `"i".upcase(:lithuanian)`
/// is `"I"`. It stays a field so the combination `(:turkic, :lithuanian)` is
/// still the one ruby accepts.
#[derive(Clone, Copy, Default)]
pub(crate) struct CaseOptions {
    pub ascii: bool,
    pub turkic: bool,
    pub lithuanian: bool,
    pub fold: bool,
}

impl CaseOptions {
    pub(crate) fn is_plain(self) -> bool {
        !self.ascii && !self.turkic && !self.fold
    }
}

/// Turkish/Azeri dotted-vs-dotless I, the one place a language changes the
/// map: `i` upcases to `İ`, `I` downcases to `ı`, and `İ` downcases to a
/// plain `i` rather than to `i` plus a combining dot.
fn turkic_map(c: char, up: bool) -> Option<&'static str> {
    match (c, up) {
        ('i', true) => Some("\u{130}"),
        ('I', false) => Some("\u{131}"),
        ('\u{130}', false) => Some("i"),
        ('\u{131}', true) => Some("I"),
        _ => None,
    }
}

/// One character's cased form under `opts` -- the shared core of every
/// encoding's mapping.
pub(crate) fn case_char(c: char, up: bool, opts: CaseOptions) -> String {
    if opts.ascii {
        return if up {
            c.to_ascii_uppercase().to_string()
        } else {
            c.to_ascii_lowercase().to_string()
        };
    }
    if opts.turkic
        && let Some(mapped) = turkic_map(c, up)
    {
        return mapped.to_string();
    }
    if opts.fold {
        // Folding is downcasing with the expansions `to_lowercase` does not
        // make -- see `crate::enc::casefold`.
        if let Ok(i) = crate::enc::casefold::FOLD_EXCEPTIONS.binary_search_by_key(&c, |(k, _)| *k) {
            return crate::enc::casefold::FOLD_EXCEPTIONS[i].1.to_string();
        }
        return c.to_lowercase().collect();
    }
    if up {
        c.to_uppercase().collect()
    } else {
        c.to_lowercase().collect()
    }
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

pub(crate) fn case_unicode_opts(s: &str, mode: CaseMode, opts: CaseOptions) -> String {
    // The plain map is `str`'s own, which handles the context-sensitive
    // rules (a final sigma, `İ` -> `i` plus a combining dot) that a
    // per-character walk cannot.
    if opts.is_plain() {
        return match mode {
            CaseMode::Up => s.to_uppercase(),
            CaseMode::Down => s.to_lowercase(),
            CaseMode::Swap => s
                .chars()
                .map(|c| match swapcase_exception(c) {
                    // A TITLECASE character swaps per HALF (`Dz` -> `dZ`),
                    // which is not "the other case" of anything -- see
                    // `enc::titlecase`.
                    Some(t) => t.to_string(),
                    None if c.is_uppercase() => c.to_lowercase().collect(),
                    None => c.to_uppercase().collect(),
                })
                .collect(),
            CaseMode::Cap => {
                let mut chars = s.chars();
                match chars.next() {
                    // The first character TITLECASES, which is not the same
                    // as uppercasing it: `\u{1f3}` capitalizes to `\u{1f2}`
                    // and upcases to `\u{1f1}`.
                    Some(c) => titlecase(c) + &chars.as_str().to_lowercase(),
                    None => String::new(),
                }
            }
        };
    }
    s.chars()
        .enumerate()
        .map(|(i, c)| {
            let up = match mode {
                CaseMode::Up => true,
                CaseMode::Down => false,
                CaseMode::Swap => !c.is_uppercase(),
                CaseMode::Cap => i == 0,
            };
            if up && matches!(mode, CaseMode::Cap) && i == 0 && opts.is_plain() {
                return titlecase(c);
            }
            case_char(c, up, opts)
        })
        .collect()
}

/// `c`'s TITLECASE, which is not its uppercase for the Dz/Lj/Nj digraphs or
/// the Greek iota-subscript forms -- Rust's standard library has no mapping
/// for the third member of Unicode's case triple, so the deltas are a
/// generated table (`enc::titlecase`).
pub(crate) fn titlecase(c: char) -> String {
    match crate::enc::titlecase::TITLECASE.binary_search_by_key(&c, |(k, _)| *k) {
        Ok(i) => crate::enc::titlecase::TITLECASE[i].1.to_string(),
        Err(_) => c.to_uppercase().collect(),
    }
}

/// `c`'s swap when it is not simply the other case -- a titlecase character
/// swaps per HALF (`Dz` -> `dZ`), which no per-char mapping can express.
fn swapcase_exception(c: char) -> Option<&'static str> {
    crate::enc::titlecase::SWAPCASE_EXCEPTIONS
        .binary_search_by_key(&c, |(k, _)| *k)
        .ok()
        .map(|i| crate::enc::titlecase::SWAPCASE_EXCEPTIONS[i].1)
}

/// CRuby's `check_case_options`, term for term -- the acceptance set is not
/// derivable from the option names: `:turkic` and `:lithuanian` may be given
/// TOGETHER (in either order) and nothing else may be given twice, `:fold`
/// is downcase-only, and each refusal has its own message.
pub(crate) fn check_case_options(
    args: &[crate::RubyValue],
    down: bool,
) -> Result<CaseOptions, crate::Signal> {
    let mut opts = CaseOptions::default();
    if args.is_empty() {
        return Ok(opts);
    }
    if args.len() > 2 {
        return Err(crate::builtins::arg_error!("too many options"));
    }
    let name = |v: &crate::RubyValue| match v {
        crate::RubyValue::Symbol(s) => Some(s.name().to_string()),
        _ => None,
    };
    let first = name(&args[0]);
    match first.as_deref() {
        Some("turkic" | "lithuanian") => {
            opts.turkic = first.as_deref() == Some("turkic");
            opts.lithuanian = !opts.turkic;
            if let Some(second) = args.get(1) {
                match (name(second).as_deref(), opts.turkic) {
                    (Some("lithuanian"), true) => opts.lithuanian = true,
                    (Some("turkic"), false) => opts.turkic = true,
                    _ => return Err(crate::builtins::arg_error!("invalid second option")),
                }
            }
        }
        _ if args.len() > 1 => return Err(crate::builtins::arg_error!("too many options")),
        Some("ascii") => opts.ascii = true,
        Some("fold") if down => opts.fold = true,
        Some("fold") => {
            return Err(crate::builtins::arg_error!(
                "option :fold only allowed for downcasing"
            ));
        }
        _ => return Err(crate::builtins::arg_error!("invalid option")),
    }
    Ok(opts)
}

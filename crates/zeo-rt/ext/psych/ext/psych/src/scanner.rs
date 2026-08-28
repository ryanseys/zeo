//! Psych's plain-scalar resolver, which reads YAML **1.1** -- not the 1.2
//! rules yaml-rust2 applies.
//!
//! The difference is not academic. It is the Norway problem in both
//! directions: `no` is `false` and `017` is `15`, while `1.0e3` and `1E2`
//! stay STRINGS. Every rule below was probed against ruby 4.0.6's psych
//! 5.4.0 rather than read from a spec, and `tests/yaml_scalar_rules.rb` is
//! that probe made permanent.
//!
//! The order of the arms is SEMANTIC, not stylistic: psych's own `tokenize`
//! runs the wordy cases (null, boolean) before the numeric ones, so a
//! scalar that could read as both takes the word.
//!
//! The three that trip people up:
//!
//!   * A FLOAT needs a DOT, and an exponent needs a SIGN. `1.0e+3` is a
//!     float; `1.0e3` and `1e+3` are strings.
//!   * An `_` must sit BETWEEN digits (`1_0` is ten, `10_` and `1__0` are
//!     strings), while a `,` may sit anywhere (`1,00,0` is a thousand).
//!   * `y` and `n` ALONE are strings. Only the spelled-out words are
//!     booleans -- in any case, so `yEs` is `true`.

use crate::{RubyValue, string_new};

/// What a plain scalar resolves to, before any permitted-class gate.
///
/// The TYPED arms are separate from the plain ones because psych gates
/// them: a bare `2001-12-14` is a `Date`, and `safe_load` refuses to build
/// one unless the caller permitted it.
pub(super) enum Scalar {
    Plain(RubyValue),
    /// A `y-m-d` date, as its three fields.
    Date(i32, u32, u32),
    /// A timestamp, as the text psych would hand to `Time`.
    Timestamp(String),
    /// A `:name` symbol, gated like the rest.
    Symbol(String),
}

/// Resolve one PLAIN scalar. A quoted or block scalar never reaches here --
/// psych scans only the plain style, which is what keeps `'017'` a string.
pub(super) fn resolve(value: &str) -> Scalar {
    // Empty and the null spellings, before anything numeric.
    if value.is_empty() || value == "~" {
        return Scalar::Plain(RubyValue::Nil);
    }
    let lower = value.to_ascii_lowercase();
    if lower == "null" {
        return Scalar::Plain(RubyValue::Nil);
    }
    match lower.as_str() {
        "yes" | "true" | "on" => return Scalar::Plain(RubyValue::Bool(true)),
        "no" | "false" | "off" => return Scalar::Plain(RubyValue::Bool(false)),
        _ => {}
    }
    // `:name` -- psych's own Symbol spelling, and how a dumped Hash's
    // symbol keys survive the round trip.
    if let Some(name) = value.strip_prefix(':').filter(|n| !n.is_empty()) {
        let name = name
            .strip_prefix('"')
            .and_then(|n| n.strip_suffix('"'))
            .unwrap_or(name);
        return Scalar::Symbol(name.to_string());
    }
    if let Some(v) = infinity_or_nan(value) {
        return Scalar::Plain(v);
    }
    if let Some((y, m, d)) = date(value) {
        return Scalar::Date(y, m, d);
    }
    if is_timestamp(value) {
        return Scalar::Timestamp(value.to_string());
    }
    if let Some(v) = sexagesimal(value) {
        return Scalar::Plain(v);
    }
    if let Some(v) = integer(value) {
        return Scalar::Plain(v);
    }
    if let Some(v) = float(value) {
        return Scalar::Plain(v);
    }
    Scalar::Plain(RubyValue::Str(string_new(value.to_string())))
}

fn infinity_or_nan(value: &str) -> Option<RubyValue> {
    let (sign, rest) = match value.strip_prefix(['+', '-']) {
        Some(r) if value.starts_with('-') => (-1.0, r),
        Some(r) => (1.0, r),
        None => (1.0, value),
    };
    match rest.to_ascii_lowercase().as_str() {
        // `.nan` takes NO sign: `-.NAN` is a string.
        ".nan" if rest == value => Some(RubyValue::Float(f64::NAN)),
        ".inf" => Some(RubyValue::Float(sign * f64::INFINITY)),
        _ => None,
    }
}

/// Digits with psych's separators removed, or `None` when an `_` is not
/// between two digits -- see the module doc.
fn strip_separators(digits: &str) -> Option<String> {
    let bytes = digits.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'_' {
            continue;
        }
        let before = i.checked_sub(1).map(|j| bytes[j]).unwrap_or(b'_');
        let after = bytes.get(i + 1).copied().unwrap_or(b'_');
        if !before.is_ascii_alphanumeric() || !after.is_ascii_alphanumeric() {
            return None;
        }
    }
    Some(digits.chars().filter(|&c| c != '_' && c != ',').collect())
}

fn integer(value: &str) -> Option<RubyValue> {
    let (neg, body) = match value.as_bytes().first() {
        Some(b'-') => (true, &value[1..]),
        Some(b'+') => (false, &value[1..]),
        _ => (false, value),
    };
    let digits = strip_separators(body)?;
    if digits.is_empty() {
        return None;
    }
    let (radix, text) = if let Some(t) = digits.strip_prefix("0b") {
        (2, t)
    } else if let Some(t) = digits.strip_prefix("0x") {
        (16, t)
    } else if digits.len() > 1 && digits.starts_with('0') {
        (8, &digits[1..])
    } else {
        (10, digits.as_str())
    };
    if text.is_empty() {
        return None;
    }
    let ok = text.chars().all(|c| c.is_digit(radix));
    if !ok {
        return None;
    }
    // Exact however long, the way every other integer in this runtime is.
    let n = num_bigint::BigInt::parse_bytes(text.as_bytes(), radix)?;
    let n = if neg { -n } else { n };
    Some(crate::builtins::integer::int_value(n))
}

/// The 1.1 float: a MANDATORY dot, and a SIGNED exponent if any.
fn float(value: &str) -> Option<RubyValue> {
    let body = value.strip_prefix(['+', '-']).unwrap_or(value);
    let neg = value.starts_with('-');
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(i) => {
            let exp = &body[i + 1..];
            // The sign is not optional here -- that is the whole rule.
            if !exp.starts_with(['+', '-']) || exp.len() < 2 {
                return None;
            }
            if !exp[1..].chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            (&body[..i], Some(exp))
        }
        None => (body, None),
    };
    let (int_part, frac) = mantissa.split_once('.')?;
    let int_part = strip_separators(int_part)?;
    if !int_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let frac = strip_separators(frac)?;
    if !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if int_part.is_empty() && frac.is_empty() {
        return None;
    }
    let text = format!(
        "{}{}.{}{}",
        if neg { "-" } else { "" },
        if int_part.is_empty() { "0" } else { &int_part },
        if frac.is_empty() { "0" } else { &frac },
        exponent.map(|e| format!("e{e}")).unwrap_or_default()
    );
    text.parse().ok().map(RubyValue::Float)
}

/// A CLOCK, not a general base-60 number: `h:m` or `h:m:s`, worth
/// `h * 3600 + m * 60 + s` seconds. So `1:00` is an hour (3,600) rather
/// than a minute, `1:02` is 3,720, and a fourth group makes it a string.
///
/// The hour is unbounded (`100:00` is 360,000); the minute and second are
/// real ones, so `1:60` is a string. A fractional second makes the whole
/// value a Float.
fn sexagesimal(value: &str) -> Option<RubyValue> {
    if !value.contains(':') {
        return None;
    }
    let (neg, body) = match value.as_bytes().first() {
        Some(b'-') => (true, &value[1..]),
        Some(b'+') => (false, &value[1..]),
        _ => (false, value),
    };
    let parts: Vec<&str> = body.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let hours = strip_separators(parts[0])?;
    if hours.is_empty() || !hours.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut total: f64 = hours.parse::<f64>().ok()? * 3600.0;
    let mut whole = true;
    for (i, part) in parts[1..].iter().enumerate() {
        let scale = if i == 0 { 60.0 } else { 1.0 };
        let (digits, frac) = match part.split_once('.') {
            Some((d, f)) => (d, Some(f)),
            None => (*part, None),
        };
        if digits.len() > 2 || digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let n: f64 = digits.parse().ok()?;
        if n > 59.0 {
            return None;
        }
        total += n * scale;
        if let Some(f) = frac {
            let f = strip_separators(f)?;
            if f.is_empty() || !f.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            whole = false;
            total += format!("0.{f}").parse::<f64>().ok()? * scale;
        }
    }
    let total = if neg { -total } else { total };
    Some(match whole {
        true => RubyValue::Int(total as i64),
        false => RubyValue::Float(total),
    })
}

/// `y-m-d`, and nothing after it.
fn date(value: &str) -> Option<(i32, u32, u32)> {
    let mut parts = value.split('-');
    let y = parts.next()?;
    let m = parts.next()?;
    let d = parts.next()?;
    if parts.next().is_some() || y.len() != 4 {
        return None;
    }
    let num = |s: &str, max: usize| -> Option<u32> {
        if s.is_empty() || s.len() > max || !s.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        s.parse().ok()
    };
    if !y.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((y.parse().ok()?, num(m, 2)?, num(d, 2)?))
}

/// `y-m-d` followed by `T`/`t`/spaces and a clock -- psych's timestamp.
pub(super) fn is_timestamp(value: &str) -> bool {
    let Some(sep) = value.find(['T', 't', ' ']) else {
        return false;
    };
    if date(&value[..sep]).is_none() {
        return false;
    }
    let clock = value[sep + 1..].trim_start();
    let mut fields = clock.splitn(3, ':');
    let (Some(h), Some(m), Some(rest)) = (fields.next(), fields.next(), fields.next()) else {
        return false;
    };
    let two = |s: &str| !s.is_empty() && s.len() <= 2 && s.chars().all(|c| c.is_ascii_digit());
    two(h) && two(m) && rest.chars().next().is_some_and(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(value: &str) -> String {
        match resolve(value) {
            Scalar::Plain(v) => v.to_display_string(),
            Scalar::Date(y, m, d) => format!("date {y}-{m}-{d}"),
            Scalar::Timestamp(t) => format!("time {t}"),
            Scalar::Symbol(s) => format!(":{s}"),
        }
    }

    #[test]
    fn the_words_are_booleans_in_any_case_and_the_letters_are_not() {
        for t in ["yes", "Yes", "YES", "yEs", "true", "TRUE", "on", "On"] {
            assert_eq!(plain(t), "true", "{t}");
        }
        for f in ["no", "No", "NO", "nO", "false", "FALSE", "off", "OFF"] {
            assert_eq!(plain(f), "false", "{f}");
        }
        // A single letter is a STRING -- the rule everybody forgets.
        for s in ["y", "Y", "n", "N", "true.", "TRUE_"] {
            assert_eq!(plain(s), s, "{s}");
        }
    }

    #[test]
    fn null_has_four_spellings_and_one_of_them_is_nothing() {
        for n in ["", "~", "null", "Null", "NULL"] {
            assert_eq!(plain(n), "", "{n}");
        }
        assert_eq!(plain("NULL."), "NULL.");
    }

    #[test]
    fn an_integer_reads_every_radix_yaml_11_has() {
        assert_eq!(plain("017"), "15");
        assert_eq!(plain("-017"), "-15");
        assert_eq!(plain("0x1A"), "26");
        assert_eq!(plain("0b101"), "5");
        assert_eq!(plain("1_000"), "1000");
        assert_eq!(plain("1,00,0"), "1000");
        assert_eq!(plain("0_1"), "1");
        assert_eq!(plain("+5"), "5");
        assert_eq!(plain("00"), "0");
        // ...and refuses what it cannot read as one.
        assert_eq!(plain("0o17"), "0o17");
        assert_eq!(plain("08"), "08");
        assert_eq!(plain("10_"), "10_");
        assert_eq!(plain("1__0"), "1__0");
        assert_eq!(plain("_1"), "_1");
        assert_eq!(plain("0xz"), "0xz");
    }

    #[test]
    fn a_float_needs_a_dot_and_a_signed_exponent() {
        assert_eq!(plain("1.5"), "1.5");
        assert_eq!(plain(".5"), "0.5");
        assert_eq!(plain("5."), "5.0");
        assert_eq!(plain("1.0e+3"), "1000.0");
        assert_eq!(plain("1.0e-3"), "0.001");
        assert_eq!(plain(".5e+2"), "50.0");
        assert_eq!(plain("5.e+2"), "500.0");
        // No dot, or no sign on the exponent: a STRING.
        assert_eq!(plain("1.0e3"), "1.0e3");
        assert_eq!(plain("1E2"), "1E2");
        assert_eq!(plain("1e+3"), "1e+3");
        assert_eq!(plain("1.2.3"), "1.2.3");
    }

    #[test]
    fn infinity_and_nan_have_their_own_spellings() {
        assert_eq!(plain(".inf"), "Infinity");
        assert_eq!(plain("+.inf"), "Infinity");
        assert_eq!(plain("-.inf"), "-Infinity");
        assert_eq!(plain(".Inf"), "Infinity");
        assert_eq!(plain(".nan"), "NaN");
        assert_eq!(plain(".NAN"), "NaN");
        // A sign on NaN makes it a string.
        assert_eq!(plain("-.NAN"), "-.NAN");
    }

    #[test]
    fn a_clock_counts_seconds_from_the_hour() {
        assert_eq!(plain("1:02:03"), "3723");
        assert_eq!(plain("1:00"), "3600");
        assert_eq!(plain("1:02"), "3720");
        assert_eq!(plain("-1:00"), "-3600");
        assert_eq!(plain("0:0"), "0");
        assert_eq!(plain("60:00"), "216000");
        assert_eq!(plain("100:00"), "360000");
        assert_eq!(plain("12:34:56.78"), "45296.78");
        assert_eq!(plain("21:59:43"), "79183");
        // A group past 59, too many digits, or a fourth group: a string.
        assert_eq!(plain("1:60"), "1:60");
        assert_eq!(plain("1:2:3:4"), "1:2:3:4");
        assert_eq!(plain("1:"), "1:");
        assert_eq!(plain(":1"), ":1");
    }

    #[test]
    fn dates_and_timestamps_are_told_apart() {
        assert_eq!(plain("2001-12-14"), "date 2001-12-14");
        assert_eq!(plain("2001-1-1"), "date 2001-1-1");
        assert!(plain("2001-12-14 21:59:43").starts_with("time "));
        assert!(plain("2001-12-14t21:59:43.10Z").starts_with("time "));
        // Neither: eight digits is an integer, and a bare clock is base 60.
        assert_eq!(plain("20011214"), "20011214");
        assert_eq!(plain("21:59:43"), "79183");
        assert_eq!(plain("1-2"), "1-2");
    }

    #[test]
    fn a_symbol_keeps_its_quoted_spelling() {
        assert_eq!(plain(":a"), ":a");
        assert_eq!(plain(":\"a b\""), ":a b");
        assert_eq!(plain(":"), ":");
    }

    /// Nothing the scanner is handed can panic it -- it is fed whatever a
    /// document holds.
    #[test]
    fn every_short_byte_string_resolves_without_panicking() {
        let alphabet = b"0123456789+-.:_,eEyYnNoOtTfFuUlLaAsSxXbB \t~'\"[]{}";
        for &a in alphabet {
            let _ = plain(&String::from_utf8_lossy(&[a]));
            for &b in alphabet {
                let _ = plain(&String::from_utf8_lossy(&[a, b]));
                for &c in alphabet {
                    let _ = plain(&String::from_utf8_lossy(&[a, b, c]));
                }
            }
        }
    }
}

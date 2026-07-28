//! The `BigDecimal` value: sign / decimal coefficient / decimal exponent,
//! CRuby's own model (`0.<digits> * 10^exponent`) with the coefficient held
//! as one arbitrary-precision integer instead of base-10^9 words. Trailing
//! zeros are stripped on construction, so a coefficient's digit count IS
//! `n_significant_digits`.

use num_bigint::BigUint;
use num_traits::{One, Zero};

/// One BigDecimal value. `Fin`'s invariants: `coeff > 0`, `coeff % 10 != 0`
/// (normalized), and the value is `sign * 0.<coeff digits> * 10^exp`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum BD {
    NaN,
    /// `sign` is `1` or `-1`.
    Inf(i8),
    /// Signed zero, as CRuby keeps it (`BigDecimal("-0")` is `-0.0`).
    Zero(i8),
    Fin { sign: i8, coeff: BigUint, exp: i64 },
}

impl BD {
    /// Builds a finite value from raw parts, stripping trailing zeros and
    /// collapsing an empty coefficient to (positive-sign preserving) zero.
    pub(crate) fn fin(sign: i8, mut coeff: BigUint, exp: i64) -> BD {
        if coeff.is_zero() {
            return BD::Zero(sign);
        }
        let ten = BigUint::from(10u8);
        loop {
            let (q, r) = num_integer::Integer::div_rem(&coeff, &ten);
            if !r.is_zero() {
                break;
            }
            coeff = q;
        }
        BD::Fin { sign, coeff, exp }
    }

    pub(crate) fn one() -> BD {
        BD::Fin { sign: 1, coeff: BigUint::one(), exp: 1 }
    }

    pub(crate) fn sign_factor(&self) -> i8 {
        match self {
            BD::NaN => 0,
            BD::Inf(s) | BD::Zero(s) => *s,
            BD::Fin { sign, .. } => *sign,
        }
    }

    /// CRuby's `#sign`: `0` NaN, `±1` zero, `±2` finite, `±3` infinite.
    pub(crate) fn sign_code(&self) -> i64 {
        match self {
            BD::NaN => 0,
            BD::Zero(s) => *s as i64,
            BD::Fin { sign, .. } => 2 * *sign as i64,
            BD::Inf(s) => 3 * *s as i64,
        }
    }

    /// Digit count of the coefficient -- `n_significant_digits` (0 for
    /// zero and the specials).
    pub(crate) fn nsd(&self) -> u64 {
        match self {
            BD::Fin { coeff, .. } => ndigits(coeff),
            _ => 0,
        }
    }

    /// `#exponent`: the decimal exponent (0 for zero and the specials).
    pub(crate) fn exponent(&self) -> i64 {
        match self {
            BD::Fin { exp, .. } => *exp,
            _ => 0,
        }
    }

    /// `#precision`: integer digits plus fraction digits, 0 for specials
    /// and zero -- oracle-pinned (`123.45` is 5, `1e20` is 21, `1e-20` is
    /// 20, `100` is 3).
    pub(crate) fn precision(&self) -> i64 {
        match self {
            BD::Fin { coeff, exp, .. } => {
                let l = ndigits(coeff) as i64;
                exp.max(&0) + (l - exp).max(0)
            }
            _ => 0,
        }
    }

    /// `#scale`: digits after the decimal point.
    pub(crate) fn scale(&self) -> i64 {
        match self {
            BD::Fin { coeff, exp, .. } => (ndigits(coeff) as i64 - exp).max(0),
            _ => 0,
        }
    }

    /// CRuby's word count (`Real->Prec`): the coefficient laid out in
    /// base-10^9 words aligned to a word-multiple exponent. Feeds the
    /// coercion-precision rule, nothing user-visible.
    pub(crate) fn word_prec(&self) -> u64 {
        match self {
            BD::Fin { coeff, exp, .. } => {
                let l = ndigits(coeff) as i64;
                let we = exp.div_euclid(9) + i64::from(exp.rem_euclid(9) != 0);
                let lead = we * 9 - exp;
                ((lead + l) as u64).div_ceil(9)
            }
            _ => 1,
        }
    }

    pub(crate) fn is_zero(&self) -> bool {
        matches!(self, BD::Zero(_))
    }

    pub(crate) fn neg(&self) -> BD {
        match self {
            BD::NaN => BD::NaN,
            BD::Inf(s) => BD::Inf(-s),
            BD::Zero(s) => BD::Zero(-s),
            BD::Fin { sign, coeff, exp } => {
                BD::Fin { sign: -sign, coeff: coeff.clone(), exp: *exp }
            }
        }
    }

    pub(crate) fn abs(&self) -> BD {
        match self {
            BD::NaN => BD::NaN,
            BD::Inf(_) => BD::Inf(1),
            BD::Zero(_) => BD::Zero(1),
            BD::Fin { coeff, exp, .. } => BD::Fin { sign: 1, coeff: coeff.clone(), exp: *exp },
        }
    }

    /// `#_decimal_shift(i)`: exact multiplication by `10^i` -- the ruby
    /// half's workhorse.
    pub(crate) fn decimal_shift(&self, i: i64) -> BD {
        match self {
            BD::Fin { sign, coeff, exp } => {
                BD::Fin { sign: *sign, coeff: coeff.clone(), exp: exp + i }
            }
            other => other.clone(),
        }
    }
}

/// Decimal digit count of a positive integer.
pub(crate) fn ndigits(n: &BigUint) -> u64 {
    if n.is_zero() {
        return 0;
    }
    // to_str_radix is O(n^2)-ish but coefficients here are user-scale.
    n.to_str_radix(10).len() as u64
}

pub(crate) fn pow10(n: u64) -> BigUint {
    BigUint::from(10u8).pow(n as u32)
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parses per CRuby's own scanner (`VpAlloc`): optional sign, digits with
/// underscores between them, optional fraction, optional `eEdD` exponent,
/// surrounding ASCII whitespace, and the literal specials. `strict` raises
/// the caller's ArgumentError (returns None); loose mode reads the longest
/// valid prefix and answers zero for none (`"x".to_d`).
pub(crate) fn parse(s: &str, strict: bool) -> Option<BD> {
    let t = s.trim_matches(|c: char| c.is_ascii_whitespace());
    let (sign, rest) = match t.strip_prefix('-') {
        Some(r) => (-1i8, r),
        None => (1i8, t.strip_prefix('+').unwrap_or(t)),
    };
    match rest {
        "Infinity" => return Some(BD::Inf(sign)),
        "NaN" if sign == 1 => return Some(BD::NaN),
        _ => {}
    }

    let bytes = rest.as_bytes();
    let mut int_digits = String::new();
    let mut frac_digits = String::new();
    let mut i = 0;
    let mut prev_digit = false;
    while i < bytes.len() {
        match bytes[i] {
            b'0'..=b'9' => {
                int_digits.push(bytes[i] as char);
                prev_digit = true;
                i += 1;
            }
            b'_' if prev_digit && matches!(bytes.get(i + 1), Some(b'0'..=b'9')) => i += 1,
            _ => break,
        }
    }
    let mut valid = !int_digits.is_empty();
    if bytes.get(i) == Some(&b'.') {
        let dot = i;
        i += 1;
        prev_digit = false;
        while i < bytes.len() {
            match bytes[i] {
                b'0'..=b'9' => {
                    frac_digits.push(bytes[i] as char);
                    prev_digit = true;
                    i += 1;
                }
                b'_' if prev_digit && matches!(bytes.get(i + 1), Some(b'0'..=b'9')) => i += 1,
                _ => break,
            }
        }
        if frac_digits.is_empty() {
            // A bare trailing dot: strict rejects, loose stops before it.
            if strict {
                return None;
            }
            i = dot;
        } else {
            valid = true;
        }
    }
    if !valid {
        return if strict { None } else { Some(BD::Zero(sign)) };
    }
    let mut exp10: i64 = 0;
    if matches!(bytes.get(i), Some(b'e' | b'E' | b'd' | b'D')) {
        let mark = i;
        i += 1;
        let esign: i64 = match bytes.get(i) {
            Some(b'-') => {
                i += 1;
                -1
            }
            Some(b'+') => {
                i += 1;
                1
            }
            _ => 1,
        };
        let mut num = String::new();
        prev_digit = false;
        while i < bytes.len() {
            match bytes[i] {
                b'0'..=b'9' => {
                    num.push(bytes[i] as char);
                    prev_digit = true;
                    i += 1;
                }
                b'_' if prev_digit && matches!(bytes.get(i + 1), Some(b'0'..=b'9')) => i += 1,
                _ => break,
            }
        }
        if num.is_empty() {
            if strict {
                return None;
            }
            i = mark;
        } else {
            exp10 = esign * num.parse::<i64>().ok()?;
        }
    }
    if strict && i != bytes.len() {
        return None;
    }

    let digits: String = int_digits.chars().chain(frac_digits.chars()).collect();
    let trimmed = digits.trim_start_matches('0');
    if trimmed.is_empty() {
        return Some(BD::Zero(sign));
    }
    let lead_zeros = (digits.len() - trimmed.len()) as i64;
    let coeff: BigUint = trimmed.parse().ok()?;
    let exp = int_digits.len() as i64 - lead_zeros + exp10;
    Some(BD::fin(sign, coeff, exp))
}

/// A Float, per CRuby: the SHORTEST round-trip decimal form when `digs` is
/// 0, else dtoa to `digs` significant digits (Rust's `{}` float formatting
/// is the same shortest-repr algorithm).
pub(crate) fn from_f64(d: f64, digs: usize) -> BD {
    if d.is_nan() {
        return BD::NaN;
    }
    if d.is_infinite() {
        return BD::Inf(if d > 0.0 { 1 } else { -1 });
    }
    if d == 0.0 {
        return BD::Zero(if d.is_sign_negative() { -1 } else { 1 });
    }
    let s = if digs == 0 {
        format!("{d:e}")
    } else {
        format!("{d:.*e}", digs - 1)
    };
    parse(&s, true).expect("float formatting is always parseable")
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

/// `#to_s` with CRuby's format string: `/^[+ ]?(\d+)?[EF]?/` -- an optional
/// plus/space for positive numbers, an optional digit-group size, and the
/// notation letter (engineering `E`, the default, or plain-Float `F`).
pub(crate) fn to_s(bd: &BD, fmt: &str) -> String {
    let mut plus = "";
    let mut rest = fmt;
    if let Some(r) = rest.strip_prefix('+') {
        plus = "+";
        rest = r;
    } else if let Some(r) = rest.strip_prefix(' ') {
        plus = " ";
        rest = r;
    }
    let group: usize = {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        rest = &rest[digits.len()..];
        digits.parse().unwrap_or(0)
    };
    let float_form = rest.starts_with('F') || rest.starts_with('f');

    match bd {
        BD::NaN => "NaN".to_string(),
        BD::Inf(1) => format!("{plus}Infinity"),
        BD::Inf(_) => "-Infinity".to_string(),
        BD::Zero(s) => {
            let sign = if *s < 0 { "-" } else { plus };
            format!("{sign}0.0")
        }
        BD::Fin { sign, coeff, exp } => {
            let sign = if *sign < 0 { "-" } else { plus };
            let digits = coeff.to_str_radix(10);
            if float_form {
                let e = *exp;
                let l = digits.len() as i64;
                let (int_part, frac_part) = if e <= 0 {
                    ("0".to_string(), format!("{}{}", "0".repeat((-e) as usize), digits))
                } else if e >= l {
                    (format!("{}{}", digits, "0".repeat((e - l) as usize)), "0".to_string())
                } else {
                    let (a, b) = digits.split_at(e as usize);
                    (a.to_string(), b.to_string())
                };
                format!(
                    "{sign}{}.{}",
                    group_digits(&int_part, group, true),
                    group_digits(&frac_part, group, false)
                )
            } else {
                format!("{sign}0.{}e{}", group_digits(&digits, group, false), exp)
            }
        }
    }
}

/// Space-groups a digit run: fraction digits from the left, integer digits
/// from the right (`123 456.789`), matching the classic extension.
fn group_digits(digits: &str, group: usize, from_right: bool) -> String {
    if group == 0 || digits.len() <= group {
        return digits.to_string();
    }
    let chunks: Vec<&[u8]> = if from_right {
        let mut v: Vec<&[u8]> = digits.as_bytes().rchunks(group).collect();
        v.reverse();
        v
    } else {
        digits.as_bytes().chunks(group).collect()
    };
    chunks
        .into_iter()
        .map(|c| std::str::from_utf8(c).expect("ASCII digits"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(input: &str) -> BD {
        parse(input, true).expect("valid")
    }

    #[test]
    fn parses_and_normalizes() {
        assert_eq!(s("1.5"), BD::Fin { sign: 1, coeff: 15u8.into(), exp: 1 });
        assert_eq!(s("-0.003"), BD::Fin { sign: -1, coeff: 3u8.into(), exp: -2 });
        assert_eq!(s("1e100"), BD::Fin { sign: 1, coeff: 1u8.into(), exp: 101 });
        assert_eq!(s("100"), BD::Fin { sign: 1, coeff: 1u8.into(), exp: 3 });
        assert_eq!(s("1_000"), s("1000"));
        assert_eq!(s(" 1.5 "), s("1.5"));
        assert_eq!(s(".5"), BD::Fin { sign: 1, coeff: 5u8.into(), exp: 0 });
        assert_eq!(s("-0"), BD::Zero(-1));
        assert_eq!(s("Infinity"), BD::Inf(1));
        assert_eq!(s("NaN"), BD::NaN);
        assert!(parse("bogus", true).is_none());
        assert!(parse("1.5x", true).is_none());
        assert!(parse("1.", true).is_none());
        assert_eq!(parse("1.5x", false), Some(s("1.5")));
        assert_eq!(parse("x", false), Some(BD::Zero(1)));
    }

    #[test]
    fn formats_the_oracle_shapes() {
        assert_eq!(to_s(&s("1.5"), ""), "0.15e1");
        assert_eq!(to_s(&s("-0.003"), ""), "-0.3e-2");
        assert_eq!(to_s(&s("-0"), ""), "-0.0");
        assert_eq!(to_s(&s("1234.5678"), "F"), "1234.5678");
        assert_eq!(to_s(&s("0.000012345"), "F"), "0.000012345");
        assert_eq!(to_s(&s("1e30"), "F"), "1000000000000000000000000000000.0");
        assert_eq!(to_s(&s("1234.5678"), "+"), "+0.12345678e4");
        assert_eq!(to_s(&s("1234.5678"), "3"), "0.123 456 78e4");
        assert_eq!(to_s(&s("123456.789"), "3F"), "123 456.789");
        assert_eq!(to_s(&s("Infinity"), ""), "Infinity");
    }

    #[test]
    fn precision_matches_the_oracle_rule() {
        assert_eq!(s("123.45").precision(), 5);
        assert_eq!(s("1e20").precision(), 21);
        assert_eq!(s("1e-20").precision(), 20);
        assert_eq!(s("0.001").precision(), 3);
        assert_eq!(s("100").precision(), 3);
        assert_eq!(s("123.45").scale(), 2);
    }

    // 3.14 is a literal conversion fixture here, not an approximation of pi.
    #[allow(clippy::approx_constant)]
    #[test]
    fn floats_take_the_shortest_form() {
        assert_eq!(from_f64(3.14, 0), s("3.14"));
        assert_eq!(from_f64(1e100, 0), s("1e100"));
        assert_eq!(from_f64(3.14, 3), s("3.14"));
        assert_eq!(from_f64(0.5, 16), s("0.5"));
        assert_eq!(from_f64(-0.0, 0), BD::Zero(-1));
    }

    #[test]
    fn word_prec_follows_cruby_alignment() {
        assert_eq!(s("1.5").word_prec(), 2);
        assert_eq!(s("123456789.123456789").word_prec(), 2);
        assert_eq!(s("0.001").word_prec(), 1);
    }
}

//! The sprintf engine (CRuby sprintf.c) backing `String#%` and, in stage
//! G, `Kernel#format`/`sprintf`/`printf`. Directives: `%s %d %i %f %x %o
//! %b %e %g %c %%`, flags `- + 0 space`, width, precision. `%<name>s`-style
//! hash references and `%*d` star-widths are Tier B.

use crate::{RubyValue, Signal};

#[derive(Default)]
struct Spec {
    minus: bool,
    plus: bool,
    zero: bool,
    space: bool,
    /// The `#` alternate-form flag: a `0x`/`0`/`0b` radix prefix for `x`/`o`/`b`.
    alt: bool,
    width: Option<usize>,
    precision: Option<usize>,
    conv: char,
}

/// A rendered directive split into its `head` (sign and any `#` radix prefix)
/// and `body` (the digits/text). Zero-padding fills BETWEEN the two, so
/// `%#08x` of 255 is `0x0000ff`, not `00000xff`.
struct Rendered {
    head: String,
    body: String,
    /// The digit used when zero-padding to `width` (`'0'` normally; the
    /// sign-extension digit `'1'`/`'7'`/`'f'` for a negative `..`-notation
    /// radix body, where the pad continues the infinite leading sign digits).
    fill: char,
}

impl Default for Rendered {
    fn default() -> Rendered {
        Rendered {
            head: String::new(),
            body: String::new(),
            fill: '0',
        }
    }
}

impl Rendered {
    fn plain(body: String) -> Rendered {
        Rendered {
            body,
            ..Default::default()
        }
    }
}

fn arg_error(msg: String) -> Signal {
    crate::dispatch::raise_error("ArgumentError", msg)
}

/// `%d`-family conversion: CRuby's `rb_Integer` (`"%d" % "12"` parses,
/// `"%d" % "x"` is `invalid value for Integer(): "x"`, `to_int` ducks
/// convert, nil is "can't convert nil into Integer" -- oracle-verified).
fn to_int_for_format(v: &RubyValue) -> Result<num_bigint::BigInt, Signal> {
    match crate::builtins::kernel::integer_impl(std::slice::from_ref(v))? {
        n @ (RubyValue::Int(_) | RubyValue::BigInt(_)) => {
            Ok(crate::builtins::integer::to_bigint(&n))
        }
        _ => unreachable!("integer_impl answers an Integer"),
    }
}

/// `%f`-family conversion: CRuby's `rb_Float` (`"%f" % "x"` is
/// `invalid value for Float(): "x"` -- oracle-verified).
fn to_f64_for_format(v: &RubyValue) -> Result<f64, Signal> {
    match crate::builtins::kernel::float_impl(std::slice::from_ref(v))? {
        RubyValue::Float(f) => Ok(f),
        _ => unreachable!("float_impl answers a Float"),
    }
}

/// Renders one directive into its head/body split (before width padding).
fn render(spec: &Spec, arg: &RubyValue) -> Result<Rendered, Signal> {
    // Non-finite floats print with Ruby's casing (`Inf`/`-Inf`/`NaN`) across
    // every float conversion, where Rust's formatter lowercases (`inf`). NaN
    // carries no sign; ±Infinity does.
    if matches!(spec.conv, 'f' | 'e' | 'E' | 'g' | 'G' | 'a' | 'A') {
        let f = to_f64_for_format(arg)?;
        if !f.is_finite() {
            return Ok(Rendered {
                head: sign_prefix(spec, f.is_sign_negative() && !f.is_nan()),
                body: if f.is_nan() {
                    "NaN".to_string()
                } else {
                    "Inf".to_string()
                },
                ..Default::default()
            });
        }
    }
    Ok(match spec.conv {
        's' => {
            // Fallible: `format("%s", obj)` with a raising `to_s`
            // propagates (catchable, CRuby's rule).
            let mut s = arg.try_display_string()?;
            if let Some(p) = spec.precision {
                s = s.chars().take(p).collect();
            }
            Rendered::plain(s)
        }
        'p' => Rendered::plain(arg.try_inspect_string()?),
        'd' | 'i' | 'u' => {
            let n = to_int_for_format(arg)?;
            let mut body = n.magnitude().to_string();
            if let Some(p) = spec.precision {
                if body.len() < p {
                    body = "0".repeat(p - body.len()) + &body;
                }
            }
            Rendered {
                head: sign_prefix(spec, n.sign() == num_bigint::Sign::Minus),
                body,
                ..Default::default()
            }
        }
        'x' | 'X' | 'o' | 'b' | 'B' => render_radix(spec, to_int_for_format(arg)?),
        'f' => {
            let f = to_f64_for_format(arg)?;
            let prec = spec.precision.unwrap_or(6);
            Rendered {
                head: sign_prefix(spec, f.is_sign_negative() && f != 0.0),
                body: format!("{:.prec$}", f.abs()),
                ..Default::default()
            }
        }
        'e' | 'E' => {
            let f = to_f64_for_format(arg)?;
            let prec = spec.precision.unwrap_or(6);
            let body = format!("{:.prec$e}", f.abs());
            // Rust: "1.234568e4" -- Ruby wants a signed, 2-digit exponent.
            let (mant, exp) = body.split_once('e').expect("{:e} has an exponent");
            let (exp_sign, exp_digits) = match exp.strip_prefix('-') {
                Some(d) => ('-', d),
                None => ('+', exp),
            };
            let e_char = if spec.conv == 'E' { 'E' } else { 'e' };
            Rendered {
                head: sign_prefix(spec, f.is_sign_negative()),
                body: format!("{mant}{e_char}{exp_sign}{:0>2}", exp_digits),
                ..Default::default()
            }
        }
        'g' | 'G' => {
            let f = to_f64_for_format(arg)?;
            let abs = f.abs();
            let e_char = if spec.conv == 'G' { 'E' } else { 'e' };
            let body = if abs != 0.0 && !(1e-4..1e6).contains(&abs) {
                let e = format!("{abs:e}");
                let (mant, exp) = e.split_once('e').expect("exponent");
                let (exp_sign, exp_digits) = match exp.strip_prefix('-') {
                    Some(d) => ('-', d),
                    None => ('+', exp),
                };
                format!("{mant}{e_char}{exp_sign}{:0>2}", exp_digits)
            } else {
                let s = format!("{abs}");
                s.trim_end_matches(".0").to_string()
            };
            Rendered {
                head: sign_prefix(spec, f.is_sign_negative()),
                body,
                ..Default::default()
            }
        }
        // C99 hexadecimal float (`%a`/`%A`): `0x1.5p+2`-style. Rare; a
        // straightforward mantissa/exponent decomposition of the IEEE bits.
        'a' | 'A' => {
            let f = to_f64_for_format(arg)?;
            render_hexfloat(spec, f)
        }
        'c' => Rendered::plain(match arg {
            RubyValue::Str(s) => s
                .lock()
                .chars()
                .next()
                .map(String::from)
                .unwrap_or_default(),
            RubyValue::Int(i) => u32::try_from(*i)
                .ok()
                .and_then(char::from_u32)
                .map(String::from)
                .ok_or_else(|| arg_error(format!("invalid character {i}")))?,
            other => {
                return Err(arg_error(format!(
                    "invalid value for %c: {}",
                    other.inspect_string()
                )));
            }
        }),
        other => return Err(arg_error(format!("malformed format string - %{other}"))),
    })
}

/// `%a`/`%A`: an IEEE-754 double as C99 hex float, e.g. `1.0 -> 0x1p+0`,
/// `0.5 -> 0x1p-1`. Subnormals/zero render as `0x0p+0`.
fn render_hexfloat(spec: &Spec, f: f64) -> Rendered {
    let neg = f.is_sign_negative();
    let head = sign_prefix(spec, neg && f != 0.0);
    let a = f.abs();
    let body = if a == 0.0 {
        "0x0p+0".to_string()
    } else {
        let bits = a.to_bits();
        let exp_field = ((bits >> 52) & 0x7ff) as i64;
        let mantissa = bits & 0xf_ffff_ffff_ffff;
        // Normalized doubles have an implicit leading 1; the exponent is
        // biased by 1023.
        let (lead, unbiased) = if exp_field == 0 {
            (0u64, -1022) // subnormal
        } else {
            (1u64, exp_field - 1023)
        };
        // 13 hex digits of mantissa, trailing zeros trimmed.
        let mut hex = format!("{mantissa:013x}");
        while hex.ends_with('0') {
            hex.pop();
        }
        let frac = if hex.is_empty() {
            String::new()
        } else {
            format!(".{hex}")
        };
        let sign = if unbiased < 0 { "-" } else { "+" };
        let out = format!("0x{lead}{frac}p{sign}{}", unbiased.abs());
        if spec.conv == 'A' {
            out.to_uppercase()
        } else {
            out
        }
    };
    Rendered {
        head,
        body,
        ..Default::default()
    }
}

/// A radix integer conversion (`%x`/`%o`/`%b`/`%B`/`%X`). Positive values are
/// the base-`radix` magnitude, zero-padded to `precision` min-digits. A negative
/// value with a sign flag (`%+b`) is signed-magnitude (`-101`); without one it
/// uses CRuby's infinite-two's-complement `..` notation (`-5` -> `..1011`),
/// where the leading `..` stands for the endless sign digit (`1`/`7`/`f`) and
/// both precision and zero-pad-to-width extend the body with that sign digit.
fn render_radix(spec: &Spec, n: num_bigint::BigInt) -> Rendered {
    use num_bigint::Sign;
    let (radix, prefix, upper) = match spec.conv {
        'x' => (16u32, "0x", false),
        'X' => (16, "0X", true),
        'o' => (8, "0", false),
        'B' => (2, "0B", false),
        _ => (2, "0b", false), // 'b'
    };
    let signed = spec.plus || spec.space;

    if n.sign() != Sign::Minus {
        // Non-negative: plain magnitude, precision as minimum digit count
        // (precision 0 renders zero as the empty string).
        let mut body = radix_digits(&n, radix, upper);
        match spec.precision {
            Some(0) if n.sign() == Sign::NoSign => body.clear(),
            Some(p) if body.len() < p => body = "0".repeat(p - body.len()) + &body,
            _ => {}
        }
        let mut head = sign_prefix(spec, false);
        if spec.alt && n.sign() != Sign::NoSign {
            head.push_str(prefix);
        }
        return Rendered {
            head,
            body,
            fill: '0',
        };
    }

    if signed {
        // Signed magnitude: leading "-" (and "0b" after it under `#`).
        let mut body = radix_digits(&(-&n), radix, upper);
        if let Some(p) = spec.precision {
            if body.len() < p {
                body = "0".repeat(p - body.len()) + &body;
            }
        }
        let mut head = "-".to_string();
        if spec.alt {
            head.push_str(prefix);
        }
        return Rendered {
            head,
            body,
            fill: '0',
        };
    }

    // Infinite two's-complement `..` notation. The `..` and any `#` prefix live
    // in the head so width zero-padding continues the sign digit after them.
    let sign_digit = digit_char(radix - 1, upper);
    let mut body = twos_complement_digits(&n, radix, upper);
    if let Some(p) = spec.precision {
        // Precision counts the leading ".." (two chars): pad the digits with the
        // sign digit so the whole `..`-body reaches `p`.
        let target = p.saturating_sub(2);
        if body.len() < target {
            body = sign_digit.to_string().repeat(target - body.len()) + &body;
        }
    }
    let mut head = String::new();
    if spec.alt {
        head.push_str(prefix);
    }
    head.push_str("..");
    Rendered {
        head,
        body,
        fill: sign_digit,
    }
}

/// The base-`radix` magnitude of a non-negative integer, uppercased for `%X`.
fn radix_digits(n: &num_bigint::BigInt, radix: u32, upper: bool) -> String {
    let s = n.to_str_radix(radix);
    if upper { s.to_uppercase() } else { s }
}

/// A single digit value (0..=15 here) as its character, uppercased when `upper`.
fn digit_char(d: u32, upper: bool) -> char {
    let c = std::char::from_digit(d, 36).unwrap_or('0');
    if upper { c.to_ascii_uppercase() } else { c }
}

/// The minimal infinite-two's-complement digit string for a negative integer:
/// the low base-`radix` digits after which the value stabilizes to all sign
/// digits (`-5` base 2 -> `1011`, `-1` base 16 -> `f`). The caller prefixes `..`.
fn twos_complement_digits(n: &num_bigint::BigInt, radix: u32, upper: bool) -> String {
    use num_bigint::{BigInt, Sign};
    let b = BigInt::from(radix);
    let half = radix / 2;
    let neg_one = BigInt::from(-1);
    let mut q = n.clone();
    let mut digits = Vec::new(); // least-significant first
    loop {
        // Euclidean remainder in [0, radix).
        let mut r = &q % &b;
        if r.sign() == Sign::Minus {
            r += &b;
        }
        let d = num_traits::ToPrimitive::to_u32(&r).unwrap_or(0);
        q = (&q - &r) / &b; // floor division (exact: q - r divisible by b)
        digits.push(d);
        // Stop once the quotient has settled to -1 and the last digit is itself
        // a sign digit, so extending with more sign digits is a no-op.
        if q == neg_one && d >= half {
            break;
        }
    }
    digits.iter().rev().map(|&d| digit_char(d, upper)).collect()
}

fn sign_prefix(spec: &Spec, negative: bool) -> String {
    if negative {
        "-".to_string()
    } else if spec.plus {
        "+".to_string()
    } else if spec.space {
        " ".to_string()
    } else {
        String::new()
    }
}

/// The Hash a template's `%<name>`/`%{name}` references read from -- the first
/// Hash among the arguments (from `str % {..}` or `format(.., k: v)`'s kwargs).
fn named_source(args: &[RubyValue]) -> Option<&crate::RHash> {
    args.iter().find_map(|a| match a {
        RubyValue::Hash(h) => Some(h),
        _ => None,
    })
}

fn named_get(args: &[RubyValue], name: &str) -> Result<RubyValue, Signal> {
    let source = named_source(args).ok_or_else(|| arg_error("one hash required".to_string()))?;
    let key = RubyValue::Symbol(crate::Symbol::intern(name));
    if crate::collections::hash_has_key(source, &key) {
        Ok(crate::collections::hash_get(source, &key))
    } else {
        Err(crate::dispatch::raise_error(
            "KeyError",
            format!("key<{name}> not found"),
        ))
    }
}

/// Reads a `*` width/precision argument (an Integer) from the sequential
/// argument stream.
fn star_int(args: &[RubyValue], next_arg: &mut usize) -> Result<i64, Signal> {
    let v = args
        .get(*next_arg)
        .ok_or_else(|| arg_error("too few arguments".to_string()))?;
    *next_arg += 1;
    match v {
        RubyValue::Int(n) => Ok(*n),
        other => Err(arg_error(format!(
            "invalid width/precision: {}",
            other.inspect_string()
        ))),
    }
}

/// The engine: `sprintf("%05.1f|%<x>d", args)`. Supports flags (`-+ 0#`),
/// width/precision (fixed, `*`-from-arg), positional (`%2$s`) and named
/// (`%<name>d` / `%{name}`) argument references.
pub fn sprintf(template: &str, args: &[RubyValue]) -> Result<String, Signal> {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    let mut next_arg = 0usize;
    'directive: while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut spec = Spec::default();
        let mut named: Option<String> = None;
        let mut arg_index: Option<usize> = None;
        // The flag/width/precision/reference loop -- broken by the conversion
        // char. Order is loose (CRuby's own), except `.precision` after width.
        loop {
            match chars.peek().copied() {
                Some('-') => {
                    spec.minus = true;
                    chars.next();
                }
                Some('+') => {
                    spec.plus = true;
                    chars.next();
                }
                Some(' ') => {
                    spec.space = true;
                    chars.next();
                }
                Some('#') => {
                    spec.alt = true;
                    chars.next();
                }
                Some('0') => {
                    spec.zero = true;
                    chars.next();
                }
                Some('<') => {
                    chars.next();
                    named = Some(read_until(&mut chars, '>')?);
                }
                Some('{') => {
                    // `%{name}` is a complete directive: the value as-is (`%s`).
                    chars.next();
                    let name = read_until(&mut chars, '}')?;
                    out.push_str(&named_get(args, &name)?.try_display_string()?);
                    continue 'directive;
                }
                Some('*') => {
                    chars.next();
                    let n = star_int(args, &mut next_arg)?;
                    if n < 0 {
                        spec.minus = true;
                        spec.width = Some((-n) as usize);
                    } else {
                        spec.width = Some(n as usize);
                    }
                }
                Some('.') => {
                    chars.next();
                    if chars.peek() == Some(&'*') {
                        chars.next();
                        spec.precision = Some(star_int(args, &mut next_arg)?.max(0) as usize);
                    } else {
                        let mut prec = String::new();
                        while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
                            prec.push(chars.next().expect("peeked"));
                        }
                        spec.precision = Some(prec.parse().unwrap_or(0));
                    }
                }
                Some(d) if d.is_ascii_digit() => {
                    let mut num = String::new();
                    while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
                        num.push(chars.next().expect("peeked"));
                    }
                    if chars.peek() == Some(&'$') {
                        chars.next();
                        arg_index = Some(num.parse::<usize>().unwrap_or(0));
                    } else {
                        spec.width = num.parse().ok();
                    }
                }
                _ => break,
            }
        }
        let Some(conv) = chars.next() else {
            return Err(arg_error(
                "incomplete format specifier; use %% (double %) instead".to_string(),
            ));
        };
        if conv == '%' {
            out.push('%');
            continue;
        }
        spec.conv = conv;
        // Pick the argument: a named reference, an explicit `N$` position, or
        // the next sequential argument.
        let arg = if let Some(name) = &named {
            named_get(args, name)?
        } else if let Some(i) = arg_index {
            args.get(i.wrapping_sub(1))
                .cloned()
                .ok_or_else(|| arg_error("too few arguments".to_string()))?
        } else {
            let a = args
                .get(next_arg)
                .cloned()
                .ok_or_else(|| arg_error("too few arguments".to_string()))?;
            next_arg += 1;
            a
        };
        let Rendered { head, body, fill } = render(&spec, &arg)?;
        let visible = head.chars().count() + body.chars().count();
        // An explicit precision disables the `0` flag for integer conversions
        // (CRuby: `%05.3d` is space-padded), where it stays for floats.
        let zero_pad = spec.zero
            && !matches!(spec.conv, 's' | 'p' | 'c')
            && !(spec.precision.is_some()
                && matches!(spec.conv, 'd' | 'i' | 'u' | 'x' | 'X' | 'o' | 'b' | 'B'));
        let padded = match spec.width {
            Some(w) if visible < w => {
                let pad = w - visible;
                if spec.minus {
                    format!("{head}{body}{}", " ".repeat(pad))
                } else if zero_pad {
                    // Pad BETWEEN the head (sign/radix/`..` prefix) and body with
                    // the fill digit (`0`, or the sign digit for `..` notation).
                    format!("{head}{}{body}", fill.to_string().repeat(pad))
                } else {
                    format!("{}{head}{body}", " ".repeat(pad))
                }
            }
            _ => format!("{head}{body}"),
        };
        out.push_str(&padded);
    }
    Ok(out)
}

/// Consumes chars up to (and including) `end`, returning the text between.
fn read_until(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    end: char,
) -> Result<String, Signal> {
    let mut name = String::new();
    for c in chars.by_ref() {
        if c == end {
            return Ok(name);
        }
        name.push(c);
    }
    Err(arg_error(format!(
        "malformed name - unmatched delimiter, expected '{end}'"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> RubyValue {
        RubyValue::Str(crate::string_new(v.to_string()))
    }

    #[test]
    fn the_oracle_directive_matrix() {
        // "%05.1f|%x|%o|%b|%e|%g|%%" % [3.14159, 255, 8, 5, 12345.678, 0.00001]
        let r = sprintf(
            "%05.1f|%x|%o|%b|%e|%g|%%",
            &[
                #[allow(clippy::approx_constant)] // an arbitrary fixture, not pi
                RubyValue::Float(3.14159),
                RubyValue::Int(255),
                RubyValue::Int(8),
                RubyValue::Int(5),
                RubyValue::Float(12345.678),
                RubyValue::Float(0.00001),
            ],
        )
        .unwrap();
        assert_eq!(r, "003.1|ff|10|101|1.234568e+04|1e-05|%");
        // "%-8s|%+d|% d" % ["ab", 5, 7]
        let r = sprintf(
            "%-8s|%+d|% d",
            &[s("ab"), RubyValue::Int(5), RubyValue::Int(7)],
        )
        .unwrap();
        assert_eq!(r, "ab      |+5| 7");
        // "Hello %s, you are %d"
        let r = sprintf("Hello %s, you are %d", &[s("Bob"), RubyValue::Int(42)]).unwrap();
        assert_eq!(r, "Hello Bob, you are 42");
    }

    #[test]
    fn too_few_arguments_is_an_argument_error() {
        let r = std::panic::catch_unwind(|| sprintf("%d %d", &[RubyValue::Int(1)]));
        assert!(r.is_err()); // registry-less: ArgumentError panics
    }

    fn named(pairs: &[(&str, RubyValue)]) -> RubyValue {
        RubyValue::Hash(crate::hash_new(
            pairs
                .iter()
                .map(|(k, v)| (RubyValue::Symbol(crate::Symbol::intern(k)), v.clone()))
                .collect(),
        ))
    }

    #[test]
    fn named_references_angle_and_brace() {
        let h = named(&[("x", RubyValue::Int(42)), ("y", s("hi"))]);
        assert_eq!(
            sprintf("%<x>d and %<y>s", std::slice::from_ref(&h)).unwrap(),
            "42 and hi"
        );
        assert_eq!(sprintf("%{y}!", std::slice::from_ref(&h)).unwrap(), "hi!");
        // Named references carry flags/width/precision.
        assert_eq!(sprintf("%<x>05d", &[h]).unwrap(), "00042");
    }

    #[test]
    fn alternate_form_and_uppercase_radix() {
        assert_eq!(sprintf("%#b", &[RubyValue::Int(10)]).unwrap(), "0b1010");
        assert_eq!(sprintf("%#x", &[RubyValue::Int(255)]).unwrap(), "0xff");
        assert_eq!(sprintf("%#o", &[RubyValue::Int(8)]).unwrap(), "010");
        assert_eq!(sprintf("%X", &[RubyValue::Int(255)]).unwrap(), "FF");
        // The `0x` prefix counts toward width; zero-padding fills after it.
        assert_eq!(
            sprintf("%#08x", &[RubyValue::Int(255)]).unwrap(),
            "0x0000ff"
        );
    }

    #[test]
    fn star_width_and_positional_and_precision() {
        assert_eq!(
            sprintf("%*d", &[RubyValue::Int(5), RubyValue::Int(42)]).unwrap(),
            "   42"
        );
        assert_eq!(
            sprintf("%-*d|", &[RubyValue::Int(5), RubyValue::Int(42)]).unwrap(),
            "42   |"
        );
        assert_eq!(sprintf("%2$s %1$s", &[s("a"), s("b")]).unwrap(), "b a");
        assert_eq!(sprintf("%.3d", &[RubyValue::Int(7)]).unwrap(), "007");
        assert_eq!(
            sprintf("%.*f", &[RubyValue::Int(2), RubyValue::Float(8.7654)]).unwrap(),
            "8.77"
        );
    }

    #[test]
    fn hexadecimal_float() {
        assert_eq!(sprintf("%a", &[RubyValue::Float(1.0)]).unwrap(), "0x1p+0");
        assert_eq!(sprintf("%a", &[RubyValue::Float(0.5)]).unwrap(), "0x1p-1");
        assert_eq!(sprintf("%a", &[RubyValue::Float(0.0)]).unwrap(), "0x0p+0");
    }
}

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
    crate::builtins::arg_error!("{}", msg)
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
            if let Some(p) = spec.precision
                && body.len() < p
            {
                body = "0".repeat(p - body.len()) + &body;
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
                // `-0.0` keeps its sign: `format("%.2f", -0.0)` is "-0.00".
                head: sign_prefix(spec, f.is_sign_negative()),
                body: fixed_body(f.abs(), prec),
                ..Default::default()
            }
        }
        'e' | 'E' => {
            let f = to_f64_for_format(arg)?;
            let prec = spec.precision.unwrap_or(6);
            // Rounded on the shortest decimal, as `%f` is.
            let body = match sci_parts(f, prec) {
                Some((digits, exp)) => match prec {
                    0 => format!("{digits}e{exp}"),
                    _ => format!("{}.{}e{exp}", &digits[..1], &digits[1..]),
                },
                None => format!("{:.prec$e}", f.abs()),
            };
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
            // `%g` shows a fixed number of SIGNIFICANT digits -- six by
            // default -- and picks its presentation from where the decimal
            // point lands, which is why the precision decides `%e` vs `%f`
            // and not just the digit count. CRuby delegates to its bundled
            // BSD `vfprintf`; this is that routine's rule.
            // `.max(1)`: `%.0g` behaves as `%.1g`, because the significant-
            // digit count reaches `dtoa` in a mode that reads a non-positive
            // request as one digit. Oracle-verified across the precision
            // range -- it is not visible in `vfprintf`'s own switch.
            let prec = spec.precision.unwrap_or(6).max(1);
            // Rounding to `prec` significant digits and reading back the
            // base-10 exponent in one step. `expt` is the decimal point's
            // position (value = 0.d1d2... x 10^expt), which is what the
            // BSD switch below is written against.
            let rounded = match sci_parts(abs, prec.saturating_sub(1)) {
                Some((digits, exp)) => match prec {
                    1 => format!("{digits}e{exp}"),
                    _ => format!("{}.{}e{exp}", &digits[..1], &digits[1..]),
                },
                None => format!("{:.*e}", prec.saturating_sub(1), abs),
            };
            let (mant, exp) = rounded.split_once('e').expect("{:e} has an exponent");
            let expt: i32 = exp.parse::<i32>().expect("{:e}'s exponent is an integer") + 1;
            let body = if expt <= -4 || (expt > prec as i32 && expt > 1) {
                let (sign, digits) = match exp.strip_prefix('-') {
                    Some(d) => ('-', d),
                    None => ('+', exp),
                };
                let mant = trim_g(mant, spec.alt);
                format!("{mant}{e_char}{sign}{digits:0>2}")
            } else {
                // Fixed style keeps `prec` significant digits, so the number
                // of DECIMALS depends on where the point sits.
                let decimals = (prec as i32 - expt).max(0) as usize;
                trim_g(&fixed_body(abs, decimals), spec.alt)
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
            // Precision 0 renders zero as the empty string -- unless `#` is
            // asking for octal's leading zero, which IS the whole prefix and
            // so survives (`%#.0o` of 0 is "0").
            Some(0) if n.sign() == Sign::NoSign => {
                body.clear();
                if spec.alt && radix == 8 {
                    body.push('0');
                }
            }
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
        if let Some(p) = spec.precision
            && body.len() < p
        {
            body = "0".repeat(p - body.len()) + &body;
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
    // Octal's `#` prefix IS a leading zero, and `..` notation already begins
    // with the sign digit 7 -- CRuby emits no extra "0" there (`%#o` of -1 is
    // "..7", not "0..7"). The `0x`/`0b` prefixes still apply.
    if spec.alt && radix != 8 {
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

/// `%g`'s trailing-zero rule. Plain `%g` strips them and the bare decimal
/// point with them -- that is what makes it the readable directive. `%#g`
/// keeps both, and keeps the point even where there are no decimals at all
/// (`%#.3g` of 100.0 is `"100."`), which is the whole of what the flag does
/// here.
fn trim_g(digits: &str, alt: bool) -> String {
    if alt {
        return if digits.contains('.') {
            digits.to_string()
        } else {
            format!("{digits}.")
        };
    }
    if !digits.contains('.') {
        return digits.to_string();
    }
    digits
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
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

/// `braces` picks the bracket the KeyError message uses, which ruby takes
/// from the FORM that was written: `%{name}` reports `key{name} not found`
/// and `%<name>s` reports `key<name> not found`.
fn named_get(args: &[RubyValue], name: &str, braces: bool) -> Result<RubyValue, Signal> {
    let source = named_source(args).ok_or_else(|| arg_error("one hash required".to_string()))?;
    let key = RubyValue::Symbol(crate::Symbol::intern(name));
    if crate::collections::hash_has_key(source, &key) {
        Ok(crate::collections::hash_get(source, &key))
    } else {
        let (open, close) = if braces { ('{', '}') } else { ('<', '>') };
        Err(crate::dispatch::raise_error(
            "KeyError",
            format!("key{open}{name}{close} not found"),
        ))
    }
}

/// How a template names its arguments. CRuby lets a template use exactly one
/// of the three (`sprintf.c`'s `CHECK_FOR_WIDTH`/`GETNEXTARG` guards): mixing
/// them leaves no consistent answer for which argument comes next.
#[derive(Clone, Copy, PartialEq)]
enum ArgStyle {
    Sequential,
    Numbered,
    Named,
}

fn note_style(seen: &mut Option<ArgStyle>, now: ArgStyle) -> Result<(), Signal> {
    match seen {
        Some(prev) if *prev != now => Err(arg_error(match (*prev, now) {
            (ArgStyle::Named, _) | (_, ArgStyle::Named) => {
                "named<>after<numbered|unnumbered> is not allowed".to_string()
            }
            _ => "numbered(1) after unnumbered(2)".to_string(),
        })),
        _ => {
            *seen = Some(now);
            Ok(())
        }
    }
}

/// The SHORTEST round-trip decimal of `f` as `(digits, exponent)`, where the
/// value is `0.d1d2... x 10^exponent`. `None` for a value Rust renders in a
/// shape this cannot read (an infinity, a NaN).
///
/// The pair is what lets `%e`/`%g` round the same string `%f` does -- see
/// [`fixed_body`] for why ruby rounds the decimal rather than the bits.
fn shortest_digits(f: f64) -> Option<(Vec<u8>, i32)> {
    if !f.is_finite() || f == 0.0 {
        return None;
    }
    let text = format!("{:e}", f.abs());
    let (mant, exp) = text.split_once('e')?;
    let exp: i32 = exp.parse().ok()?;
    let digits: Vec<u8> = mant.bytes().filter(|b| b.is_ascii_digit()).collect();
    // `{:e}` normalizes to one digit before the point, so the value is
    // `0.<digits> x 10^(exp + 1)`.
    Some((digits, exp + 1))
}

/// `digits` rounded to `sig` significant places, ties to even, with the
/// exponent adjusted when the carry grows a digit (`999` -> `100`, exponent
/// +1). `None` when the shortest form already has `sig` digits or fewer, in
/// which case no rounding happens and the caller keeps the exact expansion.
fn round_significant(digits: &[u8], exp: i32, sig: usize) -> Option<(String, i32)> {
    if digits.len() <= sig {
        return None;
    }
    let mut kept: Vec<u8> = digits[..sig].iter().map(|b| b - b'0').collect();
    let rest = &digits[sig..];
    let round_up = match rest[0] {
        d if d > b'5' => true,
        d if d < b'5' => false,
        _ if rest[1..].iter().any(|b| *b != b'0') => true,
        _ => kept.last().is_some_and(|d| d % 2 == 1),
    };
    let mut exp = exp;
    if round_up {
        let mut i = kept.len();
        loop {
            if i == 0 {
                kept.insert(0, 1);
                kept.pop();
                exp += 1;
                break;
            }
            i -= 1;
            if kept[i] == 9 {
                kept[i] = 0;
            } else {
                kept[i] += 1;
                break;
            }
        }
    }
    Some((kept.iter().map(|d| (d + b'0') as char).collect(), exp))
}

/// `%e`'s mantissa/exponent, rounded ruby's way. `None` falls back to Rust's
/// exact formatting.
fn sci_parts(f: f64, prec: usize) -> Option<(String, i32)> {
    let (digits, exp) = shortest_digits(f)?;
    let (kept, exp) = round_significant(&digits, exp, prec + 1)?;
    Some((kept, exp - 1))
}

/// `%f`'s body -- ruby rounds the SHORTEST round-trip decimal, ties to even,
/// where C's printf rounds the exact binary double. 2.675 is stored as
/// 2.67499999999999982, so printf answers "2.67" and ruby answers "2.68";
/// 2.345 is stored just ABOVE its tie, so printf answers "2.35" and ruby
/// "2.34". Both fall out of rounding the string "2.675" / "2.345".
///
/// Two shapes keep the exact expansion instead:
/// - a precision at or past the shortest digits, where no rounding happens
///   and ruby prints the real bits (`%.20f` of 0.1 is 0.10000000000000000555);
/// - a magnitude below `10^-prec`, where every kept digit is zero and there
///   is no digit to break the tie on (`%.2f` of 0.005 is "0.01", because the
///   stored value sits just above 0.005).
fn fixed_body(f: f64, prec: usize) -> String {
    let exact = format!("{f:.prec$}");
    if !f.is_finite() {
        return exact;
    }
    let short = format!("{f}");
    let Some((int, frac)) = short.split_once('.') else {
        return exact;
    };
    // Rust's `Display` never uses exponent notation for f64, but a value with
    // no fractional digits to spare needs no rounding either way.
    if frac.len() <= prec || short.contains('e') {
        return exact;
    }
    if int == "0" && frac.as_bytes()[..prec].iter().all(|b| *b == b'0') {
        return exact;
    }
    let mut digits: Vec<u8> = int
        .bytes()
        .chain(frac.bytes().take(prec))
        .map(|b| b - b'0')
        .collect();
    let rest = &frac.as_bytes()[prec..];
    let round_up = match rest[0] {
        d if d > b'5' => true,
        d if d < b'5' => false,
        // An exact tie (a lone 5) goes to even; anything after it is a
        // strict excess and rounds up.
        _ if rest[1..].iter().any(|b| *b != b'0') => true,
        _ => digits.last().is_some_and(|d| d % 2 == 1),
    };
    if round_up {
        let mut i = digits.len();
        loop {
            if i == 0 {
                digits.insert(0, 1);
                break;
            }
            i -= 1;
            if digits[i] == 9 {
                digits[i] = 0;
            } else {
                digits[i] += 1;
                break;
            }
        }
    }
    let text: String = digits.iter().map(|d| (d + b'0') as char).collect();
    let point = text.len() - prec;
    match prec {
        0 => text,
        _ => format!("{}.{}", &text[..point], &text[point..]),
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

/// One recorded step of a directive's flag/width/precision/reference
/// loop, replayed IN OCCURRENCE ORDER at render time -- which is what
/// keeps `*` argument consumption, style notes, and overwrite semantics
/// byte-identical to the old single-pass engine.
enum Op {
    Minus,
    Plus,
    Space,
    Alt,
    Zero,
    /// `*`: width from the next sequential argument (negative flips `-`).
    WidthStar,
    /// A digit run that was a width (`None` = the digits overflowed parse).
    WidthFixed(Option<usize>),
    /// `.*`: precision from the next argument (negative = no precision).
    PrecStar,
    /// `.NNN` (empty digits parse as 0).
    PrecFixed(usize),
    /// `%<name>` -- notes the Named style and records the reference.
    NamedRef(String),
    /// `N$` -- notes the Numbered style and records the position.
    ArgIndex(usize),
    /// The bare Named style note a malformed `%<`/`%{` made before its
    /// read failed -- only ever the last op before a [`Piece::Fail`].
    NoteNamed,
}

/// One parsed piece of a template.
enum Piece {
    /// A literal run between directives.
    Text(String),
    /// `%{name}`: the flag ops replay (a `%*{x}` really consumes an
    /// argument), then the value renders as-is.
    NamedInline { ops: Vec<Op>, name: String },
    /// An ordinary `%...X` directive (`conv == '%'` renders a literal `%`
    /// after its ops replay, consuming no argument).
    Directive { ops: Vec<Op>, conv: char },
    /// A malformed tail: its ops replay first (argument consumption and
    /// style notes may raise their own errors, exactly where the
    /// single-pass engine raised them), then `msg` raises. Always the
    /// last piece.
    Fail { ops: Vec<Op>, msg: String },
}

/// A parsed template, ready to render against any argument list --
/// what the frozen-template cache stores.
pub struct Template {
    pieces: Vec<Piece>,
}

/// Parse a template into replayable pieces. Infallible: malformed input
/// becomes a [`Piece::Fail`] raised when RENDER reaches it, so error
/// ORDER against argument errors stays exactly the single-pass engine's.
pub fn parse_template(template: &str) -> Template {
    let mut pieces = Vec::new();
    let mut text = String::new();
    let mut chars = template.chars().peekable();
    'directive: while let Some(c) = chars.next() {
        if c != '%' {
            text.push(c);
            continue;
        }
        if !text.is_empty() {
            pieces.push(Piece::Text(std::mem::take(&mut text)));
        }
        let mut ops: Vec<Op> = Vec::new();
        // The flag/width/precision/reference loop -- broken by the conversion
        // char. Order is loose (CRuby's own), except `.precision` after width.
        loop {
            match chars.peek().copied() {
                Some('-') => {
                    ops.push(Op::Minus);
                    chars.next();
                }
                Some('+') => {
                    ops.push(Op::Plus);
                    chars.next();
                }
                Some(' ') => {
                    ops.push(Op::Space);
                    chars.next();
                }
                Some('#') => {
                    ops.push(Op::Alt);
                    chars.next();
                }
                Some('0') => {
                    ops.push(Op::Zero);
                    chars.next();
                }
                Some('<') => {
                    chars.next();
                    match read_until(&mut chars, '>') {
                        Ok(name) => ops.push(Op::NamedRef(name)),
                        Err(msg) => {
                            ops.push(Op::NoteNamed);
                            pieces.push(Piece::Fail { ops, msg });
                            break 'directive;
                        }
                    }
                }
                Some('{') => {
                    // `%{name}` is a complete directive: the value as-is (`%s`).
                    chars.next();
                    match read_until(&mut chars, '}') {
                        Ok(name) => pieces.push(Piece::NamedInline { ops, name }),
                        Err(msg) => {
                            ops.push(Op::NoteNamed);
                            pieces.push(Piece::Fail { ops, msg });
                            break 'directive;
                        }
                    }
                    continue 'directive;
                }
                Some('*') => {
                    chars.next();
                    ops.push(Op::WidthStar);
                }
                Some('.') => {
                    chars.next();
                    if chars.peek() == Some(&'*') {
                        chars.next();
                        ops.push(Op::PrecStar);
                    } else {
                        let mut prec = String::new();
                        while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
                            prec.push(chars.next().expect("peeked"));
                        }
                        ops.push(Op::PrecFixed(prec.parse().unwrap_or(0)));
                    }
                }
                Some(d) if d.is_ascii_digit() => {
                    let mut num = String::new();
                    while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
                        num.push(chars.next().expect("peeked"));
                    }
                    if chars.peek() == Some(&'$') {
                        chars.next();
                        ops.push(Op::ArgIndex(num.parse::<usize>().unwrap_or(0)));
                    } else {
                        ops.push(Op::WidthFixed(num.parse().ok()));
                    }
                }
                _ => break,
            }
        }
        match chars.next() {
            Some(conv) => pieces.push(Piece::Directive { ops, conv }),
            None => {
                pieces.push(Piece::Fail {
                    ops,
                    msg: "incomplete format specifier; use %% (double %) instead".to_string(),
                });
                break 'directive;
            }
        }
    }
    if !text.is_empty() {
        pieces.push(Piece::Text(text));
    }
    Template { pieces }
}

/// Replay one directive's ops into its [`Spec`]/references, consuming
/// `*` arguments and noting styles exactly where the single-pass engine
/// did.
fn replay_ops<'t>(
    ops: &'t [Op],
    args: &[RubyValue],
    next_arg: &mut usize,
    style: &mut Option<ArgStyle>,
) -> Result<(Spec, Option<&'t str>, Option<usize>), Signal> {
    let mut spec = Spec::default();
    let mut named: Option<&str> = None;
    let mut arg_index: Option<usize> = None;
    for op in ops {
        match op {
            Op::Minus => spec.minus = true,
            Op::Plus => spec.plus = true,
            Op::Space => spec.space = true,
            Op::Alt => spec.alt = true,
            Op::Zero => spec.zero = true,
            Op::WidthStar => {
                let n = star_int(args, next_arg)?;
                if n < 0 {
                    spec.minus = true;
                    spec.width = Some((-n) as usize);
                } else {
                    spec.width = Some(n as usize);
                }
            }
            Op::WidthFixed(w) => spec.width = *w,
            // A NEGATIVE `*` precision is CRuby's "no precision at all"
            // (`sprintf.c`: `if (prec < 0) goto no_precision`), not a
            // precision of zero -- `%.*f` with -2 renders the default six
            // places.
            Op::PrecStar => {
                let n = star_int(args, next_arg)?;
                spec.precision = (n >= 0).then_some(n as usize);
            }
            Op::PrecFixed(p) => spec.precision = Some(*p),
            Op::NamedRef(name) => {
                note_style(style, ArgStyle::Named)?;
                named = Some(name);
            }
            Op::ArgIndex(i) => {
                note_style(style, ArgStyle::Numbered)?;
                arg_index = Some(*i);
            }
            Op::NoteNamed => note_style(style, ArgStyle::Named)?,
        }
    }
    Ok((spec, named, arg_index))
}

/// Render a parsed template against one argument list.
pub fn render_template(t: &Template, args: &[RubyValue]) -> Result<String, Signal> {
    render_template_enc(t, args, &mut Vec::new())
}

/// [`render_template`], also reporting the encoding of every String the
/// template SUBSTITUTES through a `%s`. [`negotiate_encoding`] folds them
/// with the template's own to get the result's encoding.
///
/// Only `%s` contributes. Every other conversion renders text of its own
/// making -- `%d` digits, `%p` an `inspect` -- and CRuby's `inspect` is
/// ASCII-compatible, so neither can move the answer.
pub fn render_template_enc(
    t: &Template,
    args: &[RubyValue],
    used: &mut Vec<crate::RStr>,
) -> Result<String, Signal> {
    let mut out = String::new();
    let mut next_arg = 0usize;
    // CRuby refuses a template that mixes the three ways of naming an
    // argument: sequential (`%s`), numbered (`%1$s`) and named (`%<a>s` /
    // `%{a}`). Mixing them makes the argument stream ambiguous, so the
    // FIRST style a template uses is the only one it may use.
    let mut style: Option<ArgStyle> = None;
    for piece in &t.pieces {
        let (ops, conv) = match piece {
            Piece::Text(s) => {
                out.push_str(s);
                continue;
            }
            Piece::NamedInline { ops, name } => {
                replay_ops(ops, args, &mut next_arg, &mut style)?;
                note_style(&mut style, ArgStyle::Named)?;
                out.push_str(&named_get(args, name, true)?.try_display_string()?);
                continue;
            }
            Piece::Fail { ops, msg } => {
                replay_ops(ops, args, &mut next_arg, &mut style)?;
                return Err(arg_error(msg.clone()));
            }
            Piece::Directive { ops, conv } => (ops, *conv),
        };
        let (mut spec, named, arg_index) = replay_ops(ops, args, &mut next_arg, &mut style)?;
        if conv == '%' {
            out.push('%');
            continue;
        }
        spec.conv = conv;
        // Pick the argument: a named reference, an explicit `N$` position, or
        // the next sequential argument.
        let arg = if let Some(name) = named {
            named_get(args, name, false)?
        } else if let Some(i) = arg_index {
            args.get(i.wrapping_sub(1))
                .cloned()
                .ok_or_else(|| arg_error("too few arguments".to_string()))?
        } else {
            note_style(&mut style, ArgStyle::Sequential)?;
            let a = args
                .get(next_arg)
                .cloned()
                .ok_or_else(|| arg_error("too few arguments".to_string()))?;
            next_arg += 1;
            a
        };
        if let (RubyValue::Str(s), 's') = (&arg, spec.conv) {
            used.push(s.clone());
        }
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

/// The engine: `sprintf("%05.1f|%<x>d", args)`. Supports flags (`-+ 0#`),
/// width/precision (fixed, `*`-from-arg), positional (`%2$s`) and named
/// (`%<name>d` / `%{name}`) argument references.
pub fn sprintf(template: &str, args: &[RubyValue]) -> Result<String, Signal> {
    render_template(&parse_template(template), args)
}

/// The encoding a formatted result carries.
///
/// CRuby's `rb_str_format` starts the answer in the TEMPLATE's encoding and
/// `rb_enc_check`s each substituted String into it, so `"%s" % "café".b` is
/// BINARY and `"%s".force_encoding("ISO-8859-1") % "abc"` stays ISO-8859-1.
/// zeo tagged every result UTF-8, which lost the template's encoding and the
/// argument's alike.
///
/// The rule is `Encoding.compatible?`, folded: an ASCII-only side never moves
/// the answer, two non-ASCII sides in different encodings are a
/// `CompatibilityError`.
pub fn negotiate_encoding(
    template: &crate::RStr,
    used: &[crate::RStr],
) -> Result<crate::encoding::EncodingId, Signal> {
    let (mut enc, mut ascii) = {
        let t = template.lock();
        (t.encoding(), t.ascii_only())
    };
    for s in used {
        let g = s.lock();
        if g.ascii_only() {
            continue;
        }
        if ascii {
            (enc, ascii) = (g.encoding(), false);
            continue;
        }
        if g.encoding() != enc {
            return Err(crate::dispatch::raise_error(
                "Encoding::CompatibilityError",
                format!(
                    "incompatible character encodings: {} and {}",
                    enc.inspect_name(),
                    g.encoding().inspect_name()
                ),
            ));
        }
    }
    Ok(enc)
}

/// [`sprintf_cached`] that also answers the result's encoding.
pub fn sprintf_encoded(template: &crate::RStr, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let mut used = Vec::new();
    let text = sprintf_collect(template, args, &mut used)?;
    let enc = negotiate_encoding(template, &used)?;
    Ok(crate::builtins::string::encode::str_value_in_enc(
        enc, &text,
    ))
}

/// The parsed-template cache behind [`sprintf_cached`]: FROZEN templates
/// keyed by their `Arc` address. The stored strong `RStr` keeps the
/// allocation alive, so the address can never be reused while its entry
/// stands (no ABA); a frozen string's bytes can never change, so the
/// parse stays valid forever.
type Templates = crate::FMap<usize, (crate::RStr, std::sync::Arc<Template>)>;
static TEMPLATES: std::sync::Mutex<Option<Templates>> = std::sync::Mutex::new(None);

/// [`sprintf`] for a caller holding the template as an `RStr`: a frozen
/// template (the common literal `"..." % args` shape under
/// frozen-string-literal, and every interned literal) parses ONCE and
/// renders from the cached pieces -- also skipping the per-call
/// `to_utf8_lossy` copy. A mutable template parses per call, as before.
///
/// Collects the substituted Strings for [`negotiate_encoding`].
fn sprintf_collect(
    template: &crate::RStr,
    args: &[RubyValue],
    used: &mut Vec<crate::RStr>,
) -> Result<String, Signal> {
    if template.is_frozen() {
        let key = std::sync::Arc::as_ptr(template) as usize;
        let cached = TEMPLATES
            .lock()
            .expect("template cache lock")
            .as_ref()
            .and_then(|m| m.get(&key).map(|(_, t)| t.clone()));
        let t = match cached {
            Some(t) => t,
            None => {
                let t = std::sync::Arc::new(parse_template(&template.lock().to_utf8_lossy()));
                TEMPLATES
                    .lock()
                    .expect("template cache lock")
                    .get_or_insert_with(crate::FMap::default)
                    .insert(key, (template.clone(), t.clone()));
                t
            }
        };
        return render_template_enc(&t, args, used);
    }
    let text = template.lock().to_utf8_lossy().into_owned();
    render_template_enc(&parse_template(&text), args, used)
}

/// Consumes chars up to (and including) `end`, returning the text between.
/// The `Err` is the message a [`Piece::Fail`] raises at render time.
fn read_until(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    end: char,
) -> Result<String, String> {
    let mut name = String::new();
    for c in chars.by_ref() {
        if c == end {
            return Ok(name);
        }
        name.push(c);
    }
    Err(format!(
        "malformed name - unmatched delimiter, expected '{end}'"
    ))
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

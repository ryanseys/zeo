//! `Kernel`'s conversion functions -- `Integer()`, `Float()`, `Rational()`,
//! `Complex()`, `String()`, `Array()`, `Hash()` -- and their strict parsers.
//! The `ruby_module!` rows stay in `mod.rs` and call these by bare name.

use super::*;

/// Splits the `exception:` keyword off a Kernel conversion's arguments.
///
/// Every one of `Integer`/`Float`/`Rational`/`Complex` takes it, and it is a
/// KEYWORD -- never one of the value arguments -- so the trailing options Hash
/// comes off before the positional shape is read at all. Read as a positional
/// it became a base, a denominator or an imaginary part, which is how
/// `Integer("abc", exception: false)` used to raise about a Hash.
pub(super) fn split_exception_kw(args: &[RubyValue]) -> (&[RubyValue], bool) {
    let Some(RubyValue::Hash(h)) = args.last() else {
        return (args, true);
    };
    let key = RubyValue::Symbol(crate::Symbol::intern("exception"));
    if !crate::collections::hash_has_key(h, &key) {
        return (args, true);
    }
    (
        &args[..args.len() - 1],
        crate::collections::hash_get(h, &key).truthy(),
    )
}

/// `Kernel#Integer` and its family are ordinary cfuncs in CRuby, which push a
/// control frame -- so a raise from inside one names it
/// (`f.rb:2:in 'Kernel#Integer'`) where a specialized instruction like
/// `opt_ltlt` names only the caller.
///
/// Pushed HERE and not at the dispatch boundary so every route into these
/// conversions names the frame. `synthetic_c_frame`'s exact-repeat
/// dedupe keeps the DISPATCH route to one frame, not two.
pub(super) fn conversion_frame(label: &'static str) -> crate::frames::CFrameGuard {
    crate::frames::synthetic_c_frame(label)
}

/// Runs a Kernel conversion under its `exception:` keyword: `false` answers
/// `nil` instead of raising, which is the entire point of the keyword. Only a
/// RAISE is swallowed -- a `break`/`throw` crossing the conversion still
/// propagates.
pub(super) fn with_exception_kw(
    args: &[RubyValue],
    f: impl Fn(&[RubyValue]) -> Result<RubyValue, Signal>,
) -> Result<RubyValue, Signal> {
    let (positional, raising) = split_exception_kw(args);
    match f(positional) {
        Err(Signal::Raise(_)) if !raising => Ok(RubyValue::Nil),
        other => other,
    }
}

/// `Kernel#Integer(arg, base = nil)` -- CRuby's strict conversion: strings
/// allow surrounding whitespace, single underscores between digits, and
/// radix prefixes (`0x`/`0o`/`0b`, or a leading `0` octal when no base is
/// given); floats/rationals TRUNCATE toward zero; nil and everything else
/// is a TypeError. Message shapes oracle-verified.
pub(crate) fn integer_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let base = match args.get(1) {
        None => None,
        Some(v) => Some(crate::builtins::convert::to_index(v)? as u32),
    };
    // A base only makes sense for a String argument -- CRuby raises rather than
    // silently ignoring it for an Integer/Float/etc.
    if base.is_some() && !matches!(args[0], RubyValue::Str(_)) {
        return Err(arg_error!("base specified for non string value"));
    }
    match &args[0] {
        RubyValue::Int(_) | RubyValue::BigInt(_) => Ok(args[0].clone()),
        RubyValue::Float(f) => {
            if f.is_finite() {
                Ok(crate::builtins::integer::int_value(
                    num_bigint::BigInt::from(f.trunc() as i128),
                ))
            } else {
                Err(crate::builtins::float_domain_error!(
                    "{}",
                    crate::RubyValue::Float(*f).to_display_string()
                ))
            }
        }
        RubyValue::Rational(r) => Ok(crate::builtins::integer::int_value(&r.num / &r.den)),
        RubyValue::Str(s) => {
            let text = s.lock().to_utf8_lossy().into_owned();
            // Base 0 is strtol's "detect from the prefix" -- exactly the
            // no-base rule (0x/0o/0b, leading-0 octal, else decimal).
            let base = base.filter(|&b| b != 0);
            parse_integer_strict(&text, base)
                .ok_or_else(|| arg_error!("invalid value for Integer(): {:?}", text))
        }
        // A `to_int` duck converts (CRuby tries to_int, then to_i); the
        // rest keep Kernel#Integer's own "can't convert" shape.
        other => match crate::builtins::convert::check_to_int(other)? {
            Some(n) => Ok(n),
            None => Err(type_error!(
                "can't convert {} into Integer",
                crate::builtins::convert_name_of(other)
            )),
        },
    }
}

/// The strict string parser `Integer()` and `String#to_i(base)` share:
/// optional whitespace/sign, radix prefix (honored when compatible with an
/// explicit base), single underscores between digits.
pub(crate) fn parse_integer_strict(text: &str, base: Option<u32>) -> Option<RubyValue> {
    let t = text.trim();
    let (negative, t) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let lower = t.to_ascii_lowercase();
    let (radix, digits) = if let Some(rest) = lower.strip_prefix("0x") {
        (16, rest.to_string())
    } else if let Some(rest) = lower.strip_prefix("0o") {
        (8, rest.to_string())
    } else if let Some(rest) = lower.strip_prefix("0b") {
        (2, rest.to_string())
    // `0d` is ruby's EXPLICIT decimal prefix, the fourth of the set. Without
    // it "0d19" fell through to the leading-zero octal rule and then failed on
    // the `d`.
    } else if let Some(rest) = lower.strip_prefix("0d") {
        (10, rest.to_string())
    } else if lower.len() > 1 && lower.starts_with('0') && base.is_none() {
        (8, lower[1..].to_string())
    } else {
        (base.unwrap_or(10), lower)
    };
    if let Some(b) = base
        && b != radix
        && !(b == 10 && radix == 10)
    {
        // An explicit base must agree with an explicit prefix.
        if radix != b {
            return None;
        }
    }
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
    {
        return None;
    }
    let clean: String = digits.chars().filter(|c| *c != '_').collect();
    // Only one sign is allowed, and it was already consumed above -- a residual
    // `+`/`-` (`"++7"`, `"+-7"`) is invalid, though `parse_bytes` would accept
    // a leading `+`.
    if clean.starts_with(['+', '-']) {
        return None;
    }
    let parsed = num_bigint::BigInt::parse_bytes(clean.as_bytes(), radix)?;
    Some(crate::builtins::integer::int_value(if negative {
        -parsed
    } else {
        parsed
    }))
}

/// `Kernel#Float(arg)` -- strict string parse (Rust's `f64::from_str`
/// covers Ruby's accepted forms incl. exponents; underscores stripped),
/// numerics via the tower's f64 view.
pub(crate) fn float_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    match &args[0] {
        RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Float(_) | RubyValue::Rational(_) => {
            Ok(RubyValue::Float(
                crate::builtins::numeric::num_to_f64_unchecked(&args[0]),
            ))
        }
        RubyValue::Str(s) => {
            let text = s.lock().to_utf8_lossy().into_owned();
            let trimmed = text.trim();
            let invalid = || arg_error!("invalid value for Float(): {:?}", text);
            // Underscores are only legal BETWEEN two digits (hex digits for a
            // `0x` float): a leading/trailing/doubled `_`, or one adjacent to
            // `.`/`e`/`p`/a sign, is rejected (`"1__0"`, `"1_"`, `"1_e3"`).
            let Some(clean) = strip_valid_underscores(trimmed) else {
                return Err(invalid());
            };
            // Rust's parser accepts the words "inf"/"infinity"/"nan"; CRuby's
            // Float() does not (only numeric literals). A valid numeric string
            // never contains those substrings.
            let lower = clean.to_ascii_lowercase();
            if lower.contains("inf") || lower.contains("nan") {
                return Err(invalid());
            }
            clean
                .parse::<f64>()
                .ok()
                // C99 hex-float (`"0x1p4"` = 16.0), which `str::parse` rejects.
                .or_else(|| parse_hex_float(&clean))
                .map(RubyValue::Float)
                .ok_or_else(invalid)
        }
        // Anything else goes through the `to_f` protocol, CRuby's
        // `rb_convert_type_with_id(val, T_FLOAT, "Float", idTo_f)`. This is what
        // lets `Float(obj)` and every `Numeric` default built on it answer for a
        // user `class Temp < Numeric` that defines only `to_f`. `nil` has no
        // `to_f` for this purpose -- CRuby rejects it before asking.
        other => {
            let to_f = crate::Symbol::intern("to_f");
            let refuse = || {
                type_error!(
                    "can't convert {} into Float",
                    crate::builtins::convert_name_of(other)
                )
            };
            if matches!(other, RubyValue::Nil)
                || !crate::dispatch::responds_to(other.class_id(), to_f, false)
            {
                return Err(refuse());
            }
            match crate::dispatch::send_value(other, to_f, &[], None)? {
                f @ RubyValue::Float(_) => Ok(f),
                // A `to_f` that answers something else is a broken conversion,
                // not a Float -- CRuby reports the same refusal.
                _ => Err(refuse()),
            }
        }
    }
}

/// Validates that every `_` in a `Float()` string sits between two digits and
/// returns the string with the underscores removed; `None` if any is misplaced.
/// A `0x`-prefixed value uses hex-digit adjacency (so `0x1_1` is fine) while a
/// decimal value uses `0-9` (so the `e` in `1_e3` doesn't count as a digit).
pub(super) fn strip_valid_underscores(s: &str) -> Option<String> {
    let body = s.strip_prefix(['+', '-']).unwrap_or(s);
    let is_hex = body.starts_with("0x") || body.starts_with("0X");
    let is_digit = |c: u8| {
        if is_hex {
            c.is_ascii_hexdigit()
        } else {
            c.is_ascii_digit()
        }
    };
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'_' {
            let prev_ok = i > 0 && is_digit(bytes[i - 1]);
            let next_ok = i + 1 < bytes.len() && is_digit(bytes[i + 1]);
            if !(prev_ok && next_ok) {
                return None;
            }
        }
    }
    Some(s.chars().filter(|c| *c != '_').collect())
}

/// Parse a C99 hexadecimal float (`[±]0x<hex>.<hex>p<dec-exp>`, the exponent a
/// power of TWO), which `str::parse::<f64>` rejects: `"0x1p4"` -> 16.0,
/// `"0x1.8p1"` -> 3.0. `None` if the string isn't this shape.
pub(super) fn parse_hex_float(s: &str) -> Option<f64> {
    let t = s.trim();
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X"))?;
    // The binary exponent `p<dec>` is optional: `"0xa"` is 10.0 (exponent 0).
    let (mantissa, exp): (&str, i32) = match t.find(['p', 'P']) {
        Some(idx) => (&t[..idx], t[idx + 1..].parse().ok()?),
        None => (t, 0),
    };
    let (int_str, frac_str) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if int_str.is_empty() && frac_str.is_empty() {
        return None;
    }
    let mut value = 0.0f64;
    for c in int_str.chars() {
        value = value * 16.0 + c.to_digit(16)? as f64;
    }
    let mut scale = 1.0 / 16.0;
    for c in frac_str.chars() {
        value += c.to_digit(16)? as f64 * scale;
        scale /= 16.0;
    }
    let result = value * 2f64.powi(exp);
    Some(if neg { -result } else { result })
}

/// `Kernel#Rational(num, den = 1)` -- exact components only (string forms
/// are a documented scope-cut).
pub(crate) fn rational_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let exact = |v: &RubyValue| -> Result<(num_bigint::BigInt, num_bigint::BigInt), Signal> {
        match v {
            RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Rational(_) => {
                Ok(crate::builtins::rational::as_ratio(v))
            }
            // A Float contributes its EXACT dyadic value (`Rational(0.3)` is
            // the true `5404.../18014...`, not `3/10`).
            RubyValue::Float(f) => Ok(crate::builtins::float::float_exact_parts(*f)),
            // A String is PARSED as a rational literal (`"3/4"`, `"-5/2"`,
            // `"2.5"`, `"6"`) -- its DECIMAL value, not its Float value, so
            // `"2.5"` is exactly `5/2`.
            RubyValue::Str(s) => parse_rational_string(&s.lock().to_utf8_lossy()),
            other => Err(type_error!(
                "can't convert {} into Rational",
                crate::builtins::convert_name_of(other)
            )),
        }
    };
    let (nn, nd) = exact(&args[0])?;
    let (dn, dd) = match args.get(1) {
        Some(d) => exact(d)?,
        None => (num_bigint::BigInt::from(1), num_bigint::BigInt::from(1)),
    };
    // (nn/nd) / (dn/dd) == (nn*dd) / (nd*dn)
    crate::builtins::rational::rational_new(nn * dd, nd * dn)
}

/// `Kernel#Complex(real, imag = 0)`. A single String argument is parsed as a
/// complex literal (`"2+3i"`, `"3"`, `"-i"`).
pub(crate) fn complex_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    use crate::builtins::complex;

    // A `nil` in either position is refused up front, before either
    // component is examined, so it reports the conversion rather than
    // `Complex.rect`'s "not a real" (CRuby `nucomp_convert`).
    if matches!(args[0], RubyValue::Nil) || matches!(args.get(1), Some(RubyValue::Nil)) {
        return Err(type_error!("can't convert nil into Complex"));
    }
    // A String component is parsed, in EITHER position.
    let parse = |v: &RubyValue| -> Result<RubyValue, Signal> {
        match v {
            RubyValue::Str(s) => {
                let (real, imag) = parse_complex_string(&s.lock().to_utf8_lossy())?;
                complex::complex_new(real, imag)
            }
            other => Ok(other.clone()),
        }
    };
    // A real-valued Complex contributes its own real part -- so
    // `Complex(Complex(3, 0), 4)` is `(3+4i)`, not a nested component.
    let unwrap_real = |v: RubyValue| match &v {
        RubyValue::Complex(c) if complex::is_exact_zero(&c.imag) => c.real.clone(),
        _ => v,
    };
    let a1 = unwrap_real(parse(&args[0])?);
    let a2 = match args.get(1) {
        Some(v) => Some(unwrap_real(parse(v)?)),
        None => None,
    };

    // A Complex that survived the unwrap passes through whole, provided the
    // imaginary argument would contribute nothing.
    if matches!(a1, RubyValue::Complex(_)) && a2.as_ref().is_none_or(complex::is_exact_zero) {
        return Ok(a1);
    }
    let Some(a2) = a2 else {
        // One argument: a NON-real numeric is already the answer, and a
        // non-numeric converts through `#to_c`.
        return match complex::is_real_numeric(&a1)? {
            Some(false) => Ok(a1),
            Some(true) => complex::complex_new(a1, RubyValue::Int(0)),
            None if crate::dispatch::responds_to_value(&a1, Symbol::intern("to_c"), false) => {
                crate::dispatch::send_value(&a1, Symbol::intern("to_c"), &[], None)
            }
            None => Err(type_error!(
                "can't convert {} into Complex",
                crate::builtins::convert_name_of(&a1)
            )),
        };
    };
    // Two arguments, either of them non-real: the pair means `a1 + a2*i`,
    // which is arithmetic on the components, not a rectangular build.
    if matches!(complex::is_real_numeric(&a1)?, Some(r1) if !r1)
        || matches!(complex::is_real_numeric(&a2)?, Some(r2) if !r2)
    {
        let unit = complex::complex_new(RubyValue::Int(0), RubyValue::Int(1))?;
        let scaled = crate::dispatch::send_value(&a2, Symbol::intern("*"), &[unit], None)?;
        return crate::dispatch::send_value(&a1, Symbol::intern("+"), &[scaled], None);
    }
    complex::complex_new(complex::real_check(&a1)?, complex::real_check(&a2)?)
}

/// The `ArgumentError` CRuby's numeric-string converters raise on an
/// unparseable value: `invalid value for convert(): "<original>"`.
pub(super) fn convert_error(original: &str) -> Signal {
    arg_error!("invalid value for convert(): {original:?}")
}

/// Parse a rational literal string to its `(numerator, denominator)` DECIMAL
/// value: `"3/4"` -> `(3, 4)`, `"2.5"` -> `(25, 10)` (exactly `5/2`, not the
/// Float value), `"6"` -> `(6, 1)`. Leading/trailing whitespace and a sign are
/// allowed. A `"n/0"` denominator is ZeroDivisionError, like the numeric form.
pub(super) fn parse_rational_string(
    s: &str,
) -> Result<(num_bigint::BigInt, num_bigint::BigInt), Signal> {
    use num_bigint::BigInt;
    let t = s.trim();
    // A scientific EXPONENT scales the mantissa exactly -- `Rational("1.5e2")`
    // is `(150/1)`, not the float 1.5 times 100. Peeled first so the mantissa
    // reaches the decimal branch below unchanged; a `/` form has no exponent.
    if !t.contains('/')
        && let Some(at) = t.rfind(['e', 'E'])
        && at > 0
    {
        let (mantissa, exp) = t.split_at(at);
        let exp: i32 = exp[1..].parse().map_err(|_| convert_error(s))?;
        let (mut num, mut den) = parse_rational_string(mantissa)?;
        let scale = BigInt::from(10).pow(exp.unsigned_abs());
        if exp >= 0 {
            num *= scale;
        } else {
            den *= scale;
        }
        return Ok((num, den));
    }
    if let Some((n, d)) = t.split_once('/') {
        let num: BigInt = n.trim().parse().map_err(|_| convert_error(s))?;
        let den: BigInt = d.trim().parse().map_err(|_| convert_error(s))?;
        if den == BigInt::from(0) {
            return Err(crate::builtins::zero_division_error!("divided by 0"));
        }
        Ok((num, den))
    } else if let Some((int_part, frac_part)) = t.split_once('.') {
        let neg = int_part.trim_start().starts_with('-');
        let int_digits: String = int_part.chars().filter(char::is_ascii_digit).collect();
        let frac_digits: String = frac_part.chars().filter(char::is_ascii_digit).collect();
        if int_digits.is_empty() && frac_digits.is_empty() {
            return Err(convert_error(s));
        }
        let mut num: BigInt = format!("{int_digits}{frac_digits}")
            .parse()
            .map_err(|_| convert_error(s))?;
        if neg {
            num = -num;
        }
        Ok((num, BigInt::from(10).pow(frac_digits.len() as u32)))
    } else {
        Ok((t.parse().map_err(|_| convert_error(s))?, BigInt::from(1)))
    }
}

/// Parse a complex literal string to `(real, imag)` values: `"2+3i"`,
/// `"1+2i"`, `"3"` (-> `(3, 0)`), `"-i"` (-> `(0, -1)`), `"4i"` (-> `(0, 4)`).
/// Each component is an Integer when it has no decimal point, else a Float.
pub(super) fn parse_complex_string(s: &str) -> Result<(RubyValue, RubyValue), Signal> {
    let t = s.trim();
    let num = |part: &str| -> Result<RubyValue, Signal> {
        if part.contains('.') {
            part.parse::<f64>()
                .map(RubyValue::Float)
                .map_err(|_| convert_error(s))
        } else {
            part.parse::<i64>()
                .map(RubyValue::Int)
                .map_err(|_| convert_error(s))
        }
    };
    // The imaginary coefficient: an empty/sign-only string is the unit `±1`.
    let imag = |part: &str| -> Result<RubyValue, Signal> {
        match part {
            "" | "+" => Ok(RubyValue::Int(1)),
            "-" => Ok(RubyValue::Int(-1)),
            other => num(other),
        }
    };
    let Some(body) = t.strip_suffix('i').or_else(|| t.strip_suffix('I')) else {
        // No imaginary unit -> a pure real value.
        return Ok((num(t)?, RubyValue::Int(0)));
    };
    // Split real+imag at the sign joining them (not a leading sign, and not an
    // exponent sign after `e`/`E`).
    let split = body.char_indices().rev().find(|&(idx, c)| {
        (c == '+' || c == '-')
            && idx != 0
            && !matches!(body.as_bytes().get(idx - 1), Some(b'e' | b'E'))
    });
    match split {
        Some((idx, _)) => Ok((num(&body[..idx])?, imag(&body[idx..])?)),
        None => Ok((RubyValue::Int(0), imag(body)?)),
    }
}

/// `Kernel#String(arg)` -- `to_s` (the `to_str`-first nuance is invisible
/// for builtin receivers).
pub(crate) fn string_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    // `to_str` FIRST, then `to_s` -- `rb_f_string` tries the strict conversion
    // and only falls back to the display form, so an object defining `to_str`
    // is converted by it rather than stringified.
    if let Some(s) = crate::builtins::convert::check_to_str(&args[0])? {
        return Ok(s);
    }
    Ok(RubyValue::Str(crate::string_new(
        args[0].to_display_string(),
    )))
}

/// `Kernel#Array(arg)`: nil -> [], Array -> itself, Hash -> assoc pairs,
/// Range -> to_a, anything else -> [arg]. (`to_ary`/`to_a` protocol probes
/// on user objects are a documented scope-cut, beyond the value-subclass
/// case below, which IS an Array.)
pub(crate) fn array_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    if let Some(a) = crate::builtins::convert::check_to_ary(&args[0])? {
        return Ok(a);
    }
    // `to_ary` FIRST, then `to_a` -- `rb_Array` tries both, in that order, so
    // an object defining only `to_a` still converts.
    let to_a = crate::Symbol::intern("to_a");
    if crate::dispatch::responds_to_value(&args[0], to_a, false)
        && let RubyValue::Array(_) = crate::dispatch::send_value(&args[0], to_a, &[], None)?
    {
        return crate::dispatch::send_value(&args[0], to_a, &[], None);
    }
    Ok(match &args[0] {
        RubyValue::Nil => RubyValue::Array(crate::array_new(Vec::new())),
        RubyValue::Array(_) => args[0].clone(),
        RubyValue::Hash(h) => RubyValue::Array(crate::array_new(
            h.lock()
                .values()
                .map(|(k, v)| RubyValue::Array(crate::array_new(vec![k.clone(), v.clone()])))
                .collect(),
        )),
        RubyValue::Range(..) => {
            crate::builtins::enumerable::enumerable_send(&args[0], "to_a", &[], None)
                .expect("Enumerable implements to_a")?
        }
        other => RubyValue::Array(crate::array_new(vec![other.clone()])),
    })
}

/// `Kernel#Hash(arg)`: nil/[] -> {}, Hash -> itself, else TypeError.
pub(crate) fn hash_impl(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    match &args[0] {
        RubyValue::Nil => Ok(RubyValue::Hash(crate::hash_new(Vec::new()))),
        RubyValue::Array(a) if a.lock().is_empty() => {
            Ok(RubyValue::Hash(crate::hash_new(Vec::new())))
        }
        RubyValue::Hash(_) => Ok(args[0].clone()),
        // `to_hash` converts, as `rb_Hash` does; only a value with none is the
        // TypeError.
        other => match crate::builtins::convert::check_to_hash(other)? {
            Some(h) => Ok(h),
            None => Err(type_error!(
                "can't convert {} into Hash",
                crate::builtins::convert_name_of(other)
            )),
        },
    }
}

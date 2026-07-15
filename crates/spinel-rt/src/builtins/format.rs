//! The sprintf engine (CRuby sprintf.c) backing `String#%` and, in stage
//! G, `Kernel#format`/`sprintf`/`printf`. Directives: `%s %d %i %f %x %o
//! %b %e %g %c %%`, flags `- + 0 space`, width, precision. `%<name>s`-style
//! hash references and `%*d` star-widths are Tier B.

use crate::{RubyValue, Signal};

struct Spec {
    minus: bool,
    plus: bool,
    zero: bool,
    space: bool,
    width: Option<usize>,
    precision: Option<usize>,
    conv: char,
}

fn arg_error(msg: String) -> Signal {
    crate::dispatch::raise_error("ArgumentError", msg)
}

fn to_int_for_format(v: &RubyValue) -> Result<num_bigint::BigInt, Signal> {
    match v {
        RubyValue::Int(_) | RubyValue::BigInt(_) => Ok(crate::builtins::integer::to_bigint(v)),
        RubyValue::Float(f) => Ok(num_bigint::BigInt::from(f.trunc() as i128)),
        RubyValue::Rational(r) => Ok(&r.num / &r.den),
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "can't convert {} into Integer",
                crate::builtins::class_name_of(other)
            ),
        )),
    }
}

fn to_f64_for_format(v: &RubyValue) -> Result<f64, Signal> {
    match v {
        RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Float(_) | RubyValue::Rational(_) => {
            Ok(crate::builtins::numeric::num_to_f64_unchecked(v))
        }
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "can't convert {} into Float",
                crate::builtins::class_name_of(other)
            ),
        )),
    }
}

/// Renders one directive's body (before width padding).
fn render(spec: &Spec, arg: &RubyValue) -> Result<String, Signal> {
    Ok(match spec.conv {
        's' => {
            let mut s = arg.to_display_string();
            if let Some(p) = spec.precision {
                s = s.chars().take(p).collect();
            }
            s
        }
        'p' => arg.inspect_string(),
        'd' | 'i' => {
            let n = to_int_for_format(arg)?;
            let body = n.magnitude().to_string();
            sign_prefix(spec, n.sign() == num_bigint::Sign::Minus) + &body
        }
        'x' | 'o' | 'b' => {
            let n = to_int_for_format(arg)?;
            let radix = match spec.conv {
                'x' => 16,
                'o' => 8,
                _ => 2,
            };
            let body = n.magnitude().to_str_radix(radix);
            sign_prefix(spec, n.sign() == num_bigint::Sign::Minus) + &body
        }
        'f' => {
            let f = to_f64_for_format(arg)?;
            let prec = spec.precision.unwrap_or(6);
            let body = format!("{:.prec$}", f.abs());
            sign_prefix(spec, f.is_sign_negative() && f != 0.0) + &body
        }
        'e' => {
            let f = to_f64_for_format(arg)?;
            let prec = spec.precision.unwrap_or(6);
            let body = format!("{:.prec$e}", f.abs());
            // Rust: "1.234568e4" -- Ruby wants a signed, 2-digit exponent.
            let (mant, exp) = body.split_once('e').expect("{:e} has an exponent");
            let (exp_sign, exp_digits) = match exp.strip_prefix('-') {
                Some(d) => ('-', d),
                None => ('+', exp),
            };
            sign_prefix(spec, f.is_sign_negative())
                + &format!("{mant}e{exp_sign}{:0>2}", exp_digits)
        }
        'g' => {
            let f = to_f64_for_format(arg)?;
            // C's %g: shortest of %e/%f with default precision 6,
            // trailing zeros trimmed. Approximated via Rust's shortest
            // repr with the same e-threshold rules.
            let abs = f.abs();
            let body = if abs != 0.0 && !(1e-4..1e6).contains(&abs) {
                let e = format!("{abs:e}");
                let (mant, exp) = e.split_once('e').expect("exponent");
                let (exp_sign, exp_digits) = match exp.strip_prefix('-') {
                    Some(d) => ('-', d),
                    None => ('+', exp),
                };
                format!("{mant}e{exp_sign}{:0>2}", exp_digits)
            } else {
                let s = format!("{abs}");
                s.trim_end_matches(".0").to_string()
            };
            sign_prefix(spec, f.is_sign_negative()) + &body
        }
        'c' => match arg {
            RubyValue::Str(s) => s.lock().chars().next().map(String::from).unwrap_or_default(),
            RubyValue::Int(i) => u32::try_from(*i)
                .ok()
                .and_then(char::from_u32)
                .map(String::from)
                .ok_or_else(|| arg_error(format!("invalid character {i}")))?,
            other => {
                return Err(arg_error(format!(
                    "invalid value for %c: {}",
                    other.inspect_string()
                )))
            }
        },
        other => return Err(arg_error(format!("malformed format string - %{other}"))),
    })
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

/// The engine: `sprintf("%05.1f|%x", args)`.
pub fn sprintf(template: &str, args: &[RubyValue]) -> Result<String, Signal> {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    let mut next_arg = 0usize;
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut spec = Spec {
            minus: false,
            plus: false,
            zero: false,
            space: false,
            width: None,
            precision: None,
            conv: '%',
        };
        // Flags.
        loop {
            match chars.peek() {
                Some('-') => spec.minus = true,
                Some('+') => spec.plus = true,
                Some('0') => spec.zero = true,
                Some(' ') => spec.space = true,
                _ => break,
            }
            chars.next();
        }
        // Width.
        let mut width = String::new();
        while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
            width.push(chars.next().expect("peeked"));
        }
        if !width.is_empty() {
            spec.width = width.parse().ok();
        }
        // Precision.
        if chars.peek() == Some(&'.') {
            chars.next();
            let mut prec = String::new();
            while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
                prec.push(chars.next().expect("peeked"));
            }
            spec.precision = Some(prec.parse().unwrap_or(0));
        }
        let Some(conv) = chars.next() else {
            return Err(arg_error("incomplete format specifier; use %% (double %) instead".to_string()));
        };
        if conv == '%' {
            out.push('%');
            continue;
        }
        spec.conv = conv;
        let Some(arg) = args.get(next_arg) else {
            return Err(arg_error("too few arguments".to_string()));
        };
        next_arg += 1;
        let body = render(&spec, arg)?;
        // Width padding: `-` left-justifies; `0` zero-pads numerics AFTER
        // any sign.
        let padded = match spec.width {
            Some(w) if body.chars().count() < w => {
                let pad = w - body.chars().count();
                if spec.minus {
                    format!("{body}{}", " ".repeat(pad))
                } else if spec.zero && spec.conv != 's' && spec.conv != 'p' {
                    let (sign, digits) = match body.strip_prefix(['-', '+', ' ']) {
                        Some(rest) => (body[..1].to_string(), rest.to_string()),
                        None => (String::new(), body),
                    };
                    format!("{sign}{}{digits}", "0".repeat(pad))
                } else {
                    format!("{}{body}", " ".repeat(pad))
                }
            }
            _ => body,
        };
        out.push_str(&padded);
    }
    Ok(out)
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
        let r = sprintf("%-8s|%+d|% d", &[s("ab"), RubyValue::Int(5), RubyValue::Int(7)]).unwrap();
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
}

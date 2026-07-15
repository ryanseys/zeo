//! `Kernel` -- the module every "universal" method actually belongs to
//! (CRuby's `Object` owns ZERO instance methods; object.c defines these on
//! `rb_mKernel`). Reached on every receiver through the MRO walk, since
//! every chain ends `..., Object, Kernel, BasicObject`.
//!
//! Rows migrated from `send`/`send_value`'s old hardwired universal arms:
//! `class`, `dup`/`clone`, `hash`, `to_s`/`inspect`, `is_a?`/`kind_of?`,
//! `instance_of?` -- plus the CRuby-owned additions `nil?`, `itself`,
//! `frozen?`/`freeze`, `eql?`, `===`, `respond_to?`, `tap`, `then`.
//! `Kernel#<=>` (identity-or-nil default) is deliberately ABSENT until the
//! numeric operator rows move into `integer.rs`/`float.rs` (stage C) -- it
//! would shadow the post-walk numeric `<=>` today.

use crate::builtins::{arity, block_or_enum, builtin_methods, need_block};
use crate::{RubyValue, Signal, Symbol};

builtin_methods! {
    pub(crate) fn lookup;

    "class" => fn class(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Class(recv.class_id()))
    }
    "nil?" => fn nil_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv.is_nil()))
    }
    "itself" => fn itself(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    "dup" => fn dup(recv, args, _block) {
        arity!(args, 0);
        Ok(match recv {
            RubyValue::Object(o) => RubyValue::Object(o.dup_object(false)),
            _ => recv.dup_value(false),
        })
    }
    "clone" => fn clone_m(recv, args, _block) {
        arity!(args, 0);
        Ok(match recv {
            RubyValue::Object(o) => RubyValue::Object(o.dup_object(true)),
            _ => recv.dup_value(true),
        })
    }
    "frozen?" => fn frozen_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv.is_frozen()))
    }
    "freeze" => fn freeze(recv, args, _block) {
        arity!(args, 0);
        recv.freeze_value();
        Ok(recv.clone())
    }
    "hash" => fn hash(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(crate::value_hash_code(recv)))
    }
    "to_s" => fn to_s(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_new(recv.to_display_string())))
    }
    "inspect" => fn inspect(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_new(recv.inspect_string())))
    }
    // Kernel's default `===` is `==` (case subjects fall back to equality).
    "===" => fn case_eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.rb_eq(&args[0])))
    }
    // `eql?`: same class AND `==` (what makes `1.eql?(1.0)` false while
    // `1 == 1.0` is true -- oracle-verified).
    "eql?" => fn eql_p(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(
            recv.class_id() == args[0].class_id() && recv.rb_eq(&args[0]),
        ))
    }
    "is_a?" | "kind_of?" => fn is_a_p(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Class(target) = &args[0] else {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                "class or module required".to_string(),
            ));
        };
        Ok(RubyValue::Bool(crate::dispatch::is_a(recv.class_id(), *target)))
    }
    "instance_of?" => fn instance_of_p(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Class(target) = &args[0] else {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                "class or module required".to_string(),
            ));
        };
        Ok(RubyValue::Bool(recv.class_id() == *target))
    }
    "respond_to?" => fn respond_to_p(recv, args, _block) {
        arity!(args, 1);
        let sym = match &args[0] {
            RubyValue::Symbol(s) => *s,
            RubyValue::Str(s) => Symbol::intern(&s.lock()),
            other => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!("{} is not a symbol nor a string", other.inspect_string()),
                ))
            }
        };
        Ok(RubyValue::Bool(crate::dispatch::responds_to(recv.class_id(), sym)))
    }
    "tap" => fn tap(recv, args, block) {
        arity!(args, 0);
        let p = need_block!(block);
        p(std::slice::from_ref(recv))?;
        Ok(recv.clone())
    }
    "then" | "yield_self" => fn then_m(recv, args, block) {
        arity!(args, 0);
        let p = block_or_enum!(recv, "then", args, block);
        p(std::slice::from_ref(recv))
    }
    // `x.to_enum(:meth, *args)` -- captures exactly (receiver, method,
    // args), CRuby's obj_to_enum (Phase 17.2). The block-as-size-proc
    // form is Tier B (rare; the stored-size Enumerator.new form covers
    // the practical cases).
    "to_enum" | "enum_for" => fn to_enum(recv, args, _block) {
        let meth = match args.first() {
            None => "each".to_string(),
            Some(RubyValue::Symbol(s)) => s.name().as_str().to_string(),
            Some(RubyValue::Str(s)) => s.lock().clone(),
            Some(other) => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!("{} is not a symbol nor a string", other.inspect_string()),
                ))
            }
        };
        let rest = if args.is_empty() { &[] } else { &args[1..] };
        Ok(crate::builtins::enumerator::enumerator_for(recv, &meth, rest))
    }
}



/// `Kernel#Integer(arg, base = nil)` -- CRuby's strict conversion: strings
/// allow surrounding whitespace, single underscores between digits, and
/// radix prefixes (`0x`/`0o`/`0b`, or a leading `0` octal when no base is
/// given); floats/rationals TRUNCATE toward zero; nil and everything else
/// is a TypeError. Message shapes oracle-verified.
pub fn kernel_integer(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1..=2);
    let base = match args.get(1) {
        Some(RubyValue::Int(b)) => Some(*b as u32),
        Some(other) => {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Integer",
                    crate::builtins::class_name_of(other)
                ),
            ))
        }
        None => None,
    };
    match &args[0] {
        RubyValue::Int(_) | RubyValue::BigInt(_) => Ok(args[0].clone()),
        RubyValue::Float(f) => {
            if f.is_finite() {
                Ok(crate::builtins::integer::int_value(
                    num_bigint::BigInt::from(f.trunc() as i128),
                ))
            } else {
                Err(crate::dispatch::raise_error(
                    "FloatDomainError",
                    crate::RubyValue::Float(*f).to_display_string(),
                ))
            }
        }
        RubyValue::Rational(r) => Ok(crate::builtins::integer::int_value(&r.num / &r.den)),
        RubyValue::Str(s) => {
            let text = s.lock().clone();
            parse_integer_strict(&text, base).ok_or_else(|| {
                crate::dispatch::raise_error(
                    "ArgumentError",
                    format!("invalid value for Integer(): {:?}", text),
                )
            })
        }
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "can't convert {} into Integer",
                if matches!(other, RubyValue::Nil) {
                    "nil".to_string()
                } else {
                    crate::builtins::class_name_of(other)
                }
            ),
        )),
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
    } else if lower.len() > 1 && lower.starts_with('0') && base.is_none() {
        (8, lower[1..].to_string())
    } else {
        (base.unwrap_or(10), lower)
    };
    if let Some(b) = base {
        if b != radix && !(b == 10 && radix == 10) {
            // An explicit base must agree with an explicit prefix.
            if radix != b {
                return None;
            }
        }
    }
    if digits.is_empty() || digits.starts_with('_') || digits.ends_with('_') || digits.contains("__")
    {
        return None;
    }
    let clean: String = digits.chars().filter(|c| *c != '_').collect();
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
pub fn kernel_float(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1);
    match &args[0] {
        RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Float(_) | RubyValue::Rational(_) => {
            Ok(RubyValue::Float(crate::builtins::numeric::num_to_f64_unchecked(&args[0])))
        }
        RubyValue::Str(s) => {
            let text = s.lock().clone();
            let clean: String = text.trim().chars().filter(|c| *c != '_').collect();
            clean
                .parse::<f64>()
                .ok()
                .filter(|f| f.is_finite() || clean.to_ascii_lowercase().contains("inf"))
                .map(RubyValue::Float)
                .ok_or_else(|| {
                    crate::dispatch::raise_error(
                        "ArgumentError",
                        format!("invalid value for Float(): {:?}", text),
                    )
                })
        }
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "can't convert {} into Float",
                if matches!(other, RubyValue::Nil) {
                    "nil".to_string()
                } else {
                    crate::builtins::class_name_of(other)
                }
            ),
        )),
    }
}

/// `Kernel#Rational(num, den = 1)` -- exact components only (string forms
/// are a documented scope-cut).
pub fn kernel_rational(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1..=2);
    let exact = |v: &RubyValue| -> Result<(num_bigint::BigInt, num_bigint::BigInt), Signal> {
        match v {
            RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Rational(_) => {
                Ok(crate::builtins::rational::as_ratio(v))
            }
            other => Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "can't convert {} into Rational",
                    crate::builtins::class_name_of(other)
                ),
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

/// `Kernel#Complex(real, imag = 0)`.
pub fn kernel_complex(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1..=2);
    let imag = args.get(1).cloned().unwrap_or(RubyValue::Int(0));
    crate::builtins::complex::complex_new(args[0].clone(), imag)
}

/// `Kernel#String(arg)` -- `to_s` (the `to_str`-first nuance is invisible
/// for builtin receivers).
pub fn kernel_string(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1);
    Ok(RubyValue::Str(crate::string_new(args[0].to_display_string())))
}

/// `Kernel#Array(arg)`: nil -> [], Array -> itself, Hash -> assoc pairs,
/// Range -> to_a, anything else -> [arg]. (`to_ary`/`to_a` protocol probes
/// on user objects are a documented scope-cut.)
pub fn kernel_array(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1);
    Ok(match &args[0] {
        RubyValue::Nil => RubyValue::Array(crate::array_new(Vec::new())),
        RubyValue::Array(_) => args[0].clone(),
        RubyValue::Hash(h) => RubyValue::Array(crate::array_new(
            h.lock()
                .values()
                .map(|(k, v)| {
                    RubyValue::Array(crate::array_new(vec![k.clone(), v.clone()]))
                })
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
pub fn kernel_hash(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1);
    match &args[0] {
        RubyValue::Nil => Ok(RubyValue::Hash(crate::hash_new(Vec::new()))),
        RubyValue::Array(a) if a.lock().is_empty() => {
            Ok(RubyValue::Hash(crate::hash_new(Vec::new())))
        }
        RubyValue::Hash(_) => Ok(args[0].clone()),
        other => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "can't convert {} into Hash",
                crate::builtins::class_name_of(other)
            ),
        )),
    }
}


/// `Kernel#puts`: zero args print one newline; arrays flatten recursively,
/// each scalar on its own line (nil renders empty) -- CRuby's exact rules.
pub fn kernel_puts(args: &[RubyValue]) -> RubyValue {
    // `seen` guards SELF-REFERENTIAL arrays (CRuby prints `[...]` for the
    // recursive appearance instead of flattening forever).
    fn put_one(v: &RubyValue, seen: &mut Vec<usize>) {
        match v {
            RubyValue::Array(a) => {
                let id = std::sync::Arc::as_ptr(a) as usize;
                if seen.contains(&id) {
                    println!("[...]");
                    return;
                }
                seen.push(id);
                let items = a.lock().clone();
                if items.is_empty() {
                    println!();
                }
                for e in &items {
                    put_one(e, seen);
                }
                seen.pop();
            }
            other => {
                let s = other.to_display_string();
                if s.ends_with('\n') {
                    print!("{s}");
                } else {
                    println!("{s}");
                }
            }
        }
    }
    if args.is_empty() {
        println!();
    }
    for a in args {
        put_one(a, &mut Vec::new());
    }
    RubyValue::Nil
}

/// `Kernel#p`: each argument's INSPECT rendering on its own line; returns
/// nil / the single argument / the argument array (CRuby's exact shapes).
pub fn kernel_p(args: &[RubyValue]) -> RubyValue {
    for a in args {
        println!("{}", a.inspect_string());
    }
    match args.len() {
        0 => RubyValue::Nil,
        1 => args[0].clone(),
        _ => RubyValue::Array(crate::array_new(args.to_vec())),
    }
}

/// `Kernel#pp` -- for this runtime's value shapes, `p`'s rendering.
pub fn kernel_pp(args: &[RubyValue]) -> RubyValue {
    kernel_p(args)
}

/// `Kernel#print`: display renderings, no separators, no newline.
pub fn kernel_print(args: &[RubyValue]) -> RubyValue {
    for a in args {
        print!("{}", a.to_display_string());
    }
    use std::io::Write;
    let _ = std::io::stdout().flush();
    RubyValue::Nil
}

/// `Kernel#format`/`sprintf`.
pub fn kernel_format(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let Some((RubyValue::Str(template), rest)) = args.split_first() else {
        return Err(crate::dispatch::raise_error(
            "TypeError",
            "no format string given".to_string(),
        ));
    };
    let template = template.lock().clone();
    Ok(RubyValue::Str(crate::string_new(
        crate::builtins::format::sprintf(&template, rest)?,
    )))
}

/// `Kernel#printf`.
pub fn kernel_printf(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    if args.is_empty() {
        return Ok(RubyValue::Nil);
    }
    let formatted = kernel_format(args)?;
    print!("{}", formatted.to_display_string());
    use std::io::Write;
    let _ = std::io::stdout().flush();
    Ok(RubyValue::Nil)
}

/// The process-wide PRNG behind `rand`/`srand` -- xorshift64*, reseedable.
/// DOCUMENTED DIVERGENCE: not CRuby's MT19937, so seeded SEQUENCES differ;
/// oracle tests assert ranges/properties, never exact values.
static PRNG: parking_lot::Mutex<(u64, u64)> = parking_lot::Mutex::new((0, 0)); // (state, seed)

pub(crate) fn prng_next() -> u64 {
    let mut guard = PRNG.lock();
    if guard.0 == 0 {
        // First use, unseeded: derive from the clock.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9e3779b97f4a7c15);
        *guard = (now | 1, now);
    }
    let mut x = guard.0;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    guard.0 = x;
    x.wrapping_mul(0x2545f4914f6cdd1d)
}

/// `Kernel#rand`: no arg -> Float in [0, 1); positive Integer n -> Integer
/// in [0, n); Float x -> Float in [0, x).
pub fn kernel_rand(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 0..=1);
    let r = prng_next();
    Ok(match args.first() {
        None | Some(RubyValue::Nil) | Some(RubyValue::Int(0)) => {
            RubyValue::Float((r >> 11) as f64 / (1u64 << 53) as f64)
        }
        Some(RubyValue::Int(n)) if *n > 0 => RubyValue::Int((r % (*n as u64)) as i64),
        Some(RubyValue::Int(n)) => RubyValue::Int(-((r % (n.unsigned_abs())) as i64)),
        Some(RubyValue::Float(x)) => {
            RubyValue::Float((r >> 11) as f64 / (1u64 << 53) as f64 * x)
        }
        Some(other) => {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                format!("invalid argument - {}", other.to_display_string()),
            ))
        }
    })
}

/// `Kernel#srand(seed)`: reseeds, returns the PREVIOUS seed.
pub fn kernel_srand(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 0..=1);
    let new_seed = match args.first() {
        Some(RubyValue::Int(n)) => *n as u64,
        _ => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1),
    };
    let mut guard = PRNG.lock();
    let previous = guard.1;
    *guard = (new_seed | 1, new_seed);
    Ok(crate::builtins::integer::int_value(num_bigint::BigInt::from(previous)))
}

/// `Kernel#catch(tag) { ... }` / `Kernel#throw(tag[, value])`.
pub fn kernel_catch(tag: RubyValue, block: RubyValue) -> Result<RubyValue, Signal> {
    let RubyValue::Proc(p) = &block else {
        panic!("Kernel#catch requires a block");
    };
    match p(std::slice::from_ref(&tag)) {
        Err(Signal::Throw(t, v)) if t.rb_eq(&tag) => Ok(v),
        other => other,
    }
}

pub fn kernel_throw(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 1..=2);
    Err(Signal::Throw(
        args[0].clone(),
        args.get(1).cloned().unwrap_or(RubyValue::Nil),
    ))
}

/// `Kernel#sleep(seconds)` -- cooperative (`may`'s coroutine sleep, like
/// the Thread machinery); returns the rounded seconds slept.
pub fn kernel_sleep(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    crate::builtins::arity!(args, 0..=1);
    let secs = match args.first() {
        Some(RubyValue::Int(n)) if *n >= 0 => *n as f64,
        Some(RubyValue::Float(f)) if *f >= 0.0 => *f,
        None => {
            panic!("Kernel#sleep without a duration (sleep forever) isn't supported (spike scope)")
        }
        Some(other) => {
            return Err(crate::dispatch::raise_error(
                "TypeError",
                format!(
                    "can't convert {} into time interval",
                    crate::builtins::class_name_of(other)
                ),
            ))
        }
    };
    may::coroutine::sleep(std::time::Duration::from_secs_f64(secs));
    Ok(RubyValue::Int(secs.round() as i64))
}

/// `Kernel#exit` / `Kernel#abort` -- direct process exit (CRuby raises
/// SystemExit through at_exit handlers; both are documented scope-cuts).
pub fn kernel_exit(args: &[RubyValue]) -> ! {
    let code = match args.first() {
        None | Some(RubyValue::Bool(true)) => 0,
        Some(RubyValue::Bool(false)) => 1,
        Some(RubyValue::Int(n)) => *n as i32,
        Some(_) => 0,
    };
    std::process::exit(code)
}

pub fn kernel_abort(args: &[RubyValue]) -> ! {
    if let Some(msg) = args.first() {
        eprintln!("{}", msg.to_display_string());
    }
    std::process::exit(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eql_requires_same_class_and_equality() {
        let t = eql_p(&RubyValue::Int(1), &[RubyValue::Int(1)], None).unwrap();
        assert!(matches!(t, RubyValue::Bool(true)));
        let f = eql_p(&RubyValue::Int(1), &[RubyValue::Float(1.0)], None).unwrap();
        assert!(matches!(f, RubyValue::Bool(false)));
    }

    #[test]
    fn to_s_and_inspect_render_like_puts_and_p() {
        let s = to_s(&RubyValue::Nil, &[], None).unwrap();
        let RubyValue::Str(s) = s else { panic!() };
        assert_eq!(&*s.lock(), "");
        let i = inspect(&RubyValue::Nil, &[], None).unwrap();
        let RubyValue::Str(i) = i else { panic!() };
        assert_eq!(&*i.lock(), "nil");
    }

    #[test]
    fn itself_returns_the_receiver_and_freeze_reports_frozen() {
        assert!(matches!(
            itself(&RubyValue::Int(7), &[], None).unwrap(),
            RubyValue::Int(7)
        ));
        assert!(matches!(
            frozen_p(&RubyValue::Int(7), &[], None).unwrap(),
            RubyValue::Bool(true) // immediates are frozen
        ));
        let s = RubyValue::Str(crate::string_new("x".to_string()));
        assert!(matches!(
            frozen_p(&s, &[], None).unwrap(),
            RubyValue::Bool(false)
        ));
        freeze(&s, &[], None).unwrap();
        assert!(matches!(
            frozen_p(&s, &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
    }
}

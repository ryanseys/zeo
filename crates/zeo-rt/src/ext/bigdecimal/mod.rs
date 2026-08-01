//! `bigdecimal` -- the `BigDecimal` class, an in-tree require-gated
//! extension. This native half is the same slice CRuby's C extension keeps
//! for itself in bigdecimal 4.x: the value type, exact add/sub/mult,
//! division to the documented precision rule, rounding, comparisons,
//! conversions, and the process-wide mode/limit state. Everything built on
//! top -- `**`/`power`, `sqrt`, `BigMath.exp`/`log`, `util`'s `to_d`
//! family -- is the gem's own RUBY code, vendored untouched in
//! `gems/bigdecimal/` and compiled like any user code.

pub(crate) mod arith;
pub(crate) mod value;

use arith::{
    DOUBLE_FIG, EXCEPTION_ALL, EXCEPTION_INFINITY, EXCEPTION_NAN, EXCEPTION_OVERFLOW,
    EXCEPTION_UNDERFLOW, EXCEPTION_ZERODIVIDE, ROUND_CEILING, ROUND_DOWN, ROUND_FLOOR,
    ROUND_HALF_DOWN, ROUND_HALF_EVEN, ROUND_HALF_UP, ROUND_UP,
};
use num_bigint::{BigInt, BigUint, Sign};
use num_traits::Zero;
use std::sync::Arc;
use value::BD;
use zeo_macros::ruby_class;

use crate::builtins::{arg_error, convert, type_error};
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::{RubyValue, Signal};

pub(crate) struct RBigDecimal {
    pub(crate) bd: BD,
}

impl RubyObject for RBigDecimal {
    fn class_id(&self) -> crate::ClassId {
        zeo_abi::BIGDECIMAL_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    // BigDecimal values are frozen, as in CRuby.
    fn is_frozen(&self) -> bool {
        true
    }
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(RBigDecimal {
            bd: self.bd.clone(),
        })
    }
}

/// Wraps without the exception gate (constants, plain readers).
pub(crate) fn wrap(bd: BD) -> RubyValue {
    RubyValue::Object(Arc::new(RBigDecimal { bd }))
}

/// Wraps a COMPUTATION result: NaN/Infinity raise `FloatDomainError` when
/// the corresponding `BigDecimal.mode` exception bit is on (CRuby's
/// `CheckGetValue`).
fn checked(bd: BD) -> Result<RubyValue, Signal> {
    let flags = arith::exception_flags();
    match &bd {
        BD::NaN if flags & EXCEPTION_NAN != 0 => Err(raise_error(
            "FloatDomainError",
            "Computation results in 'NaN' (Not a Number)".to_string(),
        )),
        BD::Inf(s) if flags & EXCEPTION_INFINITY != 0 => Err(raise_error(
            "FloatDomainError",
            format!(
                "Computation results in '{}Infinity'",
                if *s < 0 { "-" } else { "" }
            ),
        )),
        _ => Ok(wrap(bd)),
    }
}

fn bd_of(v: &RubyValue) -> Option<&BD> {
    match v {
        RubyValue::Object(o) => o.as_any().downcast_ref::<RBigDecimal>().map(|r| &r.bd),
        _ => None,
    }
}

fn recv_bd(recv: &RubyValue) -> &BD {
    bd_of(recv).expect("the BigDecimal table only dispatches on BigDecimal receivers")
}

fn bd_from_bigint(n: &BigInt) -> BD {
    let (sign, mag) = n.clone().into_parts();
    let l = value::ndigits(&mag) as i64;
    BD::fin(if sign == Sign::Minus { -1 } else { 1 }, mag, l)
}

/// The `GetCoercePrec` rule for converting a Rational operand: the given
/// precision, else the receiver's word-aligned digit capacity, floored at
/// `2*DOUBLE_FIG`.
fn coerce_prec(a: &BD, prec: i64) -> i64 {
    let p = if prec == 0 {
        (a.word_prec() * 9) as i64
    } else {
        prec
    };
    p.max(2 * DOUBLE_FIG)
}

/// Converts a mixed-arithmetic operand: Integer exactly, Float by its
/// shortest decimal form, Rational by division at `prec` digits. `None`
/// hands the caller to the coerce protocol / TypeError.
fn operand_bd(v: &RubyValue, prec: i64) -> Result<Option<BD>, Signal> {
    Ok(match v {
        RubyValue::Int(n) => Some(bd_from_bigint(&BigInt::from(*n))),
        RubyValue::BigInt(n) => Some(bd_from_bigint(n)),
        RubyValue::Float(f) => Some(value::from_f64(*f, 0)),
        RubyValue::Rational(r) => Some(rational_bd(&r.num, &r.den, prec)?),
        RubyValue::Object(_) => bd_of(v).cloned(),
        _ => None,
    })
}

fn rational_bd(num: &BigInt, den: &BigInt, prec: i64) -> Result<BD, Signal> {
    let a = bd_from_bigint(num);
    let b = bd_from_bigint(den);
    if a.is_zero() {
        return Ok(BD::Zero(1));
    }
    Ok(arith::div_to(&a, &b, prec))
}

fn coerce_error(v: &RubyValue) -> Signal {
    type_error!("{} can't be coerced into BigDecimal", v.inspect_string())
}

/// The shared binary-operand path: BigDecimal/Integer/Float/Rational
/// convert; anything else raises CRuby's coercion `TypeError`.
fn rhs_bd(a: &BD, v: &RubyValue) -> Result<BD, Signal> {
    operand_bd(v, coerce_prec(a, 0))?.ok_or_else(|| coerce_error(v))
}

// ---------------------------------------------------------------------------
// Division building blocks (CRuby's BigDecimal_div2 / DoDivmod)
// ---------------------------------------------------------------------------

/// `a / b` to `ix` digits (0 = the default rule), with the special-value
/// and zero-divide handling of `BigDecimal_div2`.
fn div2(a: &BD, b: &BD, ix: i64) -> Result<RubyValue, Signal> {
    if matches!(a, BD::NaN) || matches!(b, BD::NaN) {
        return checked(BD::NaN);
    }
    match (a, b) {
        (BD::Inf(_), BD::Inf(_)) => return checked(BD::NaN),
        (BD::Inf(s), other) => return checked(BD::Inf(s * other.sign_factor())),
        (_, BD::Inf(_)) => return checked(BD::Zero(a.sign_factor() * b.sign_factor())),
        _ => {}
    }
    if b.is_zero() {
        if a.is_zero() {
            return checked(BD::NaN);
        }
        if arith::exception_flags() & EXCEPTION_ZERODIVIDE != 0 {
            return Err(raise_error(
                "FloatDomainError",
                "Divide by zero".to_string(),
            ));
        }
        return checked(BD::Inf(a.sign_factor() * b.sign_factor()));
    }
    if a.is_zero() {
        return checked(BD::Zero(a.sign_factor() * b.sign_factor()));
    }
    let ix = if ix == 0 {
        arith::default_div_prec(a, b)
    } else {
        ix
    };
    checked(arith::div_to(a, b, ix))
}

/// Floor (or truncating) quotient and matching modulus -- `DoDivmod`.
fn do_divmod(a: &BD, b: &BD, truncate: bool) -> Result<(BD, BD), Signal> {
    if matches!(a, BD::NaN)
        || matches!(b, BD::NaN)
        || (matches!(a, BD::Inf(_)) && matches!(b, BD::Inf(_)))
    {
        return Ok((BD::NaN, BD::NaN));
    }
    if b.is_zero() {
        return Err(raise_error("ZeroDivisionError", "divided by 0".to_string()));
    }
    if let BD::Inf(sa) = a {
        let div = BD::Inf(sa * b.sign_factor());
        return Ok((div, BD::NaN));
    }
    if a.is_zero() {
        return Ok((BD::Zero(1), a.clone()));
    }
    if matches!(b, BD::Inf(_)) {
        if !truncate && a.sign_factor() * b.sign_factor() < 0 {
            return Ok((bd_from_bigint(&BigInt::from(-1)), b.clone()));
        }
        return Ok((BD::Zero(1), a.clone()));
    }
    let q = arith::trunc_quotient(a, b);
    let m = arith::sub(a, &arith::mult(&q, b));
    if !truncate && !m.is_zero() && a.sign_factor() * b.sign_factor() < 0 {
        let q = arith::sub(&q, &BD::one());
        let m = arith::add(&m, b);
        return Ok((q, m));
    }
    Ok((q, m))
}

// ---------------------------------------------------------------------------
// Conversions out
// ---------------------------------------------------------------------------

fn to_integer(bd: &BD) -> Result<RubyValue, Signal> {
    match bd {
        BD::NaN => Err(raise_error(
            "FloatDomainError",
            "Computation results in 'NaN' (Not a Number)".to_string(),
        )),
        BD::Inf(s) => Err(raise_error(
            "FloatDomainError",
            format!(
                "Computation results in '{}Infinity'",
                if *s < 0 { "-" } else { "" }
            ),
        )),
        BD::Zero(_) => Ok(RubyValue::Int(0)),
        BD::Fin { sign, coeff, exp } => {
            let l = value::ndigits(coeff) as i64;
            let n: BigUint = if *exp <= 0 {
                BigUint::zero()
            } else if *exp >= l {
                coeff * value::pow10((*exp - l) as u64)
            } else {
                coeff / value::pow10((l - *exp) as u64)
            };
            let n = BigInt::from_biguint(if *sign < 0 { Sign::Minus } else { Sign::Plus }, n);
            Ok(crate::builtins::integer::int_value(n))
        }
    }
}

fn to_float(bd: &BD) -> f64 {
    match bd {
        BD::NaN => f64::NAN,
        BD::Inf(s) => f64::INFINITY * *s as f64,
        BD::Zero(s) => 0.0 * *s as f64,
        BD::Fin { .. } => value::to_s(bd, "").parse().unwrap_or(f64::NAN),
    }
}

/// The rounding-position family: `round`/`floor`/`ceil`/`truncate` share
/// argument shapes -- no argument (or `n < 1`) answers an Integer, a
/// positive `n` a BigDecimal.
fn positional(
    recv: &RubyValue,
    ndigits: Option<&RubyValue>,
    mode: u32,
) -> Result<RubyValue, Signal> {
    let bd = recv_bd(recv);
    let n = match ndigits {
        None => 0,
        Some(v) => convert::to_index(v)?,
    };
    let rounded = arith::mid_round(bd, mode, n);
    if ndigits.is_none() || n < 1 {
        to_integer(&rounded)
    } else {
        checked(rounded)
    }
}

/// A rounding-mode SETTING: the integer constants or their symbol names,
/// CRuby's `check_rounding_mode`.
fn rounding_mode_arg(v: &RubyValue) -> Result<u32, Signal> {
    if let RubyValue::Symbol(s) = v {
        return match s.name().as_str() {
            "up" => Ok(ROUND_UP),
            "down" | "truncate" => Ok(ROUND_DOWN),
            "half_up" | "default" => Ok(ROUND_HALF_UP),
            "half_down" => Ok(ROUND_HALF_DOWN),
            "half_even" | "banker" => Ok(ROUND_HALF_EVEN),
            "ceiling" | "ceil" => Ok(ROUND_CEILING),
            "floor" => Ok(ROUND_FLOOR),
            _ => Err(arg_error!("invalid rounding mode: {}", s.name())),
        };
    }
    let n = convert::to_index(v)?;
    if (1..=7).contains(&n) {
        Ok(n as u32)
    } else {
        Err(arg_error!("invalid rounding mode: {n}"))
    }
}

/// The `half:` keyword form (`round(half: :even)`).
fn rounding_mode_option(h: &RubyValue) -> Result<u32, Signal> {
    let RubyValue::Hash(hash) = h else {
        return rounding_mode_arg(h);
    };
    let half = crate::hash_get(hash, &RubyValue::Symbol(crate::Symbol::intern("half")));
    match half {
        RubyValue::Symbol(s) => match s.name().as_str() {
            "up" => Ok(ROUND_HALF_UP),
            "down" => Ok(ROUND_HALF_DOWN),
            "even" => Ok(ROUND_HALF_EVEN),
            other => Err(arg_error!("invalid rounding mode (half: :{other})")),
        },
        _ => Err(arg_error!("invalid rounding mode")),
    }
}

// ---------------------------------------------------------------------------
// Kernel#BigDecimal
// ---------------------------------------------------------------------------

/// `Kernel#BigDecimal(value, ndigits = nil, exception: true)` -- CRuby's
/// `f_BigDecimal`.
pub(crate) fn kernel_big_decimal(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    // A trailing `exception:` hash is the only keyword.
    let (args, exception) = split_exception_kwarg(args);
    crate::builtins::check_arity(args.len(), 1, Some(2))?;
    let digs: Option<i64> = match args.get(1) {
        None => None,
        Some(v) => {
            let n = convert::to_index(v)?;
            if n < 0 {
                if !exception {
                    return Ok(RubyValue::Nil);
                }
                return Err(arg_error!("negative precision"));
            }
            Some(n)
        }
    };
    let fail = |sig: Signal| {
        if exception {
            Err(sig)
        } else {
            Ok(RubyValue::Nil)
        }
    };
    match &args[0] {
        RubyValue::Str(s) => {
            let text = s.lock().to_utf8_lossy().into_owned();
            match value::parse(&text, true) {
                Some(bd) => checked(bd),
                None => fail(arg_error!("invalid value for BigDecimal(): \"{text}\"")),
            }
        }
        RubyValue::Int(n) => Ok(wrap(bd_from_bigint(&BigInt::from(*n)))),
        RubyValue::BigInt(n) => Ok(wrap(bd_from_bigint(n))),
        RubyValue::Float(f) => {
            let digs = digs.unwrap_or(0);
            if digs > DOUBLE_FIG {
                return fail(arg_error!("precision too large."));
            }
            checked(value::from_f64(*f, digs as usize))
        }
        RubyValue::Rational(r) => match digs {
            None => fail(arg_error!("can't omit precision for a Rational.")),
            Some(d) => checked(rational_bd(
                &r.num,
                &r.den,
                if d == 0 { 2 * DOUBLE_FIG } else { d },
            )?),
        },
        v => match bd_of(v) {
            Some(bd) => checked(bd.clone()),
            None => {
                let name = match v {
                    RubyValue::Nil => "nil".to_string(),
                    RubyValue::Bool(true) => "true".to_string(),
                    RubyValue::Bool(false) => "false".to_string(),
                    other => crate::class_name_of_value(other),
                };
                fail(type_error!("can't convert {name} into BigDecimal"))
            }
        },
    }
}

fn split_exception_kwarg(args: &[RubyValue]) -> (&[RubyValue], bool) {
    if let Some(RubyValue::Hash(h)) = args.last() {
        let key = RubyValue::Symbol(crate::Symbol::intern("exception"));
        if crate::hash_has_key(h, &key) && crate::hash_len(h) == 1 {
            let on = !matches!(
                crate::hash_get(h, &key),
                RubyValue::Nil | RubyValue::Bool(false)
            );
            return (&args[..args.len() - 1], on);
        }
    }
    (args, true)
}

/// Runs `block` and restores a piece of global state after -- the
/// `save_exception_mode`/`save_rounding_mode`/`save_limit` shape.
fn with_restored<G: Fn() -> i64, S: Fn(i64)>(
    get: G,
    set: S,
    block: Option<&RubyValue>,
) -> Result<RubyValue, Signal> {
    let Some(RubyValue::Proc(p)) = block else {
        return Err(raise_error("LocalJumpError", "no block given".to_string()));
    };
    let saved = get();
    let result = p.call(&[]);
    set(saved);
    result
}

ruby_class! {
    BigDecimal = zeo_abi::BIGDECIMAL_CLASS < zeo_abi::NUMERIC_CLASS;

    const VERSION = str_value("4.1.2");
    const BASE = RubyValue::Int(1_000_000_000);
    const INFINITY = wrap(BD::Inf(1));
    const NAN = wrap(BD::NaN);
    const SIGN_NaN = RubyValue::Int(0);
    const SIGN_POSITIVE_ZERO = RubyValue::Int(1);
    const SIGN_NEGATIVE_ZERO = RubyValue::Int(-1);
    const SIGN_POSITIVE_FINITE = RubyValue::Int(2);
    const SIGN_NEGATIVE_FINITE = RubyValue::Int(-2);
    const SIGN_POSITIVE_INFINITE = RubyValue::Int(3);
    const SIGN_NEGATIVE_INFINITE = RubyValue::Int(-3);
    const ROUND_UP = RubyValue::Int(ROUND_UP as i64);
    const ROUND_DOWN = RubyValue::Int(ROUND_DOWN as i64);
    const ROUND_HALF_UP = RubyValue::Int(ROUND_HALF_UP as i64);
    const ROUND_HALF_DOWN = RubyValue::Int(ROUND_HALF_DOWN as i64);
    const ROUND_CEILING = RubyValue::Int(ROUND_CEILING as i64);
    const ROUND_FLOOR = RubyValue::Int(ROUND_FLOOR as i64);
    const ROUND_HALF_EVEN = RubyValue::Int(ROUND_HALF_EVEN as i64);
    const ROUND_MODE = RubyValue::Int(256);
    const EXCEPTION_ALL = RubyValue::Int(EXCEPTION_ALL as i64);
    const EXCEPTION_NaN = RubyValue::Int(EXCEPTION_NAN as i64);
    const EXCEPTION_INFINITY = RubyValue::Int(EXCEPTION_INFINITY as i64);
    const EXCEPTION_UNDERFLOW = RubyValue::Int(EXCEPTION_UNDERFLOW as i64);
    const EXCEPTION_OVERFLOW = RubyValue::Int(EXCEPTION_OVERFLOW as i64);
    const EXCEPTION_ZERODIVIDE = RubyValue::Int(EXCEPTION_ZERODIVIDE as i64);

    def self."double_fig"(_recv) {
        Ok(RubyValue::Int(DOUBLE_FIG))
    }

    // `BigDecimal.limit([n])`: reads (and optionally sets) the global
    // significant-digit cap; answers the PREVIOUS value on set.
    def self."limit" (_recv, arg?) {
        let current = arith::prec_limit();
        match arg {
            None | Some(RubyValue::Nil) => Ok(RubyValue::Int(current)),
            Some(v) => {
                let n = convert::to_index(v)?;
                if n < 0 {
                    return Err(arg_error!("argument must be positive"));
                }
                arith::set_prec_limit(n);
                Ok(RubyValue::Int(current))
            }
        }
    }

    // `BigDecimal.mode(flag[, setting])`: ROUND_MODE (256) reads/sets the
    // rounding mode; an EXCEPTION_* mask reads/sets those bits, answering
    // the full flag set.
    def self."mode" arity -1 (_recv, arg1, arg2?) {
        let flag = convert::to_index(arg1)?;
        if flag == 256 {
            match arg2 {
                None | Some(RubyValue::Nil) => Ok(RubyValue::Int(arith::round_mode() as i64)),
                Some(v) => {
                    let m = rounding_mode_arg(v)?;
                    arith::set_round_mode(m);
                    Ok(RubyValue::Int(m as i64))
                }
            }
        } else {
            let mask = flag as u32 & EXCEPTION_ALL;
            match arg2 {
                None | Some(RubyValue::Nil) => Ok(RubyValue::Int(arith::exception_flags() as i64)),
                Some(RubyValue::Bool(on)) => {
                    let flags = arith::exception_flags();
                    let flags = if *on { flags | mask } else { flags & !mask };
                    arith::set_exception_flags(flags);
                    Ok(RubyValue::Int(flags as i64))
                }
                Some(_) => Err(arg_error!("second argument must be true or false")),
            }
        }
    }

    def self."save_exception_mode"(_recv, &block) {
        with_restored(
            || arith::exception_flags() as i64,
            |v| arith::set_exception_flags(v as u32),
            block.as_ref(),
        )
    }
    def self."save_rounding_mode"(_recv, &block) {
        with_restored(
            || arith::round_mode() as i64,
            |v| arith::set_round_mode(v as u32),
            block.as_ref(),
        )
    }
    def self."save_limit"(_recv, &block) {
        with_restored(arith::prec_limit, arith::set_prec_limit, block.as_ref())
    }

    def self."interpret_loosely"(_recv, arg) {
        let text = convert::to_rstr(arg)?.lock().to_utf8_lossy().into_owned();
        Ok(wrap(value::parse(&text, false).expect("loose parsing is total")))
    }

    def "+" (recv, other) {
        let a = recv_bd(recv);
        checked(arith::limit_round(&arith::add(a, &rhs_bd(a, other)?)))
    }
    def "-" (recv, other) {
        let a = recv_bd(recv);
        checked(arith::limit_round(&arith::sub(a, &rhs_bd(a, other)?)))
    }
    def "*" (recv, other) {
        let a = recv_bd(recv);
        checked(arith::limit_round(&arith::mult(a, &rhs_bd(a, other)?)))
    }
    def "/" (recv, other) {
        let a = recv_bd(recv);
        div2(a, &rhs_bd(a, other)?, 0)
    }

    // `add`/`sub`/`mult`: the operator plus a REQUIRED result precision
    // (`0` keeps the exact/limit behaviour).
    def "add" (recv, arg1, arg2) {
        let a = recv_bd(recv);
        let prec = precision_arg(arg2)?;
        let b = operand_bd(arg1, coerce_prec(a, prec))?.ok_or_else(|| coerce_error(arg1))?;
        let sum = arith::add(a, &b);
        checked(if prec > 0 {
            arith::left_round(&sum, arith::round_mode(), prec)
        } else {
            arith::limit_round(&sum)
        })
    }
    def "sub" (recv, arg1, arg2) {
        let a = recv_bd(recv);
        let prec = precision_arg(arg2)?;
        let b = operand_bd(arg1, coerce_prec(a, prec))?.ok_or_else(|| coerce_error(arg1))?;
        let diff = arith::sub(a, &b);
        checked(if prec > 0 {
            arith::left_round(&diff, arith::round_mode(), prec)
        } else {
            arith::limit_round(&diff)
        })
    }
    def "mult" (recv, arg1, arg2) {
        let a = recv_bd(recv);
        let prec = precision_arg(arg2)?;
        let b = operand_bd(arg1, coerce_prec(a, prec))?.ok_or_else(|| coerce_error(arg1))?;
        let prod = arith::mult(a, &b);
        checked(if prec > 0 {
            arith::left_round(&prod, arith::round_mode(), prec)
        } else {
            arith::limit_round(&prod)
        })
    }

    // `div(b)` is the Integer floor quotient; `div(b, prec)` divides to
    // `prec` digits (0 = the `/` rule).
    def "div" arity -1 (recv, arg1, arg2?) {
        let a = recv_bd(recv);
        match arg2 {
            None => {
                let b = rhs_bd(a, arg1)?;
                let (q, _) = do_divmod(a, &b, false)?;
                to_integer(&q)
            }
            Some(p) => {
                let prec = precision_arg(p)?;
                let b = operand_bd(arg1, coerce_prec(a, prec))?
                    .ok_or_else(|| coerce_error(arg1))?;
                div2(a, &b, prec)
            }
        }
    }
    def "quo" arity -1 (recv, arg1, arg2?) {
        let a = recv_bd(recv);
        let prec = match arg2 {
            None => 0,
            Some(p) => precision_arg(p)?,
        };
        let b = operand_bd(arg1, coerce_prec(a, prec))?
            .ok_or_else(|| coerce_error(arg1))?;
        div2(a, &b, prec)
    }
    def "%" | "modulo" (recv, other) {
        let a = recv_bd(recv);
        let b = rhs_bd(a, other)?;
        let (_, m) = do_divmod(a, &b, false)?;
        checked(m)
    }
    def "remainder" (recv, arg) {
        let a = recv_bd(recv);
        let b = rhs_bd(a, arg)?;
        let (_, m) = do_divmod(a, &b, true)?;
        checked(m)
    }
    def "divmod" (recv, arg) {
        let a = recv_bd(recv);
        let b = rhs_bd(a, arg)?;
        let (q, m) = do_divmod(a, &b, false)?;
        let q = match q {
            BD::NaN | BD::Inf(_) => wrap(q),
            _ => to_integer(&q)?,
        };
        Ok(RubyValue::Array(crate::array_new(vec![q, checked(m)?])))
    }

    def "-@" (recv) {
        checked(recv_bd(recv).neg())
    }
    def "+@" (recv) {
        Ok(recv.clone())
    }
    def "abs" (recv) {
        checked(recv_bd(recv).abs())
    }

    // `fix`/`frac`: the integer and fraction parts, both BigDecimal.
    def "fix" (recv) {
        checked(arith::mid_round(recv_bd(recv), ROUND_DOWN, 0))
    }
    def "frac" (recv) {
        let bd = recv_bd(recv);
        let fix = arith::mid_round(bd, ROUND_DOWN, 0);
        checked(arith::sub(bd, &fix))
    }

    def "floor" (recv, ndigits?) {
        positional(recv, ndigits, ROUND_FLOOR)
    }
    def "ceil" (recv, ndigits?) {
        positional(recv, ndigits, ROUND_CEILING)
    }
    def "truncate" (recv, ndigits?) {
        positional(recv, ndigits, ROUND_DOWN)
    }
    // `round`: `()` and `(n < 1)` answer Integers; a `half:` hash or a
    // trailing mode symbol/flag overrides the global mode.
    def "round" (recv, _n?, _mode?) {
        let args = __args;
        let bd = recv_bd(recv);
        let mut mode = arith::round_mode();
        let mut n = 0i64;
        let mut to_int = args.is_empty();
        match args {
            [] => {}
            [RubyValue::Hash(_)] => mode = rounding_mode_option(&args[0])?,
            [v] => {
                n = convert::to_index(v)?;
                to_int = n < 1;
            }
            [v, m] => {
                n = convert::to_index(v)?;
                to_int = n < 1;
                mode = rounding_mode_option(m)?;
            }
            _ => unreachable!("arity checked above"),
        }
        let rounded = arith::mid_round(bd, mode, n);
        if to_int {
            to_integer(&rounded)
        } else {
            checked(rounded)
        }
    }

    def "<=>" (recv, other) {
        let a = recv_bd(recv);
        let Some(b) = operand_bd(other, coerce_prec(a, 0))? else {
            return Ok(RubyValue::Nil);
        };
        Ok(match arith::cmp(a, &b) {
            None => RubyValue::Nil,
            Some(o) => RubyValue::Int(o as i64),
        })
    }
    def "==" | "===" | "eql?" (recv, other) {
        let a = recv_bd(recv);
        let b = operand_bd(other, coerce_prec(a, 0))?;
        Ok(RubyValue::Bool(matches!(
            b.and_then(|b| arith::cmp(a, &b)),
            Some(std::cmp::Ordering::Equal)
        )))
    }
    def "<" (recv, other) {
        compare(recv, other, |o| o == std::cmp::Ordering::Less)
    }
    def "<=" (recv, other) {
        compare(recv, other, |o| o != std::cmp::Ordering::Greater)
    }
    def ">" (recv, other) {
        compare(recv, other, |o| o == std::cmp::Ordering::Greater)
    }
    def ">=" (recv, other) {
        compare(recv, other, |o| o != std::cmp::Ordering::Less)
    }

    def "coerce" (recv, arg) {
        let a = recv_bd(recv);
        let b = rhs_bd(a, arg)?;
        Ok(RubyValue::Array(crate::array_new(vec![wrap(b), recv.clone()])))
    }

    def "hash" (recv) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        match recv_bd(recv) {
            BD::NaN => 0u8.hash(&mut h),
            BD::Inf(s) => (1u8, s).hash(&mut h),
            // Signed zeros compare equal, so they must hash equal.
            BD::Zero(_) => 2u8.hash(&mut h),
            BD::Fin { sign, coeff, exp } => (3u8, sign, coeff.to_bytes_le(), exp).hash(&mut h),
        }
        Ok(RubyValue::Int(h.finish() as i64))
    }

    def "zero?" (recv) {
        Ok(RubyValue::Bool(recv_bd(recv).is_zero()))
    }
    def "nonzero?" (recv) {
        Ok(if recv_bd(recv).is_zero() { RubyValue::Nil } else { recv.clone() })
    }
    def "positive?" (recv) {
        Ok(RubyValue::Bool(matches!(recv_bd(recv), BD::Fin { sign: 1, .. } | BD::Inf(1))))
    }
    def "negative?" (recv) {
        Ok(RubyValue::Bool(matches!(recv_bd(recv), BD::Fin { sign: -1, .. } | BD::Inf(-1))))
    }
    def "finite?" (recv) {
        Ok(RubyValue::Bool(matches!(recv_bd(recv), BD::Fin { .. } | BD::Zero(_))))
    }
    def "infinite?" (recv) {
        Ok(match recv_bd(recv) {
            BD::Inf(s) => RubyValue::Int(*s as i64),
            _ => RubyValue::Nil,
        })
    }
    def "nan?" (recv) {
        Ok(RubyValue::Bool(matches!(recv_bd(recv), BD::NaN)))
    }
    def "sign" (recv) {
        Ok(RubyValue::Int(recv_bd(recv).sign_code()))
    }
    def "exponent" (recv) {
        Ok(RubyValue::Int(recv_bd(recv).exponent()))
    }
    def "precision" (recv) {
        Ok(RubyValue::Int(recv_bd(recv).precision()))
    }
    def "scale" (recv) {
        Ok(RubyValue::Int(recv_bd(recv).scale()))
    }
    def "precision_scale" (recv) {
        let bd = recv_bd(recv);
        Ok(RubyValue::Array(crate::array_new(vec![
            RubyValue::Int(bd.precision()),
            RubyValue::Int(bd.scale()),
        ])))
    }
    def "n_significant_digits" (recv) {
        Ok(RubyValue::Int(recv_bd(recv).nsd() as i64))
    }
    def "split" (recv) {
        let bd = recv_bd(recv);
        let (sign, digits, exp) = match bd {
            BD::NaN => (0i64, "NaN".to_string(), 0),
            BD::Inf(s) => (*s as i64, "Infinity".to_string(), 0),
            BD::Zero(s) => (*s as i64, "0".to_string(), 0),
            BD::Fin { sign, coeff, exp } => (*sign as i64, coeff.to_str_radix(10), *exp),
        };
        Ok(RubyValue::Array(crate::array_new(vec![
            RubyValue::Int(sign),
            RubyValue::Str(crate::string_new(digits)),
            RubyValue::Int(10),
            RubyValue::Int(exp),
        ])))
    }

    def "_decimal_shift" (recv, arg) {
        Ok(wrap(recv_bd(recv).decimal_shift(convert::to_index(arg)?)))
    }

    def "to_i" | "to_int" (recv) {
        to_integer(recv_bd(recv))
    }
    def "to_f" (recv) {
        Ok(RubyValue::Float(to_float(recv_bd(recv))))
    }
    def "to_r" (recv) {
        match recv_bd(recv) {
            BD::Fin { sign, coeff, exp } => {
                let l = value::ndigits(coeff) as i64;
                let n = BigInt::from_biguint(
                    if *sign < 0 { Sign::Minus } else { Sign::Plus },
                    coeff.clone(),
                );
                if *exp >= l {
                    let n = n * BigInt::from(value::pow10((*exp - l) as u64));
                    crate::builtins::rational::rational_new(n, BigInt::from(1))
                } else {
                    crate::builtins::rational::rational_new(
                        n,
                        BigInt::from(value::pow10((l - *exp) as u64)),
                    )
                }
            }
            BD::Zero(_) => crate::builtins::rational::rational_new(BigInt::from(0), BigInt::from(1)),
            _ => Err(raise_error("FloatDomainError", value::to_s(recv_bd(recv), "").to_string())),
        }
    }
    def "to_s" (recv, arg?) {
        let fmt = match arg {
            None => String::new(),
            Some(RubyValue::Int(n)) => n.to_string(),
            Some(v) => convert::to_rstr(v)?.lock().to_utf8_lossy().into_owned(),
        };
        Ok(RubyValue::Str(crate::string_new(value::to_s(recv_bd(recv), &fmt))))
    }
    def "inspect" (recv) {
        Ok(RubyValue::Str(crate::string_new(value::to_s(recv_bd(recv), ""))))
    }

    def "clone" | "dup" (recv) {
        // Frozen value semantics: a copy IS the value.
        Ok(recv.clone())
    }
}

fn compare(
    recv: &RubyValue,
    other: &RubyValue,
    ok: fn(std::cmp::Ordering) -> bool,
) -> Result<RubyValue, Signal> {
    let a = recv_bd(recv);
    let b = operand_bd(other, coerce_prec(a, 0))?.ok_or_else(|| {
        raise_error(
            "ArgumentError",
            format!(
                "comparison of BigDecimal with {} failed",
                crate::class_name_of_value(other)
            ),
        )
    })?;
    match arith::cmp(a, &b) {
        Some(o) => Ok(RubyValue::Bool(ok(o))),
        None => Ok(RubyValue::Bool(false)),
    }
}

fn precision_arg(v: &RubyValue) -> Result<i64, Signal> {
    let n = convert::to_index(v)?;
    if n < 0 {
        return Err(arg_error!("negative precision"));
    }
    Ok(n)
}

fn str_value(s: &str) -> RubyValue {
    RubyValue::Str(crate::string_new(s.to_string()))
}

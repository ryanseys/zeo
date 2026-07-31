//! `OpenSSL::BN` -- OpenSSL's BIGNUM, wrapped directly (`openssl::bn`), so
//! modular arithmetic, primality and byte encodings are libcrypto's own.
//! Failures raise `OpenSSL::BNError` (defined in the gem's Ruby half);
//! `Integer#to_bn`, `Comparable` and `pretty_print` live there too.
//!
//! An instance is a `Mutex<BigNum>` because the bang methods (`set_bit!`)
//! mutate in place. Binary operands are COPIED out of the receiver/argument
//! before computing, so `b.mod_exp(b, b)` can never deadlock on its own
//! lock.

use super::{bin_str, str, str_bytes};
use crate::builtins::integer::int_value;
use crate::builtins::{arg_error, arity, convert, type_error};
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::{ClassId, RubyValue, Signal};
use openssl::bn::{BigNum, BigNumContext, MsbOption};
use parking_lot::Mutex;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zeo_macros::ruby_class;

pub(crate) struct RBn {
    n: Mutex<BigNum>,
    frozen: AtomicBool,
}

impl RubyObject for RBn {
    fn class_id(&self) -> ClassId {
        zeo_abi::OPENSSL_BN_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Relaxed)
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let d = RBn {
            n: Mutex::new(copy(&self.n.lock())),
            frozen: AtomicBool::new(false),
        };
        if copy_frozen {
            d.set_frozen();
        }
        Arc::new(d)
    }
}

fn bn_error_msg(msg: &str) -> Signal {
    raise_error("OpenSSL::BNError", msg.to_string())
}

fn bn_error(e: openssl::error::ErrorStack) -> Signal {
    let msg = e
        .errors()
        .first()
        .and_then(|err| err.reason().map(str::to_string))
        .unwrap_or_else(|| format!("{e}"));
    bn_error_msg(&msg)
}

fn copy(n: &BigNum) -> BigNum {
    // UFCS: plain `n.to_owned()` resolves to `ToOwned` on `&BigNum` (a
    // no-op borrow), not `BigNumRef`'s real BN_dup.
    openssl::bn::BigNumRef::to_owned(n).expect("BN_dup of a live BIGNUM cannot fail")
}

/// Run `f` with a fresh `BigNumContext`, mapping the error stack to
/// `OpenSSL::BNError`.
fn with_ctx<T>(
    f: impl FnOnce(&mut openssl::bn::BigNumContextRef) -> Result<T, openssl::error::ErrorStack>,
) -> Result<T, Signal> {
    let mut c = BigNumContext::new().map_err(bn_error)?;
    f(&mut c).map_err(bn_error)
}

fn wrap(n: BigNum) -> RubyValue {
    RubyValue::Object(Arc::new(RBn {
        n: Mutex::new(n),
        frozen: AtomicBool::new(false),
    }))
}

fn bn_of(recv: &RubyValue) -> &RBn {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RBn>()
            .expect("the OpenSSL::BN table only dispatches on BN receivers"),
        _ => unreachable!("the OpenSSL::BN table only dispatches on BN receivers"),
    }
}

/// The receiver's value, copied out so no lock is held during computation.
fn self_bn(recv: &RubyValue) -> BigNum {
    copy(&bn_of(recv).n.lock())
}

/// A BN-or-Integer operand as an owned `BigNum` -- CRuby coerces Integer
/// right-hand sides implicitly.
fn arg_bn(v: &RubyValue) -> Result<BigNum, Signal> {
    match v {
        RubyValue::Object(o) => {
            if let Some(b) = o.as_any().downcast_ref::<RBn>() {
                Ok(copy(&b.n.lock()))
            } else {
                Err(type_error!("Cannot convert into OpenSSL::BN"))
            }
        }
        RubyValue::Int(i) => BigNum::from_dec_str(&i.to_string()).map_err(bn_error),
        RubyValue::BigInt(b) => BigNum::from_dec_str(&b.to_string()).map_err(bn_error),
        _ => Err(type_error!("Cannot convert into OpenSSL::BN")),
    }
}

/// The Ruby Integer of `n` (through the canonical Int/BigInt split).
fn to_int_value(n: &BigNum) -> Result<RubyValue, Signal> {
    let dec = n.to_dec_str().map_err(bn_error)?;
    let big = num_bigint::BigInt::parse_bytes(dec.as_bytes(), 10)
        .expect("BN_bn2dec produces a valid decimal string");
    Ok(int_value(big))
}

/// `BN.new(arg, base = 10)`: an Integer, another BN, or a String in base
/// 0 (MPI), 2 (big-endian bytes), 10 or 16.
fn construct(args: &[RubyValue]) -> Result<BigNum, Signal> {
    arity!(args, 1..=2);
    match &args[0] {
        RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Object(_) => {
            if args.len() > 1 {
                return Err(arg_error!(
                    "wrong number of arguments (given 2, expected 1)"
                ));
            }
            arg_bn(&args[0])
        }
        v @ RubyValue::Str(_) => {
            let base = match args.get(1) {
                None => 10,
                Some(b) => convert::to_index(b)?,
            };
            let bytes = str_bytes(v)?;
            match base {
                10 => BigNum::from_dec_str(&String::from_utf8_lossy(&bytes)).map_err(bn_error),
                16 => BigNum::from_hex_str(&String::from_utf8_lossy(&bytes)).map_err(bn_error),
                2 => BigNum::from_slice(&bytes).map_err(bn_error),
                0 => from_mpi(&bytes),
                _ => Err(arg_error!("invalid radix {}", base)),
            }
        }
        _ => Err(type_error!("Cannot convert into OpenSSL::BN")),
    }
}

/// BN_bn2mpi: a 4-byte big-endian length, then the magnitude with a leading
/// zero byte whenever the top bit would otherwise read as the sign, which
/// carries the negative flag instead.
fn to_mpi(n: &BigNum) -> Vec<u8> {
    let mut mag = n.to_vec();
    if mag.first().is_some_and(|b| b & 0x80 != 0) {
        mag.insert(0, 0);
    }
    if n.is_negative() {
        if let Some(first) = mag.first_mut() {
            *first |= 0x80;
        }
    }
    let mut out = (mag.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(&mag);
    out
}

fn from_mpi(bytes: &[u8]) -> Result<BigNum, Signal> {
    if bytes.len() < 4 {
        return Err(bn_error_msg("invalid MPI format"));
    }
    let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let data = &bytes[4..];
    if data.len() != len {
        return Err(bn_error_msg("invalid MPI format"));
    }
    let negative = data.first().is_some_and(|b| b & 0x80 != 0);
    let mut mag = data.to_vec();
    if let Some(first) = mag.first_mut() {
        *first &= 0x7F;
    }
    let mut n = BigNum::from_slice(&mag).map_err(bn_error)?;
    if negative {
        n.set_negative(true);
    }
    Ok(n)
}

fn cmp_int(a: &BigNum, b: &BigNum) -> i64 {
    match a.partial_cmp(b) {
        Some(std::cmp::Ordering::Less) => -1,
        Some(std::cmp::Ordering::Greater) => 1,
        _ => 0,
    }
}

ruby_class! {
    BN = zeo_abi::OPENSSL_BN_CLASS < zeo_abi::OBJECT_CLASS;

    def self."new" (_recv, *args, &_block) {
        Ok(wrap(construct(args)?))
    }
    // `BN.rand(bits, fill = 0, odd = false)` -- `fill` -1 allows a zero top
    // bit, 0 forces the top bit, 1 forces the top two (BN_rand's msb knob).
    def self."rand" arity -1 (_recv, arg1, arg2?, arg3?) {
        let bits = convert::to_index(arg1)? as i32;
        let msb = match arg2 {
            None => MsbOption::MAYBE_ZERO,
            Some(v) => match convert::to_index(v)? {
                -1 => MsbOption::MAYBE_ZERO,
                0 => MsbOption::ONE,
                _ => MsbOption::TWO_ONES,
            },
        };
        let odd = matches!(arg3, Some(RubyValue::Bool(true)));
        let mut n = BigNum::new().map_err(bn_error)?;
        n.rand(bits, msb, odd).map_err(bn_error)?;
        Ok(wrap(n))
    }
    // `BN.rand_range(range)` -- uniform in [0, range).
    def self."rand_range" (_recv, arg) {
        let range = arg_bn(arg)?;
        let mut n = BigNum::new().map_err(bn_error)?;
        range.rand_range(&mut n).map_err(bn_error)?;
        Ok(wrap(n))
    }
    // `BN.generate_prime(bits = 2048, safe = false, add = nil, rem = nil)`.
    def self."generate_prime" (_recv, arg1?, arg2?, arg3?, arg4?) {
        let bits = match arg1 {
            None => 2048,
            Some(v) => convert::to_index(v)? as i32,
        };
        let safe = matches!(arg2, Some(RubyValue::Bool(true)));
        let add = match arg3 {
            None | Some(RubyValue::Nil) => None,
            Some(v) => Some(arg_bn(v)?),
        };
        let rem = match arg4 {
            None | Some(RubyValue::Nil) => None,
            Some(v) => Some(arg_bn(v)?),
        };
        let mut n = BigNum::new().map_err(bn_error)?;
        n.generate_prime(bits, safe, add.as_deref(), rem.as_deref())
            .map_err(bn_error)?;
        Ok(wrap(n))
    }

    def "to_i" | "to_int" (recv) {
        to_int_value(&bn_of(recv).n.lock())
    }
    // `to_s(base = 10)`: 10/16 are digit strings, 2 the big-endian
    // magnitude bytes, 0 the MPI encoding.
    def "to_s" (recv, arg?) {
        let base = match arg {
            None => 10,
            Some(v) => convert::to_index(v)?,
        };
        let n = self_bn(recv);
        match base {
            10 => Ok(str(n.to_dec_str().map_err(bn_error)?.to_string())),
            16 => Ok(str(n.to_hex_str().map_err(bn_error)?.to_string())),
            2 => Ok(bin_str(n.to_vec())),
            0 => Ok(bin_str(to_mpi(&n))),
            _ => Err(arg_error!("invalid radix {}", base)),
        }
    }
    def "to_bn" (recv) {
        Ok(recv.clone())
    }

    def "+" (recv, other) {
        let (a, b) = (self_bn(recv), arg_bn(other)?);
        let mut r = BigNum::new().map_err(bn_error)?;
        r.checked_add(&a, &b).map_err(bn_error)?;
        Ok(wrap(r))
    }
    def "-" (recv, other) {
        let (a, b) = (self_bn(recv), arg_bn(other)?);
        let mut r = BigNum::new().map_err(bn_error)?;
        r.checked_sub(&a, &b).map_err(bn_error)?;
        Ok(wrap(r))
    }
    def "*" (recv, other) {
        let (a, b) = (self_bn(recv), arg_bn(other)?);
        let mut r = BigNum::new().map_err(bn_error)?;
        with_ctx(|c| r.checked_mul(&a, &b, c))?;
        Ok(wrap(r))
    }
    // `/` answers BOTH halves -- `[quotient, remainder]` -- as CRuby's does.
    def "/" (recv, other) {
        let (a, b) = (self_bn(recv), arg_bn(other)?);
        let mut q = BigNum::new().map_err(bn_error)?;
        let mut r = BigNum::new().map_err(bn_error)?;
        with_ctx(|c| q.checked_div(&a, &b, c))?;
        with_ctx(|c| r.checked_rem(&a, &b, c))?;
        Ok(RubyValue::Array(crate::array_new(vec![wrap(q), wrap(r)])))
    }
    def "%" (recv, other) {
        let (a, b) = (self_bn(recv), arg_bn(other)?);
        let mut r = BigNum::new().map_err(bn_error)?;
        with_ctx(|c| r.nnmod(&a, &b, c))?;
        Ok(wrap(r))
    }
    def "**" (recv, other) {
        let (a, b) = (self_bn(recv), arg_bn(other)?);
        let mut r = BigNum::new().map_err(bn_error)?;
        with_ctx(|c| r.exp(&a, &b, c))?;
        Ok(wrap(r))
    }
    def "sqr" (recv) {
        let a = self_bn(recv);
        let mut r = BigNum::new().map_err(bn_error)?;
        with_ctx(|c| r.sqr(&a, c))?;
        Ok(wrap(r))
    }
    def "<<" (recv, other) {
        let a = self_bn(recv);
        let n = convert::to_index(other)? as i32;
        let mut r = BigNum::new().map_err(bn_error)?;
        r.lshift(&a, n).map_err(bn_error)?;
        Ok(wrap(r))
    }
    def ">>" (recv, other) {
        let a = self_bn(recv);
        let n = convert::to_index(other)? as i32;
        let mut r = BigNum::new().map_err(bn_error)?;
        r.rshift(&a, n).map_err(bn_error)?;
        Ok(wrap(r))
    }

    def "mod_exp" (recv, arg1, arg2) {
        let (a, p, m) = (self_bn(recv), arg_bn(arg1)?, arg_bn(arg2)?);
        let mut r = BigNum::new().map_err(bn_error)?;
        with_ctx(|c| r.mod_exp(&a, &p, &m, c))?;
        Ok(wrap(r))
    }
    def "mod_add" (recv, arg1, arg2) {
        let (a, b, m) = (self_bn(recv), arg_bn(arg1)?, arg_bn(arg2)?);
        let mut r = BigNum::new().map_err(bn_error)?;
        with_ctx(|c| r.mod_add(&a, &b, &m, c))?;
        Ok(wrap(r))
    }
    def "mod_sub" (recv, arg1, arg2) {
        let (a, b, m) = (self_bn(recv), arg_bn(arg1)?, arg_bn(arg2)?);
        let mut r = BigNum::new().map_err(bn_error)?;
        with_ctx(|c| r.mod_sub(&a, &b, &m, c))?;
        Ok(wrap(r))
    }
    def "mod_mul" (recv, arg1, arg2) {
        let (a, b, m) = (self_bn(recv), arg_bn(arg1)?, arg_bn(arg2)?);
        let mut r = BigNum::new().map_err(bn_error)?;
        with_ctx(|c| r.mod_mul(&a, &b, &m, c))?;
        Ok(wrap(r))
    }
    def "mod_sqr" (recv, arg) {
        let (a, m) = (self_bn(recv), arg_bn(arg)?);
        let mut r = BigNum::new().map_err(bn_error)?;
        with_ctx(|c| r.mod_sqr(&a, &m, c))?;
        Ok(wrap(r))
    }
    def "mod_inverse" (recv, arg) {
        let (a, m) = (self_bn(recv), arg_bn(arg)?);
        let mut r = BigNum::new().map_err(bn_error)?;
        with_ctx(|c| r.mod_inverse(&a, &m, c))?;
        Ok(wrap(r))
    }
    def "gcd" (recv, arg) {
        let (a, b) = (self_bn(recv), arg_bn(arg)?);
        let mut r = BigNum::new().map_err(bn_error)?;
        with_ctx(|c| r.gcd(&a, &b, c))?;
        Ok(wrap(r))
    }

    def "num_bits" (recv) {
        Ok(RubyValue::Int(bn_of(recv).n.lock().num_bits() as i64))
    }
    def "num_bytes" (recv) {
        Ok(RubyValue::Int(bn_of(recv).n.lock().num_bytes() as i64))
    }
    def "zero?" (recv) {
        Ok(RubyValue::Bool(bn_of(recv).n.lock().num_bits() == 0))
    }
    def "one?" (recv) {
        let one = BigNum::from_u32(1).map_err(bn_error)?;
        Ok(RubyValue::Bool(*bn_of(recv).n.lock() == one))
    }
    def "odd?" (recv) {
        Ok(RubyValue::Bool(bn_of(recv).n.lock().is_bit_set(0)))
    }
    def "negative?" (recv) {
        Ok(RubyValue::Bool(bn_of(recv).n.lock().is_negative()))
    }

    // `==` coerces an Integer right-hand side; `eql?` does NOT -- CRuby's
    // ossl_bn_eql demands a real BN, so `BN.new(255).eql?(255)` is false
    // where `== 255` is true.
    def "==" (recv, other) {
        let Ok(b) = arg_bn(other) else {
            return Ok(RubyValue::Bool(false));
        };
        Ok(RubyValue::Bool(self_bn(recv) == b))
    }
    def "eql?" (recv, arg) {
        let RubyValue::Object(o) = arg else {
            return Ok(RubyValue::Bool(false));
        };
        let Some(other) = o.as_any().downcast_ref::<RBn>() else {
            return Ok(RubyValue::Bool(false));
        };
        Ok(RubyValue::Bool(self_bn(recv) == copy(&other.n.lock())))
    }
    def "<=>" | "cmp" (recv, other) {
        let b = arg_bn(other)?;
        Ok(RubyValue::Int(cmp_int(&self_bn(recv), &b)))
    }
    def "ucmp" (recv, arg) {
        let (mut a, mut b) = (self_bn(recv), arg_bn(arg)?);
        a.set_negative(false);
        b.set_negative(false);
        Ok(RubyValue::Int(cmp_int(&a, &b)))
    }
    // Integer's `coerce` half: hand arithmetic between an Integer LHS and a
    // BN back to Integer math (`5 + bn` answers an Integer, as CRuby's does).
    def "coerce" (recv, arg) {
        match arg {
            v @ (RubyValue::Int(_) | RubyValue::BigInt(_)) => Ok(RubyValue::Array(
                crate::array_new(vec![v.clone(), to_int_value(&bn_of(recv).n.lock())?]),
            )),
            _ => Err(type_error!("Don't know how to coerce")),
        }
    }
    def "hash" (recv) {
        let n = bn_of(recv).n.lock();
        let mut h = std::hash::DefaultHasher::new();
        n.to_vec().hash(&mut h);
        n.is_negative().hash(&mut h);
        Ok(RubyValue::Int(h.finish() as i64))
    }

    // `prime?(checks = nil)` -- BN_is_prime_ex; 0 checks means the
    // bit-length-derived default, CRuby's own nil.
    def "prime?" (recv, arg?) {
        let checks = match arg {
            None | Some(RubyValue::Nil) => 0,
            Some(v) => convert::to_index(v)? as i32,
        };
        let n = self_bn(recv);
        Ok(RubyValue::Bool(with_ctx(|c| n.is_prime(checks, c))?))
    }
    def "bit_set?" (recv, arg) {
        let i = convert::to_index(arg)? as i32;
        Ok(RubyValue::Bool(bn_of(recv).n.lock().is_bit_set(i)))
    }
    def "set_bit!" (recv, arg) {
        let i = convert::to_index(arg)? as i32;
        bn_of(recv).n.lock().set_bit(i).map_err(bn_error)?;
        Ok(recv.clone())
    }
    def "clear_bit!" (recv, arg) {
        let i = convert::to_index(arg)? as i32;
        bn_of(recv).n.lock().clear_bit(i).map_err(bn_error)?;
        Ok(recv.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::registered_table;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(crate::string_new(text.to_string()))
    }
    fn im(name: &str) -> crate::builtins::BuiltinMethodFn {
        let table = registered_table(zeo_abi::OPENSSL_BN_CLASS)
            .and_then(|t| t.instance.as_ref())
            .expect("BN registers an instance table");
        (table.lookup)(name).expect("instance method exists")
    }
    fn cm(name: &str) -> crate::builtins::BuiltinMethodFn {
        let table = registered_table(zeo_abi::OPENSSL_BN_CLASS)
            .and_then(|t| t.class.as_ref())
            .expect("BN registers a class table");
        (table.lookup)(name).expect("class method exists")
    }
    fn bn(v: i64) -> RubyValue {
        cm("new")(&RubyValue::Nil, &[RubyValue::Int(v)], None).unwrap()
    }
    fn int(v: Result<RubyValue, Signal>) -> i64 {
        match v.unwrap() {
            RubyValue::Int(i) => i,
            other => panic!("expected Int, got {other:?}"),
        }
    }
    fn t(v: Result<RubyValue, Signal>) -> String {
        match v.unwrap() {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn construction_bases_match_ruby() {
        assert_eq!(int(im("to_i")(&bn(255), &[], None)), 255);
        let from_dec = cm("new")(&RubyValue::Nil, &[s("255")], None).unwrap();
        assert_eq!(int(im("to_i")(&from_dec, &[], None)), 255);
        let from_hex = cm("new")(&RubyValue::Nil, &[s("ff"), RubyValue::Int(16)], None).unwrap();
        assert_eq!(int(im("to_i")(&from_hex, &[], None)), 255);
        assert_eq!(int(im("to_i")(&bn(-42), &[], None)), -42);
    }

    #[test]
    fn string_encodings_match_ruby() {
        let b = bn(255);
        assert_eq!(t(im("to_s")(&b, &[], None)), "255");
        assert_eq!(t(im("to_s")(&b, &[RubyValue::Int(16)], None)), "FF");
        // ruby 4.0.6: BN.new(65537).to_s(0).unpack1("H*") == "00000003010001".
        let mpi = im("to_s")(&bn(65537), &[RubyValue::Int(0)], None).unwrap();
        let RubyValue::Str(mpi) = mpi else {
            panic!("expected Str")
        };
        assert_eq!(super::super::hex(mpi.lock().bytes()), "00000003010001");
        // And back in through base 0.
        let round = cm("new")(
            &RubyValue::Nil,
            &[RubyValue::Str(mpi), RubyValue::Int(0)],
            None,
        )
        .unwrap();
        assert_eq!(int(im("to_i")(&round, &[], None)), 65537);
    }

    #[test]
    fn arithmetic_matches_ruby() {
        let b = bn(255);
        let plus = im("+")(&b, &[bn(1)], None).unwrap();
        assert_eq!(int(im("to_i")(&plus, &[], None)), 256);
        // Integer right-hand sides coerce.
        let minus = im("-")(&b, &[RubyValue::Int(5)], None).unwrap();
        assert_eq!(int(im("to_i")(&minus, &[], None)), 250);
        let div = im("/")(&b, &[bn(4)], None).unwrap();
        let RubyValue::Array(pair) = div else {
            panic!("expected [q, r]")
        };
        assert_eq!(int(im("to_i")(&crate::array_get(&pair, 0), &[], None)), 63);
        assert_eq!(int(im("to_i")(&crate::array_get(&pair, 1), &[], None)), 3);
        assert_eq!(
            int(im("to_i")(&im("%")(&b, &[bn(7)], None).unwrap(), &[], None)),
            3
        );
        let pow = im("**")(&bn(3), &[bn(4)], None).unwrap();
        assert_eq!(int(im("to_i")(&pow, &[], None)), 81);
        let modexp = im("mod_exp")(&b, &[bn(3), bn(100)], None).unwrap();
        assert_eq!(int(im("to_i")(&modexp, &[], None)), 75);
        let inv = im("mod_inverse")(&b, &[bn(7)], None).unwrap();
        assert_eq!(int(im("to_i")(&inv, &[], None)), 5);
        let gcd = im("gcd")(&b, &[bn(100)], None).unwrap();
        assert_eq!(int(im("to_i")(&gcd, &[], None)), 5);
        let shifted = im("<<")(&b, &[RubyValue::Int(4)], None).unwrap();
        assert_eq!(int(im("to_i")(&shifted, &[], None)), 4080);
    }

    #[test]
    fn predicates_and_comparison_match_ruby() {
        let b = bn(255);
        assert_eq!(int(im("num_bits")(&b, &[], None)), 8);
        assert_eq!(int(im("num_bytes")(&b, &[], None)), 1);
        assert!(matches!(
            im("odd?")(&b, &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
        assert!(matches!(
            im("zero?")(&bn(0), &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
        assert!(matches!(
            im("==")(&b, &[RubyValue::Int(255)], None).unwrap(),
            RubyValue::Bool(true)
        ));
        assert_eq!(int(im("<=>")(&b, &[bn(300)], None)), -1);
        // ucmp compares magnitudes.
        assert_eq!(int(im("ucmp")(&b, &[bn(-300)], None)), -1);
        assert!(matches!(
            im("prime?")(&bn(97), &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
        assert!(matches!(
            im("prime?")(&bn(100), &[], None).unwrap(),
            RubyValue::Bool(false)
        ));
        assert!(matches!(
            im("bit_set?")(&b, &[RubyValue::Int(0)], None).unwrap(),
            RubyValue::Bool(true)
        ));
        let c = bn(255);
        im("set_bit!")(&c, &[RubyValue::Int(8)], None).unwrap();
        assert_eq!(int(im("to_i")(&c, &[], None)), 511);
        assert_eq!(
            int(im("hash")(&bn(255), &[], None)),
            int(im("hash")(&bn(255), &[], None))
        );
    }

    #[test]
    fn bigint_scale_round_trips() {
        let two = bn(2);
        let big = im("**")(&two, &[bn(130)], None).unwrap();
        let dec = t(im("to_s")(&big, &[], None));
        assert_eq!(dec, "1361129467683753853853498429727072845824");
        let back = cm("new")(&RubyValue::Nil, &[s(&dec)], None).unwrap();
        assert!(matches!(
            im("==")(&big, &[back], None).unwrap(),
            RubyValue::Bool(true)
        ));
        // to_i produces a real Integer (BigInt) with the same digits.
        match im("to_i")(&big, &[], None).unwrap() {
            RubyValue::BigInt(b) => assert_eq!(b.to_string(), dec),
            other => panic!("expected a BigInt, got {other:?}"),
        }
    }

    #[test]
    fn rand_and_generate_prime_hold_their_contracts() {
        let r = cm("rand")(&RubyValue::Nil, &[RubyValue::Int(64)], None).unwrap();
        assert!(int(im("num_bits")(&r, &[], None)) <= 64);
        let p = cm("generate_prime")(&RubyValue::Nil, &[RubyValue::Int(32)], None).unwrap();
        assert!(matches!(
            im("prime?")(&p, &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
        let range = bn(100);
        let rr = cm("rand_range")(&RubyValue::Nil, &[range], None).unwrap();
        assert_eq!(int(im("<=>")(&rr, &[bn(100)], None)), -1);
    }
}

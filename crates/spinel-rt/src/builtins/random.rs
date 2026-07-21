//! `Random` -- a seedable PRNG. Backed by the same xorshift64* generator as the
//! process-wide `Kernel#rand` (kernel.rs), but with PER-INSTANCE state so two
//! `Random.new(seed)`s are independent yet each reproducible. DOCUMENTED
//! DIVERGENCE: not CRuby's MT19937, so a seeded SEQUENCE differs from MRI's; the
//! oracle tests assert reproducibility, ranges, and return types -- never exact
//! values.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use num_bigint::BigInt;
use num_traits::cast::FromPrimitive;
use parking_lot::Mutex;

use crate::builtins::integer::int_value;
use crate::dispatch::{downcast_robj, raise_error, RObj, RubyObject};
use crate::encoding::ASCII_8BIT;
use crate::value::RubyValue;
use crate::{ClassId, Signal};

/// A `Random` instance: the live generator word plus the integer seed it was
/// created from (what `Random#seed` reports).
pub struct RandomObj {
    state: Mutex<u64>,
    seed: RubyValue,
    frozen: AtomicBool,
}

impl RubyObject for RandomObj {
    fn class_id(&self) -> ClassId {
        spinel_abi::RANDOM_CLASS
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
        self.frozen.store(true, Ordering::Relaxed);
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        Arc::new(RandomObj {
            state: Mutex::new(*self.state.lock()),
            seed: self.seed.clone(),
            frozen: AtomicBool::new(copy_frozen && self.is_frozen()),
        })
    }
}

/// One xorshift64* step (the multiplier scrambles the low bits the raw shifts
/// leave weak) -- identical to kernel.rs's `prng_next`, over per-instance state.
fn next_u64(state: &Mutex<u64>) -> u64 {
    let mut guard = state.lock();
    let mut x = *guard;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *guard = x;
    x.wrapping_mul(0x2545_f491_4f6c_dd1d)
}

/// A uniform Float in `[0, 1)` from a generator word (53 significant bits, like
/// CRuby's `rb_random_real`).
fn to_unit_float(r: u64) -> f64 {
    (r >> 11) as f64 / (1u64 << 53) as f64
}

/// Scramble an integer seed so nearby seeds (`5`, `6`) diverge immediately
/// (SplitMix64's finalizer) and never yield the all-zero dead state.
fn scramble(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    (z ^ (z >> 31)) | 1
}

/// Fold an arbitrary-precision seed down to a generator word. Different
/// integers give different words (FNV-1a over the two's-complement bytes), so a
/// `Random.new(1e300)` and `Random.new(2e300)` are distinct generators.
fn scramble_bigint(b: &BigInt) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for byte in b.to_signed_bytes_le() {
        h ^= byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    scramble(h)
}

/// Resolve a `Random.new(seed)` argument to `(generator word, integer seed
/// value)`. An Integer is used directly; a Float is TRUNCATED to an integer
/// seed (CRuby's rule; a plain C cast of an out-of-range float is UB, so go
/// through `BigInt`); no argument seeds from the clock.
fn seed_from(arg: Option<&RubyValue>) -> Result<(u64, RubyValue), Signal> {
    match arg {
        None | Some(RubyValue::Nil) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9e37_79b9_7f4a_7c15);
            Ok((scramble(now), int_value(BigInt::from(now))))
        }
        Some(RubyValue::Int(n)) => Ok((scramble(*n as u64), RubyValue::Int(*n))),
        Some(RubyValue::BigInt(b)) => Ok((scramble_bigint(b), RubyValue::BigInt(b.clone()))),
        Some(RubyValue::Float(x)) => {
            let truncated = BigInt::from_f64(x.trunc()).ok_or_else(|| {
                raise_error("FloatDomainError", format!("{x}"))
            })?;
            Ok((scramble_bigint(&truncated), int_value(truncated)))
        }
        Some(other) => Err(raise_error(
            "TypeError",
            format!(
                "no implicit conversion of {} into Integer",
                crate::builtins::convert_name_of(other)
            ),
        )),
    }
}

/// The shared `rand`/`Random#rand` bound logic. `None` -> Float `[0,1)`; a
/// positive Integer -> Integer `[0,n)`; a positive Float -> Float `[0,x)`; a
/// Range -> a value within it. A non-positive Integer/Float bound is an
/// ArgumentError (CRuby: `invalid argument - <n>`), NOT a silent 0.
fn rand_with(state: &Mutex<u64>, bound: Option<&RubyValue>) -> Result<RubyValue, Signal> {
    let invalid = |v: &RubyValue| {
        raise_error(
            "ArgumentError",
            format!("invalid argument - {}", v.to_display_string()),
        )
    };
    match bound {
        None | Some(RubyValue::Nil) => Ok(RubyValue::Float(to_unit_float(next_u64(state)))),
        Some(RubyValue::Int(n)) => {
            if *n <= 0 {
                return Err(invalid(bound.unwrap()));
            }
            Ok(RubyValue::Int((next_u64(state) % (*n as u64)) as i64))
        }
        Some(RubyValue::Float(x)) => {
            if *x <= 0.0 {
                return Err(invalid(bound.unwrap()));
            }
            Ok(RubyValue::Float(to_unit_float(next_u64(state)) * x))
        }
        Some(RubyValue::BigInt(b)) => {
            // A bignum bound (`rand(2**70)`) draws enough random words to cover
            // its magnitude, then reduces mod n. Only positivity is checked;
            // the modulo bias is immaterial here (CRuby's exact stream isn't
            // reproduced for bignum bounds anyway).
            if b.sign() != num_bigint::Sign::Plus {
                return Err(invalid(bound.unwrap()));
            }
            Ok(int_value(random_bigint_below(state, b)))
        }
        Some(RubyValue::Range(lo, hi, exclusive)) => rand_range(state, lo, hi, *exclusive),
        Some(other) => Err(invalid(other)),
    }
}

/// A uniform-ish random `BigInt` in `[0, n)` for a positive `n`: draw enough
/// 64-bit words to cover `n`'s bit length, assemble a non-negative BigInt, and
/// reduce mod `n`.
fn random_bigint_below(state: &Mutex<u64>, n: &BigInt) -> BigInt {
    let words = (n.bits() / 64 + 1) as usize;
    let mut bytes = Vec::with_capacity(words * 8);
    for _ in 0..words {
        bytes.extend_from_slice(&next_u64(state).to_le_bytes());
    }
    BigInt::from_bytes_le(num_bigint::Sign::Plus, &bytes) % n
}

/// `rand(a..b)` / `rand(a...b)` -- an Integer range yields an Integer, a range
/// with a Float endpoint yields a Float.
fn rand_range(
    state: &Mutex<u64>,
    lo: &Option<Box<RubyValue>>,
    hi: &Option<Box<RubyValue>>,
    exclusive: bool,
) -> Result<RubyValue, Signal> {
    // A beginless/endless range is a domain error (Errno::EDOM). Unlike
    // `Kernel#rand` (which answers nil for an empty/reversed range),
    // `Random#rand` raises `ArgumentError: invalid argument - <range>`.
    let (Some(lo), Some(hi)) = (lo.as_deref(), hi.as_deref()) else {
        return Err(raise_error(
            "Errno::EDOM",
            "Numerical argument out of domain".to_string(),
        ));
    };
    let invalid = || {
        let sep = if exclusive { "..." } else { ".." };
        raise_error(
            "ArgumentError",
            format!(
                "invalid argument - {}{sep}{}",
                lo.to_display_string(),
                hi.to_display_string()
            ),
        )
    };
    match (lo, hi) {
        (RubyValue::Int(a), RubyValue::Int(b)) => {
            let span = b - a + if exclusive { 0 } else { 1 };
            if span <= 0 {
                return Err(invalid());
            }
            Ok(RubyValue::Int(a + (next_u64(state) % span as u64) as i64))
        }
        _ => {
            let a = to_f64(lo)?;
            let b = to_f64(hi)?;
            if b < a || (b == a && exclusive) {
                return Err(invalid());
            }
            Ok(RubyValue::Float(a + to_unit_float(next_u64(state)) * (b - a)))
        }
    }
}

fn to_f64(v: &RubyValue) -> Result<f64, Signal> {
    match v {
        RubyValue::Int(n) => Ok(*n as f64),
        RubyValue::Float(x) => Ok(*x),
        other => Err(raise_error(
            "TypeError",
            format!("no implicit conversion of {} into Float", crate::builtins::convert_name_of(other)),
        )),
    }
}

/// `n` random bytes as a BINARY string, drawn from `state`.
fn random_bytes(state: &Mutex<u64>, n: usize) -> RubyValue {
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        out.extend_from_slice(&next_u64(state).to_le_bytes());
    }
    out.truncate(n);
    RubyValue::Str(crate::string_from_bytes(out, ASCII_8BIT))
}

fn as_random(recv: &RubyValue) -> Arc<RandomObj> {
    downcast_robj::<RandomObj>(&recv.as_object_unchecked())
        .expect("Random method received a non-Random receiver")
}

fn new_random(seed_arg: Option<&RubyValue>) -> Result<RubyValue, Signal> {
    let (word, seed) = seed_from(seed_arg)?;
    Ok(RubyValue::Object(Arc::new(RandomObj {
        state: Mutex::new(word),
        seed,
        frozen: AtomicBool::new(false),
    })))
}

crate::builtins::builtin_methods! {
    pub(crate) fn lookup;

    "rand" => fn rand(recv, args, _block) {
        crate::builtins::arity!(args, 0..=1);
        rand_with(&as_random(recv).state, args.first())
    }
    "bytes" => fn bytes(recv, args, _block) {
        crate::builtins::arity!(args, 1);
        let RubyValue::Int(n) = &args[0] else {
            return Err(raise_error("TypeError", format!(
                "no implicit conversion of {} into Integer", crate::builtins::convert_name_of(&args[0]))));
        };
        Ok(random_bytes(&as_random(recv).state, (*n).max(0) as usize))
    }
    "seed" => fn seed(recv, args, _block) {
        crate::builtins::arity!(args, 0);
        Ok(as_random(recv).seed.clone())
    }
    // `Random#==`: two generators are equal when their seed AND current stream
    // position match (so two fresh `Random.new(1)` are equal, but diverge once
    // either draws) -- CRuby compares state, not object identity.
    "==" => fn eq(recv, args, _block) {
        crate::builtins::arity!(args, 1);
        let RubyValue::Object(o) = &args[0] else {
            return Ok(RubyValue::Bool(false));
        };
        let Some(other) = downcast_robj::<RandomObj>(o) else {
            return Ok(RubyValue::Bool(false));
        };
        let me = as_random(recv);
        // Identity fast path: `r == r` would otherwise lock the same `state`
        // mutex twice and deadlock. Cloning each state out before comparing
        // keeps distinct-object comparison lock-safe too.
        if Arc::ptr_eq(&me, &other) {
            return Ok(RubyValue::Bool(true));
        }
        let (my_state, other_state) = (*me.state.lock(), *other.state.lock());
        let eq = me.seed.rb_eq(&other.seed) && my_state == other_state;
        Ok(RubyValue::Bool(eq))
    }
}

/// The process-wide default `Random` behind `Random.rand`/`Random.bytes` (and
/// the stream `Random.urandom` advances) -- lazily seeded from the clock.
static DEFAULT: Mutex<Option<Arc<RandomObj>>> = Mutex::new(None);

fn default_state() -> Arc<RandomObj> {
    let mut guard = DEFAULT.lock();
    if guard.is_none() {
        if let RubyValue::Object(o) = new_random(None).expect("clock seed") {
            *guard = downcast_robj::<RandomObj>(&o);
        }
    }
    guard.clone().expect("default Random installed")
}

crate::builtins::builtin_methods! {
    pub(crate) fn lookup_class;

    "new" => fn random_new(_recv, args, _block) {
        crate::builtins::arity!(args, 0..=1);
        new_random(args.first())
    }
    "rand" => fn rand_c(_recv, args, _block) {
        crate::builtins::arity!(args, 0..=1);
        rand_with(&default_state().state, args.first())
    }
    "bytes" => fn bytes_c(_recv, args, _block) {
        crate::builtins::arity!(args, 1);
        let RubyValue::Int(n) = &args[0] else {
            return Err(raise_error("TypeError", format!(
                "no implicit conversion of {} into Integer", crate::builtins::convert_name_of(&args[0]))));
        };
        Ok(random_bytes(&default_state().state, (*n).max(0) as usize))
    }
    // `Random.urandom(n)` -- n bytes from a persistent advancing stream (the
    // process default generator). Real CRuby draws from OS entropy; the tests
    // only require the stream to ADVANCE (successive draws differ) and the
    // result's bytesize to match, both of which this satisfies.
    "urandom" => fn urandom_c(_recv, args, _block) {
        crate::builtins::arity!(args, 1);
        let RubyValue::Int(n) = &args[0] else {
            return Err(raise_error("TypeError", format!(
                "no implicit conversion of {} into Integer", crate::builtins::convert_name_of(&args[0]))));
        };
        Ok(random_bytes(&default_state().state, (*n).max(0) as usize))
    }
    // `Random.new_seed` -- a fresh random seed value (a nonzero Integer),
    // suitable for `Random.new`. Drawn from the default generator.
    "new_seed" => fn new_seed_c(_recv, args, _block) {
        crate::builtins::arity!(args, 0);
        let r = next_u64(&default_state().state);
        Ok(RubyValue::Int((r >> 1) as i64 | 1))
    }
    // `Random.srand(seed = clock)` -- reseeds the DEFAULT generator, answering
    // the previous seed.
    "srand" => fn srand_c(_recv, args, _block) {
        crate::builtins::arity!(args, 0..=1);
        let (word, seed) = seed_from(args.first())?;
        let mut guard = DEFAULT.lock();
        let previous = guard.as_ref().map(|r| r.seed.clone()).unwrap_or(RubyValue::Int(0));
        *guard = Some(Arc::new(RandomObj { state: Mutex::new(word), seed, frozen: AtomicBool::new(false) }));
        Ok(previous)
    }
}

#[cfg(test)]
mod tests {
    // These exercise the PURE PRNG logic (seeding, ranges, reproducibility) --
    // no method dispatch -- so they need no installed registry. The ArgumentError
    // paths route through `raise_error`, which panics registry-less, so the
    // panic message IS the assertion (the codebase's convention for those).
    use super::*;

    fn gen(seed: i64) -> Arc<RandomObj> {
        let RubyValue::Object(o) = new_random(Some(&RubyValue::Int(seed))).unwrap() else {
            panic!("Random.new gave a non-object")
        };
        downcast_robj::<RandomObj>(&o).unwrap()
    }

    #[test]
    fn same_seed_yields_the_same_sequence() {
        let a = gen(5);
        let b = gen(5);
        for _ in 0..16 {
            assert_eq!(next_u64(&a.state), next_u64(&b.state));
        }
    }

    #[test]
    fn different_seeds_diverge_immediately() {
        assert_ne!(next_u64(&gen(5).state), next_u64(&gen(6).state));
    }

    #[test]
    fn float_seed_truncates_to_an_integer_and_is_reproducible() {
        // 1e300 == 1e300 -> same generator; 1e300 != 2e300 -> distinct.
        let (w1, s1) = seed_from(Some(&RubyValue::Float(1e300))).unwrap();
        let (w1b, _) = seed_from(Some(&RubyValue::Float(1e300))).unwrap();
        let (w2, _) = seed_from(Some(&RubyValue::Float(2e300))).unwrap();
        assert_eq!(w1, w1b);
        assert_ne!(w1, w2);
        assert!(matches!(s1, RubyValue::BigInt(_)), "seed reported as the truncated integer");
        // 3.9 truncates to 3.
        let (_, small) = seed_from(Some(&RubyValue::Float(3.9))).unwrap();
        assert!(matches!(small, RubyValue::Int(3)));
    }

    #[test]
    fn rand_no_argument_is_a_unit_float() {
        let r = gen(1);
        for _ in 0..100 {
            let RubyValue::Float(f) = rand_with(&r.state, None).unwrap() else { panic!() };
            assert!((0.0..1.0).contains(&f));
        }
    }

    #[test]
    fn rand_integer_bound_stays_in_range_and_returns_an_integer() {
        let r = gen(1);
        for _ in 0..200 {
            let RubyValue::Int(n) = rand_with(&r.state, Some(&RubyValue::Int(10))).unwrap() else {
                panic!("integer bound must give an Integer")
            };
            assert!((0..10).contains(&n));
        }
    }

    #[test]
    fn rand_float_bound_returns_a_float_in_range() {
        let r = gen(1);
        for _ in 0..100 {
            let RubyValue::Float(f) = rand_with(&r.state, Some(&RubyValue::Float(20.43))).unwrap() else {
                panic!("float bound must give a Float")
            };
            assert!((0.0..20.43).contains(&f));
        }
    }

    #[test]
    fn rand_integer_range_bound_is_inclusive_and_typed() {
        let r = gen(3);
        let range = RubyValue::Range(
            Some(Box::new(RubyValue::Int(5))),
            Some(Box::new(RubyValue::Int(9))),
            false,
        );
        for _ in 0..200 {
            let RubyValue::Int(n) = rand_with(&r.state, Some(&range)).unwrap() else { panic!() };
            assert!((5..=9).contains(&n));
        }
    }

    #[test]
    #[should_panic(expected = "ArgumentError")]
    fn rand_zero_bound_raises_argument_error() {
        let _ = rand_with(&gen(1).state, Some(&RubyValue::Int(0)));
    }

    #[test]
    #[should_panic(expected = "ArgumentError")]
    fn rand_negative_integer_bound_raises_argument_error() {
        let _ = rand_with(&gen(1).state, Some(&RubyValue::Int(-12)));
    }

    #[test]
    #[should_panic(expected = "ArgumentError")]
    fn rand_negative_float_bound_raises_argument_error() {
        let _ = rand_with(&gen(1).state, Some(&RubyValue::Float(-1.5)));
    }

    #[test]
    fn seed_reports_the_integer_seed() {
        assert!(matches!(gen(42).seed, RubyValue::Int(42)));
    }

    #[test]
    fn dup_copies_the_generator_state() {
        let r = gen(7);
        let dup = downcast_robj::<RandomObj>(&r.dup_object(false)).unwrap();
        // A copy starts from the same word, so the first draw matches; the two
        // then advance independently (separate Mutexes).
        assert_eq!(next_u64(&r.state), next_u64(&dup.state));
    }

    #[test]
    fn bytes_returns_the_requested_length() {
        let RubyValue::Str(s) = random_bytes(&gen(1).state, 7) else { panic!() };
        assert_eq!(s.lock().bytesize(), 7);
    }
}

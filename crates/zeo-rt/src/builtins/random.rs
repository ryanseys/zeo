//! `Random` -- a seedable PRNG. MT19937 (see `crate::mt`), read through ruby's
//! own draw primitives, with PER-INSTANCE state so two `Random.new(seed)`s are
//! independent yet each reproducible -- and each reproduces ruby's sequence for
//! that seed, number for number.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zeo_macros::ruby_class;

use num_bigint::BigInt;
use num_traits::cast::FromPrimitive;
use parking_lot::Mutex;

use crate::builtins::integer::int_value;
use crate::builtins::{arg_error, float_domain_error, type_error};
use crate::dispatch::{RObj, RubyObject, downcast_robj, raise_error};
use crate::encoding::ASCII_8BIT;
use crate::value::RubyValue;
use crate::{ClassId, Signal};

/// A `Random` instance: the live generator word plus the integer seed it was
/// created from (what `Random#seed` reports).
pub struct RandomObj {
    state: Mutex<crate::mt::Mt>,
    seed: RubyValue,
    frozen: AtomicBool,
}

impl RubyObject for RandomObj {
    fn class_id(&self) -> ClassId {
        zeo_abi::RANDOM_CLASS
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
            state: Mutex::new(self.state.lock().clone()),
            seed: self.seed.clone(),
            frozen: AtomicBool::new(copy_frozen && self.is_frozen()),
        })
    }
}

/// Resolve a `Random.new(seed)` argument to `(generator, integer seed value)`.
/// An Integer seeds by its magnitude; a Float is TRUNCATED to an integer seed
/// (ruby's rule; a plain cast of an out-of-range float is UB, so go through
/// `BigInt`); no argument seeds from the clock.
fn seed_from(arg: Option<&RubyValue>) -> Result<(crate::mt::Mt, RubyValue), Signal> {
    let from_big = |b: BigInt| (crate::mt::Mt::from_bigint(&b), int_value(b));
    Ok(match arg {
        None | Some(RubyValue::Nil) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9e37_79b9_7f4a_7c15);
            from_big(BigInt::from(now))
        }
        Some(RubyValue::Int(n)) => from_big(BigInt::from(*n)),
        Some(RubyValue::BigInt(b)) => from_big((**b).clone()),
        Some(RubyValue::Float(x)) => {
            from_big(BigInt::from_f64(x.trunc()).ok_or_else(|| float_domain_error!("{x}"))?)
        }
        Some(other) => match crate::builtins::convert::to_int(other)? {
            RubyValue::Int(n) => from_big(BigInt::from(n)),
            RubyValue::BigInt(b) => from_big((*b).clone()),
            _ => unreachable!("to_int post-checks its answer"),
        },
    })
}

/// The shared `rand`/`Random#rand` bound logic. `None` -> Float `[0,1)`; a
/// positive Integer -> Integer `[0,n)`; a positive Float -> Float `[0,x)`; a
/// Range -> a value within it. A non-positive Integer/Float bound is an
/// ArgumentError (CRuby: `invalid argument - <n>`), NOT a silent 0.
/// `Random#rand`'s domain error for a non-finite Float bound or endpoint.
fn edom() -> Signal {
    raise_error(
        "Errno::EDOM",
        "Numerical argument out of domain".to_string(),
    )
}

fn rand_with(state: &Mutex<crate::mt::Mt>, bound: Option<&RubyValue>) -> Result<RubyValue, Signal> {
    let invalid = |v: &RubyValue| arg_error!("invalid argument - {}", v.to_display_string());
    match bound {
        None | Some(RubyValue::Nil) => Ok(RubyValue::Float(state.lock().next_real())),
        Some(RubyValue::Int(n)) => {
            if *n <= 0 {
                return Err(invalid(bound.unwrap()));
            }
            Ok(RubyValue::Int(state.lock().limited(*n as u64 - 1) as i64))
        }
        Some(RubyValue::Float(x)) => {
            // Unlike `Kernel#rand` (FloatDomainError), `Random#rand`/`Random.rand`
            // raise Errno::EDOM for a non-finite Float bound.
            if !x.is_finite() {
                return Err(edom());
            }
            if *x < 0.0 {
                return Err(invalid(bound.unwrap()));
            }
            // `rand(0.0)` behaves like the no-argument draw ([0.0, 1.0) unit
            // float), same as `rand(0)`; only a negative bound is invalid.
            if *x == 0.0 {
                return Ok(RubyValue::Float(state.lock().next_real()));
            }
            Ok(RubyValue::Float(state.lock().next_real() * x))
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
fn random_bigint_below(state: &Mutex<crate::mt::Mt>, n: &BigInt) -> BigInt {
    state.lock().bigint_below(n)
}

/// `rand(a..b)` / `rand(a...b)` -- an Integer range yields an Integer, a range
/// with a Float endpoint yields a Float.
fn rand_range(
    state: &Mutex<crate::mt::Mt>,
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
        arg_error!(
            "invalid argument - {}{sep}{}",
            lo.to_display_string(),
            hi.to_display_string()
        )
    };
    match (lo, hi) {
        (RubyValue::Int(a), RubyValue::Int(b)) => {
            let span = b - a + if exclusive { 0 } else { 1 };
            if span <= 0 {
                return Err(invalid());
            }
            Ok(RubyValue::Int(
                a + state.lock().limited(span as u64 - 1) as i64,
            ))
        }
        _ => {
            let a = to_f64(lo)?;
            let b = to_f64(hi)?;
            if !a.is_finite() || !b.is_finite() {
                return Err(edom());
            }
            if b < a || (b == a && exclusive) {
                return Err(invalid());
            }
            // An INCLUSIVE range must be able to answer its own endpoint, so
            // ruby draws it through a `[0, 1]` unit rather than `[0, 1)`.
            let unit = if exclusive {
                state.lock().next_real()
            } else {
                state.lock().next_real_inclusive()
            };
            Ok(RubyValue::Float(a + unit * (b - a)))
        }
    }
}

fn to_f64(v: &RubyValue) -> Result<f64, Signal> {
    match v {
        RubyValue::Int(n) => Ok(*n as f64),
        RubyValue::Float(x) => Ok(*x),
        // CRuby's rand converts an unknown bound via `to_int` (oracle:
        // `rand("x")` is "no implicit conversion of String into Integer").
        other => Ok(crate::builtins::convert::to_index(other)? as f64),
    }
}

/// A byte-count argument (`bytes`/`urandom`): the `to_int` protocol with
/// the generic "of nil into Integer" nil shape (oracle-verified).
fn bytes_count(v: &RubyValue) -> Result<i64, Signal> {
    match v {
        RubyValue::Nil => Err(type_error!("no implicit conversion of nil into Integer")),
        v => crate::builtins::convert::to_index(v),
    }
}

/// `n` random bytes as a BINARY string, drawn from `state`.
fn random_bytes(state: &Mutex<crate::mt::Mt>, n: usize) -> RubyValue {
    RubyValue::Str(crate::string_from_bytes(state.lock().bytes(n), ASCII_8BIT))
}

/// `n` bytes from the OS cryptographic entropy source (getrandom(2) /
/// SecRandomCopyBytes), as a BINARY string. Backs `Random.urandom` and,
/// through it, SecureRandom. Gvl-released because the pool can block right
/// after boot. A CRuby `RuntimeError` on failure matches `Random.urandom`'s
/// "no random device" surface (securerandom.rb rescues exactly that).
fn os_urandom(n: usize) -> Result<RubyValue, Signal> {
    let mut buf = vec![0u8; n];
    crate::gvl::without_gvl(|| getrandom::fill(&mut buf))
        .map_err(|e| raise_error("RuntimeError", format!("failed to read random device: {e}")))?;
    Ok(RubyValue::Str(crate::string_from_bytes(buf, ASCII_8BIT)))
}

fn as_random(recv: &RubyValue) -> Arc<RandomObj> {
    downcast_robj::<RandomObj>(&recv.as_object_unchecked())
        .expect("Random method received a non-Random receiver")
}

fn new_random(seed_arg: Option<&RubyValue>) -> Result<RubyValue, Signal> {
    let (mt, seed) = seed_from(seed_arg)?;
    Ok(RubyValue::Object(Arc::new(RandomObj {
        state: Mutex::new(mt),
        seed,
        frozen: AtomicBool::new(false),
    })))
}

ruby_class! {
    Random = zeo_abi::RANDOM_CLASS < zeo_abi::OBJECT_CLASS;

    def self."new"(_recv, arg?) {
        new_random(arg)
    }
    def self."rand"(_recv, arg?) {
        rand_with(&default_state().state, arg)
    }
    def self."bytes"(_recv, arg) {
        let n = &bytes_count(arg)?;
        if *n < 0 {
            return Err(arg_error!("negative string size (or size too big)"));
        }
        Ok(random_bytes(&default_state().state, *n as usize))
    }
    // `Random.urandom(n)` -- n bytes drawn from the OS CSPRNG (getrandom(2) /
    // SecRandomCopyBytes), NOT the seedable PRNG. This is the entropy source
    // SecureRandom sits on, so it must be genuinely cryptographic; a seeded
    // generator would make SecureRandom predictable.
    def self."urandom"(_recv, arg) {
        let n = &bytes_count(arg)?;
        if *n < 0 {
            return Err(arg_error!("negative string size (or size too big)"));
        }
        os_urandom(*n as usize)
    }
    // `Random.new_seed` -- a fresh random seed value (a nonzero Integer),
    // suitable for `Random.new`. Drawn from the default generator.
    // `Random.seed` -- the seed the DEFAULT generator is running from, the
    // same value `Random.srand` answers when it replaces one.
    def self."seed"(_recv) {
        Ok(DEFAULT
            .lock()
            .as_ref()
            .map(|r| r.seed.clone())
            .unwrap_or(RubyValue::Int(0)))
    }
    def self."new_seed"(_recv) {
        let r = default_state().state.lock().limited(u64::from(u32::MAX));
        Ok(RubyValue::Int(r as i64 | 1))
    }
    // `Random.srand(seed = clock)` -- reseeds the DEFAULT generator, answering
    // the previous seed.
    def self."srand"(_recv, arg?) {
        let (mt, seed) = seed_from(arg)?;
        let mut guard = DEFAULT.lock();
        let previous = guard.as_ref().map(|r| r.seed.clone()).unwrap_or(RubyValue::Int(0));
        *guard = Some(Arc::new(RandomObj { state: Mutex::new(mt), seed, frozen: AtomicBool::new(false) }));
        Ok(previous)
    }

    def "rand"(recv, arg?) {
        rand_with(&as_random(recv).state, arg)
    }
    def "bytes"(recv, arg) {
        let n = &bytes_count(arg)?;
        if *n < 0 {
            return Err(arg_error!("negative string size (or size too big)"));
        }
        Ok(random_bytes(&as_random(recv).state, *n as usize))
    }
    def "seed"(recv) {
        Ok(as_random(recv).seed.clone())
    }
    // `Random#==`: two generators are equal when their seed AND current stream
    // position match (so two fresh `Random.new(1)` are equal, but diverge once
    // either draws) -- CRuby compares state, not object identity.
    def "=="(recv, other) {
        let RubyValue::Object(o) = other else {
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
        // Ruby compares two Randoms by their seed and their POSITION in the
        // stream, which the next word stands for -- and a peek costs nothing
        // here, since both generators are already reproducible from the seed.
        // Sequential (the tuple form would hold both guards to statement
        // end -- an opposite-order deadlock under parallel threads).
        let mine = me.state.lock().clone().next_u32();
        let theirs = other.state.lock().clone().next_u32();
        Ok(RubyValue::Bool(me.seed.rb_eq(&other.seed) && mine == theirs))
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

#[cfg(test)]
mod tests {
    // These exercise the PURE PRNG logic (seeding, ranges, reproducibility) --
    // no method dispatch -- so they need no installed registry. The ArgumentError
    // paths route through `raise_error`, which panics registry-less, so the
    // panic message IS the assertion (the codebase's convention for those).
    use super::*;

    /// One word from a generator, for the tests that compare two streams.
    fn draw(state: &Mutex<crate::mt::Mt>) -> u32 {
        state.lock().next_u32()
    }

    fn r#gen(seed: i64) -> Arc<RandomObj> {
        let RubyValue::Object(o) = new_random(Some(&RubyValue::Int(seed))).unwrap() else {
            panic!("Random.new gave a non-object")
        };
        downcast_robj::<RandomObj>(&o).unwrap()
    }

    #[test]
    fn same_seed_yields_the_same_sequence() {
        let a = r#gen(5);
        let b = r#gen(5);
        for _ in 0..16 {
            assert_eq!(draw(&a.state), draw(&b.state));
        }
    }

    #[test]
    fn different_seeds_diverge_immediately() {
        assert_ne!(draw(&r#gen(5).state), draw(&r#gen(6).state));
    }

    #[test]
    fn float_seed_truncates_to_an_integer_and_is_reproducible() {
        // 1e300 == 1e300 -> same generator; 1e300 != 2e300 -> distinct.
        let (mut w1, s1) = seed_from(Some(&RubyValue::Float(1e300))).unwrap();
        let (mut w1b, _) = seed_from(Some(&RubyValue::Float(1e300))).unwrap();
        let (mut w2, _) = seed_from(Some(&RubyValue::Float(2e300))).unwrap();
        assert_eq!(w1.next_u32(), w1b.next_u32());
        assert_ne!(w1.next_u32(), w2.next_u32());
        assert!(
            matches!(s1, RubyValue::BigInt(_)),
            "seed reported as the truncated integer"
        );
        // 3.9 truncates to 3.
        let (_, small) = seed_from(Some(&RubyValue::Float(3.9))).unwrap();
        assert!(matches!(small, RubyValue::Int(3)));
    }

    #[test]
    fn rand_no_argument_is_a_unit_float() {
        let r = r#gen(1);
        for _ in 0..100 {
            let RubyValue::Float(f) = rand_with(&r.state, None).unwrap() else {
                panic!()
            };
            assert!((0.0..1.0).contains(&f));
        }
    }

    #[test]
    fn rand_integer_bound_stays_in_range_and_returns_an_integer() {
        let r = r#gen(1);
        for _ in 0..200 {
            let RubyValue::Int(n) = rand_with(&r.state, Some(&RubyValue::Int(10))).unwrap() else {
                panic!("integer bound must give an Integer")
            };
            assert!((0..10).contains(&n));
        }
    }

    #[test]
    fn rand_float_bound_returns_a_float_in_range() {
        let r = r#gen(1);
        for _ in 0..100 {
            let RubyValue::Float(f) = rand_with(&r.state, Some(&RubyValue::Float(20.43))).unwrap()
            else {
                panic!("float bound must give a Float")
            };
            assert!((0.0..20.43).contains(&f));
        }
    }

    #[test]
    fn rand_integer_range_bound_is_inclusive_and_typed() {
        let r = r#gen(3);
        let range = RubyValue::Range(
            Some(Box::new(RubyValue::Int(5))),
            Some(Box::new(RubyValue::Int(9))),
            false,
        );
        for _ in 0..200 {
            let RubyValue::Int(n) = rand_with(&r.state, Some(&range)).unwrap() else {
                panic!()
            };
            assert!((5..=9).contains(&n));
        }
    }

    #[test]
    #[should_panic(expected = "ArgumentError")]
    fn rand_zero_bound_raises_argument_error() {
        let _ = rand_with(&r#gen(1).state, Some(&RubyValue::Int(0)));
    }

    #[test]
    #[should_panic(expected = "ArgumentError")]
    fn rand_negative_integer_bound_raises_argument_error() {
        let _ = rand_with(&r#gen(1).state, Some(&RubyValue::Int(-12)));
    }

    #[test]
    #[should_panic(expected = "ArgumentError")]
    fn rand_negative_float_bound_raises_argument_error() {
        let _ = rand_with(&r#gen(1).state, Some(&RubyValue::Float(-1.5)));
    }

    #[test]
    fn seed_reports_the_integer_seed() {
        assert!(matches!(r#gen(42).seed, RubyValue::Int(42)));
    }

    #[test]
    fn dup_copies_the_generator_state() {
        let r = r#gen(7);
        let dup = downcast_robj::<RandomObj>(&r.dup_object(false)).unwrap();
        // A copy starts from the same word, so the first draw matches; the two
        // then advance independently (separate Mutexes).
        assert_eq!(draw(&r.state), draw(&dup.state));
    }

    #[test]
    fn bytes_returns_the_requested_length() {
        let RubyValue::Str(s) = random_bytes(&r#gen(1).state, 7) else {
            panic!()
        };
        assert_eq!(s.lock().bytesize(), 7);
    }
}

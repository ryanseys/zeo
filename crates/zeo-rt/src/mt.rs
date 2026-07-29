//! MT19937 -- the generator Ruby's `Random` is, plus the two draw primitives
//! it is read through. Reproducing all three exactly is what makes a seeded
//! sequence agree with ruby's, which is the only thing a seed is for: a test
//! fixture, a shuffled deck replayed from a log, a property-test seed pasted
//! from a failure report.
//!
//! The generator is the reference MT19937 (Matsumoto & Nishimura, 1998); the
//! two draws are ruby's own `limited_rand` (masked rejection, NOT a modulo --
//! see [`Mt::limited`]) and `genrand_real` (53 bits from two words).

use num_bigint::{BigInt, Sign};

const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908_b0df;
const UPPER_MASK: u32 = 0x8000_0000;
const LOWER_MASK: u32 = 0x7fff_ffff;

pub struct Mt {
    state: [u32; N],
    /// How many words of `state` are still untempered; `0` forces a refill.
    left: usize,
}

impl Clone for Mt {
    fn clone(&self) -> Self {
        Mt {
            state: self.state,
            left: self.left,
        }
    }
}

impl Mt {
    /// Ruby's seeding for a seed that fits in one 32-bit word.
    pub fn from_u32(seed: u32) -> Mt {
        let mut state = [0u32; N];
        state[0] = seed;
        for i in 1..N {
            let prev = state[i - 1];
            state[i] = 1_812_433_253u32
                .wrapping_mul(prev ^ (prev >> 30))
                .wrapping_add(i as u32);
        }
        Mt { state, left: 0 }
    }

    /// Ruby's seeding for a wider seed: the reference `init_by_array` over the
    /// seed's 32-bit words, least significant first.
    pub fn from_words(key: &[u32]) -> Mt {
        let mut mt = Mt::from_u32(19_650_218);
        let (mut i, mut j) = (1usize, 0usize);
        for _ in 0..N.max(key.len()) {
            let prev = mt.state[i - 1];
            mt.state[i] = (mt.state[i] ^ (prev ^ (prev >> 30)).wrapping_mul(1_664_525))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                mt.state[0] = mt.state[N - 1];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
        }
        for _ in 0..N - 1 {
            let prev = mt.state[i - 1];
            mt.state[i] = (mt.state[i] ^ (prev ^ (prev >> 30)).wrapping_mul(1_566_083_941))
                .wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                mt.state[0] = mt.state[N - 1];
                i = 1;
            }
        }
        mt.state[0] = 0x8000_0000;
        mt.left = 0;
        mt
    }

    /// A seed of any magnitude, the way ruby packs one: a single word seeds
    /// through [`Mt::from_u32`], anything wider through [`Mt::from_words`].
    /// The seed's SIGN is dropped -- ruby seeds from the magnitude.
    pub fn from_bigint(seed: &BigInt) -> Mt {
        let magnitude = if seed.sign() == Sign::Minus {
            -seed
        } else {
            seed.clone()
        };
        let mut words: Vec<u32> = magnitude
            .to_bytes_le()
            .1
            .chunks(4)
            .map(|c| {
                let mut w = [0u8; 4];
                w[..c.len()].copy_from_slice(c);
                u32::from_le_bytes(w)
            })
            .collect();
        while words.len() > 1 && *words.last().unwrap() == 0 {
            words.pop();
        }
        if words.len() <= 1 {
            return Mt::from_u32(words.first().copied().unwrap_or(0));
        }
        // Ruby drops a most-significant word of exactly 1 here -- the
        // leading-zero guard its own bignum packing leaves behind. Keeping it
        // would seed every `2**32k + x` differently from ruby.
        if *words.last().unwrap() == 1 {
            words.pop();
        }
        Mt::from_words(&words)
    }

    fn refill(&mut self) {
        let mag01 = [0u32, MATRIX_A];
        for i in 0..N {
            let y = (self.state[i] & UPPER_MASK) | (self.state[(i + 1) % N] & LOWER_MASK);
            self.state[i] = self.state[(i + M) % N] ^ (y >> 1) ^ mag01[(y & 1) as usize];
        }
        self.left = N;
    }

    /// One tempered 32-bit word -- the reference `genrand_int32`.
    pub fn next_u32(&mut self) -> u32 {
        if self.left == 0 {
            self.refill();
        }
        let mut y = self.state[N - self.left];
        self.left -= 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    /// A uniform integer in `[0, limit]`, ruby's `limited_rand`: mask off the
    /// bits above `limit` and REDRAW when the result overshoots. Not a modulo
    /// -- a modulo consumes one word per draw and biases toward the low end,
    /// and both differences show up in the very first number of a sequence.
    pub fn limited(&mut self, limit: u64) -> u64 {
        if limit == 0 {
            return 0;
        }
        let mut mask = limit;
        for shift in [1, 2, 4, 8, 16, 32] {
            mask |= mask >> shift;
        }
        loop {
            let mut val = 0u64;
            let mut overshot = false;
            // Most significant word first, and only for a word the mask
            // actually reaches -- so a small limit draws ONE word, not two.
            for i in (0..2).rev() {
                if (mask >> (i * 32)) & 0xffff_ffff == 0 {
                    continue;
                }
                val |= u64::from(self.next_u32()) << (i * 32);
                val &= mask;
                if limit < val {
                    overshot = true;
                    break;
                }
            }
            if !overshot {
                return val;
            }
        }
    }

    /// A uniform Float in `[0, 1)` with 53 significant bits -- ruby's
    /// `genrand_real`, which is two words, high bits first.
    pub fn next_real(&mut self) -> f64 {
        let a = self.next_u32() >> 5;
        let b = self.next_u32() >> 6;
        (f64::from(a) * 67_108_864.0 + f64::from(b)) * (1.0 / 9_007_199_254_740_992.0)
    }

    /// A uniform Float in `[0, 1]` -- what an INCLUSIVE range draws through.
    /// Two whole words this time (not the truncated pair [`Mt::next_real`]
    /// uses), scaled by `2^53 + 1` so that 1.0 is reachable.
    pub fn next_real_inclusive(&mut self) -> f64 {
        let a = u128::from(self.next_u32());
        let b = u128::from(self.next_u32());
        let r = (((a << 32) | b) * ((1u128 << 53) | 1)) >> 64;
        (r as f64) * (1.0 / 9_007_199_254_740_992.0)
    }

    /// A uniform `BigInt` in `[0, n)` for a positive `n` -- [`Mt::limited`]'s
    /// rejection scheme widened over words. Ruby fills the result MOST
    /// significant word first, masking that word down to the bound's own
    /// width and redrawing the whole number when it overshoots; every word
    /// below the first that comes in strictly under the bound is then free to
    /// take all 32 bits.
    pub fn bigint_below(&mut self, n: &BigInt) -> BigInt {
        let limit = n - 1u32;
        let mut words: Vec<u32> = limit
            .to_bytes_le()
            .1
            .chunks(4)
            .map(|c| {
                let mut w = [0u8; 4];
                w[..c.len()].copy_from_slice(c);
                u32::from_le_bytes(w)
            })
            .collect();
        if words.is_empty() {
            words.push(0);
        }
        'retry: loop {
            let mut out = vec![0u32; words.len()];
            // `at_boundary` means every word so far has equalled the bound's,
            // so this word is still constrained by it.
            let mut at_boundary = true;
            for i in (0..words.len()).rev() {
                let lim = words[i];
                let rnd = if at_boundary {
                    let mut mask = lim;
                    for shift in [1, 2, 4, 8, 16] {
                        mask |= mask >> shift;
                    }
                    if mask == 0 {
                        0
                    } else {
                        let r = self.next_u32() & mask;
                        if lim < r {
                            continue 'retry;
                        }
                        if r < lim {
                            at_boundary = false;
                        }
                        r
                    }
                } else {
                    self.next_u32()
                };
                out[i] = rnd;
            }
            let bytes: Vec<u8> = out.iter().flat_map(|w| w.to_le_bytes()).collect();
            return BigInt::from_bytes_le(Sign::Plus, &bytes);
        }
    }

    /// `n` random bytes, four per word, least significant first.
    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(n + 4);
        while out.len() < n {
            out.extend_from_slice(&self.next_u32().to_le_bytes());
        }
        out.truncate(n);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference MT19937's own published output for `init_genrand(5489)`,
    /// its documented default seed.
    #[test]
    fn matches_the_reference_stream() {
        let mut mt = Mt::from_u32(5489);
        assert_eq!(mt.next_u32(), 3_499_211_612);
        assert_eq!(mt.next_u32(), 581_869_302);
        assert_eq!(mt.next_u32(), 3_890_346_734);
    }

    /// `Random.new(42)` in ruby answers 51, then 92, for `rand(100)`.
    #[test]
    fn reproduces_rubys_seeded_sequence() {
        let mut mt = Mt::from_u32(42);
        assert_eq!(mt.limited(99), 51);
        assert_eq!(mt.limited(99), 92);
    }
}

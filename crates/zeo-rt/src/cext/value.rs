//! `VALUE`: MRI's encoding, bit for bit.
//!
//! An extension is free to write `INT2FIX(3)` by hand, to test `RB_NIL_P`
//! with a comparison against `0x04`, and to assume a heap object is a
//! non-zero 8-aligned pointer. Thousands do. So zeo does not invent an
//! encoding -- it adopts MRI's, and `ruby/internal/special_consts.h` stays
//! unpatched.
//!
//! The constants here are the ones that header spells under `USE_FLONUM`,
//! which is what `SIZEOF_VALUE >= SIZEOF_DOUBLE` selects on every target zeo
//! builds extensions for. `the_constants_match_the_vendored_header` reads
//! them back out of the header rather than trusting this comment.
//!
//! What is NOT here is the heap half. A heap `VALUE` is a `*const Handle`
//! ([`super::handles`]); this module only decides which values never need
//! one.

use crate::RubyValue;

/// MRI's `VALUE`: pointer-width, and a pointer when it is not an immediate.
pub type Value = usize;

pub const Q_FALSE: Value = 0x00;
pub const Q_NIL: Value = 0x04;
pub const Q_TRUE: Value = 0x14;
pub const Q_UNDEF: Value = 0x24;

const IMMEDIATE_MASK: Value = 0x07;
const FIXNUM_FLAG: Value = 0x01;
const FLONUM_MASK: Value = 0x03;
const FLONUM_FLAG: Value = 0x02;
const SYMBOL_FLAG: Value = 0x0c;

/// MRI's Fixnum is 63-bit: one bit pays for the tag. zeo's `Int` is a full
/// `i64`, so the range outside this becomes a heap `T_BIGNUM` handle -- which
/// is what CRuby does with the same integers.
pub const FIXNUM_MAX: i64 = i64::MAX >> 1;
pub const FIXNUM_MIN: i64 = i64::MIN >> 1;

/// `+0.0` has no rotated form, so MRI spells it as this one constant.
const FLONUM_ZERO: Value = 0x8000_0000_0000_0002;

pub const fn is_immediate(v: Value) -> bool {
    v & IMMEDIATE_MASK != 0
}

/// `RB_SPECIAL_CONST_P`: an immediate, or one of the three non-pointer
/// singletons. Everything else is a handle.
pub const fn is_special_const(v: Value) -> bool {
    is_immediate(v) || v == Q_FALSE || v == Q_NIL || v == Q_TRUE || v == Q_UNDEF
}

pub const fn is_fixnum(v: Value) -> bool {
    v & FIXNUM_FLAG != 0
}

pub const fn is_flonum(v: Value) -> bool {
    v & FLONUM_MASK == FLONUM_FLAG
}

pub const fn is_static_symbol(v: Value) -> bool {
    v & 0xff == SYMBOL_FLAG
}

pub const fn fits_fixnum(n: i64) -> bool {
    n >= FIXNUM_MIN && n <= FIXNUM_MAX
}

pub const fn fixnum(n: i64) -> Value {
    ((n as Value) << 1) | FIXNUM_FLAG
}

/// An arithmetic shift, so a negative Fixnum decodes to a negative `i64`.
pub const fn fixnum_value(v: Value) -> i64 {
    (v as i64) >> 1
}

pub const fn static_symbol(id: u32) -> Value {
    ((id as Value) << 8) | SYMBOL_FLAG
}

pub const fn static_symbol_id(v: Value) -> u32 {
    (v >> 8) as u32
}

/// MRI's `rb_flonum_new`, transcribed. Three of the eight exponent-high
/// nibbles round-trip through a 3-bit left rotation with the low bit spent on
/// the tag; a double outside them needs a heap object.
///
/// The `0x3000000000000000` exclusion is upstream's: that bit pattern would
/// rotate onto `+0.0`'s reserved spelling.
pub fn flonum(d: f64) -> Option<Value> {
    let t = d.to_bits() as Value;
    let bits = ((t >> 60) & 0x7) as i32;
    if t != 0x3000_0000_0000_0000 && (bits - 3) & !0x01 == 0 {
        return Some((t.rotate_left(3) & !0x01) | FLONUM_FLAG);
    }
    if t == 0 {
        return Some(FLONUM_ZERO);
    }
    None
}

/// MRI's `rb_float_value_inline`, transcribed.
pub fn flonum_value(v: Value) -> f64 {
    if v == FLONUM_ZERO {
        return 0.0;
    }
    let b63 = v >> 63;
    f64::from_bits(((2 - b63) | (v & !0x03)).rotate_right(3) as u64)
}

/// The `VALUE` for a Ruby value that needs no heap handle.
///
/// `None` means "this one is a handle", and is the only answer for anything
/// with identity. Note what is deliberately absent: `Int` outside the Fixnum
/// range, and a `Float` the flonum encoding cannot hold.
pub fn immediate_of(v: &RubyValue) -> Option<Value> {
    Some(match v {
        RubyValue::Nil => Q_NIL,
        RubyValue::Bool(true) => Q_TRUE,
        RubyValue::Bool(false) => Q_FALSE,
        RubyValue::Int(n) if fits_fixnum(*n) => fixnum(*n),
        RubyValue::Float(f) => flonum(*f)?,
        RubyValue::Symbol(s) => static_symbol(s.to_u32()),
        _ => return None,
    })
}

/// The Ruby value a `VALUE` spells, when it spells one on its own.
///
/// `None` means the `VALUE` is a handle and [`super::handles`] owns the
/// answer. `Q_UNDEF` also answers `None`: it is not a Ruby value, and a
/// caller that can see one has to say what it means itself.
pub fn from_immediate(v: Value) -> Option<RubyValue> {
    Some(match v {
        Q_NIL => RubyValue::Nil,
        Q_TRUE => RubyValue::Bool(true),
        Q_FALSE => RubyValue::Bool(false),
        Q_UNDEF => return None,
        _ if is_fixnum(v) => RubyValue::Int(fixnum_value(v)),
        _ if is_flonum(v) => RubyValue::Float(flonum_value(v)),
        _ if is_static_symbol(v) => RubyValue::Symbol(crate::Symbol::from_u32(static_symbol_id(v))),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constants are MRI's or they are nothing. Read them back out of the
    /// vendored header rather than trusting the transcription above -- a
    /// re-vendor at a later Ruby is exactly when one of them could move.
    #[test]
    fn the_constants_match_the_vendored_header() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/cext/include/ruby/internal/special_consts.h"
        );
        let text = std::fs::read_to_string(path).expect("the vendored header is present");
        // The USE_FLONUM arm, which is the one every zeo target selects.
        let arm = text
            .split("#elif USE_FLONUM")
            .nth(1)
            .and_then(|s| s.split("#else").next())
            .expect("special_consts.h still has a USE_FLONUM arm");
        for (name, want) in [
            ("RUBY_Qfalse", Q_FALSE),
            ("RUBY_Qnil", Q_NIL),
            ("RUBY_Qtrue", Q_TRUE),
            ("RUBY_Qundef", Q_UNDEF),
            ("RUBY_IMMEDIATE_MASK", IMMEDIATE_MASK),
            ("RUBY_FIXNUM_FLAG", FIXNUM_FLAG),
            ("RUBY_FLONUM_MASK", FLONUM_MASK),
            ("RUBY_FLONUM_FLAG", FLONUM_FLAG),
            ("RUBY_SYMBOL_FLAG", SYMBOL_FLAG),
        ] {
            let line = arm
                .lines()
                .find(|l| l.trim_start().starts_with(&format!("{name} ")))
                .unwrap_or_else(|| panic!("{name} is gone from special_consts.h"));
            // `RUBY_Qnil = 0x04, /* ...0000 0100 */`
            let hex = line
                .split('=')
                .nth(1)
                .and_then(|s| s.split("/*").next())
                .map(|s| s.trim().trim_end_matches(','))
                .and_then(|s| s.strip_prefix("0x"))
                .unwrap_or_else(|| panic!("{name} is no longer a hex literal: {line}"));
            let got = Value::from_str_radix(hex, 16).expect("a hex literal");
            assert_eq!(got, want, "{name}");
        }
    }

    #[test]
    fn a_fixnum_round_trips_across_its_whole_range() {
        for n in [0, 1, -1, 42, -42, FIXNUM_MAX, FIXNUM_MIN] {
            assert!(fits_fixnum(n), "{n}");
            let v = fixnum(n);
            assert!(
                is_fixnum(v) && is_immediate(v) && is_special_const(v),
                "{n}"
            );
            assert_eq!(fixnum_value(v), n);
        }
        assert!(!fits_fixnum(FIXNUM_MAX + 1));
        assert!(!fits_fixnum(FIXNUM_MIN - 1));
        assert!(!fits_fixnum(i64::MAX));
        assert!(!fits_fixnum(i64::MIN));
    }

    #[test]
    fn a_flonum_round_trips_and_the_rest_ask_for_a_handle() {
        for d in [0.0, 1.0, -1.0, 0.5, -0.5, 3.14, 1e10, -1e-10, 1.5e-100] {
            match flonum(d) {
                Some(v) => {
                    assert!(is_flonum(v) && is_special_const(v), "{d}");
                    assert_eq!(flonum_value(v), d, "{d}");
                }
                // Out of the encodable range is a fine answer; a WRONG value
                // is not, and that is what the assert above rules out.
                None => {}
            }
        }
        // The extremes of the exponent are outside the three nibbles.
        assert_eq!(flonum(f64::MAX), None);
        assert_eq!(flonum(f64::INFINITY), None);
        assert_eq!(flonum(f64::NAN), None);
        // `-0.0` is not `+0.0`: it must not collapse onto the reserved
        // spelling, because `1/-0.0` and `1/0.0` differ in Ruby.
        if let Some(v) = flonum(-0.0) {
            assert!(flonum_value(v).is_sign_negative());
        }
    }

    #[test]
    fn the_singletons_are_not_pointers_and_are_not_each_other() {
        let all = [Q_FALSE, Q_NIL, Q_TRUE, Q_UNDEF];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
            assert!(is_special_const(*a));
            assert!(!is_fixnum(*a) && !is_static_symbol(*a));
        }
        // Qfalse is 0, so it is the one special const that is not immediate.
        assert!(!is_immediate(Q_FALSE));
    }

    #[test]
    fn an_identity_value_asks_for_a_handle() {
        assert!(immediate_of(&RubyValue::Int(i64::MAX)).is_none());
        assert!(immediate_of(&RubyValue::Float(f64::INFINITY)).is_none());
        let s = crate::builtins::string::str_value_in_enc(crate::encoding::UTF_8, "x");
        assert!(immediate_of(&s).is_none());
        assert!(from_immediate(Q_UNDEF).is_none());
        // A handle is 8-aligned, so no pointer can be mistaken for one of
        // these -- that is the whole reason the mask is 0x07.
        for v in [Q_NIL, Q_TRUE, Q_UNDEF, fixnum(7), static_symbol(3)] {
            assert!(v % 8 != 0 || v == Q_FALSE, "{v:#x} collides with a pointer");
        }
    }
}

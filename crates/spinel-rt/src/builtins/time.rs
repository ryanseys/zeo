//! `Time` (CRuby time.c) -- an instant, as seconds+nanoseconds since the
//! epoch plus the offset it renders in.
//!
//! Fully safe Rust, no `unsafe`/libc: the civil<->epoch conversion is done
//! in-tree with Howard Hinnant's integer days<->civil algorithm (proleptic
//! Gregorian, no leap seconds -- exactly what `timegm`/`gmtime` compute), and
//! the OS-dependent LOCAL zone (offset/DST/abbreviation at an instant) is read
//! via `jiff` over the system zoneinfo. Ruby's `strftime` directive set and
//! zone semantics are specific enough that the directive table below is
//! hand-ported, because that part IS Ruby-specific.
//!
//! `Time` includes `Comparable` (see the ABI table), so `<`/`between?`/
//! `clamp` all fall out of the existing `comparable_send` driver once `<=>`
//! exists -- only `<=>` is defined here.

use std::sync::Arc;

use crate::builtins::{arity, builtin_methods};
use crate::dispatch::{raise_error, RObj, RubyObject};
use crate::{RubyValue, Signal};
use spinel_abi::{ClassId, TIME_CLASS};

/// One instant. `sec`/`nsec` are epoch-based and zone-independent; `offset`
/// only affects RENDERING (and the civil-field readers), which is why
/// `getutc` can answer a new value sharing the same `sec`.
pub struct RTime {
    /// Seconds since the Unix epoch, EXACTLY: `num / den` (`den` > 0; may be
    /// negative for pre-1970).
    ///
    /// A rational, not `(sec, nsec)`, because that is what CRuby stores and
    /// the difference is observable: `Time.at(10.8).subsec` is
    /// `(225179981368525/281474976710656)` -- the double's true value, whose
    /// denominator is a power of two -- and `nsec` is only a TRUNCATED VIEW
    /// of it. Rounding to nanoseconds at construction loses the tail, and
    /// the loss then survives arithmetic: `Time.at(10.8) - 0.9` is nsec
    /// 900000000 in Ruby but 899999999 if the 0.71ns tail was dropped first.
    /// Oracle-verified both ways.
    num: num_bigint::BigInt,
    den: num_bigint::BigInt,
    /// Seconds east of UTC. `None` means "this Time is in LOCAL time" --
    /// distinct from a fixed offset, because a local Time's offset depends on
    /// its own instant (DST), so it can't be baked in at construction. The
    /// sentinel `Some(RTime::UTC)` marks an explicitly-UTC Time (renders
    /// `UTC`, `utc?` true), as opposed to a numeric `Some(0)` = `+00:00`.
    ///
    /// Interior-mutable because CRuby's `Time#utc`/`#gmtime`/`#localtime`
    /// convert the receiver IN PLACE and answer self (as opposed to the
    /// `get*` copies) -- which callers observe: `t.utc; t.to_s` renders UTC.
    /// Only the RENDERING zone is mutable; the instant never changes, which
    /// is why `==`/`hash`/`<=>` are unaffected by it.
    offset: parking_lot::Mutex<Option<i32>>,
}

impl RTime {
    /// The sentinel `offset` value marking a UTC-flagged Time, distinct from a
    /// numeric `+00:00` fixed offset (`Some(0)`): CRuby renders the former as
    /// `UTC` and the latter as `+0000`, and only the former is `utc?`. It is
    /// far outside the valid ±86399 offset range, so it can never collide with
    /// a real offset (and never reaches `offset_str`, guarded by `is_utc`).
    const UTC: i32 = i32::MAX;

    fn offset(&self) -> Option<i32> {
        *self.offset.lock()
    }

    /// Whether this Time renders in UTC (`Time.utc`, `#utc`, or a `"UTC"`/`"Z"`
    /// string), as opposed to a fixed numeric offset or system-local.
    fn is_utc(&self) -> bool {
        self.offset() == Some(Self::UTC)
    }

    /// Whole seconds since the epoch, FLOORED -- so a pre-1970 instant with a
    /// fraction still leaves a positive sub-second remainder (`Time.at(-0.5)`
    /// is second -1 plus 500000000ns, Ruby's own normalization).
    fn sec(&self) -> i64 {
        use num_integer::Integer;
        i64::try_from(self.num.div_floor(&self.den)).unwrap_or(i64::MAX)
    }

    /// The sub-second part as an exact `(num, den)` fraction of a second.
    fn frac(&self) -> (num_bigint::BigInt, num_bigint::BigInt) {
        use num_integer::Integer;
        (self.num.mod_floor(&self.den), self.den.clone())
    }

    /// The sub-second part in whole nanoseconds, TRUNCATED -- `#nsec`'s view
    /// of `frac`.
    fn nsec(&self) -> u32 {
        let (n, d) = self.frac();
        u32::try_from((n * num_bigint::BigInt::from(1_000_000_000u32)) / d).unwrap_or(0)
    }
}

impl RubyObject for RTime {
    fn class_id(&self) -> ClassId {
        TIME_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    // The only mutable state is the rendering zone (`Time#utc` and friends);
    // CRuby's `freeze` does guard those, but nothing here consults it yet.
    fn is_frozen(&self) -> bool {
        false
    }
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(RTime {
            num: self.num.clone(),
            den: self.den.clone(),
            offset: parking_lot::Mutex::new(self.offset()),
        })
    }
}

/// A Time from an EXACT rational count of seconds.
///
/// REDUCES to lowest terms (and forces `den > 0`), which makes the stored
/// pair canonical -- so `==` can compare the fields directly and `#hash` can
/// derive from them and still agree with it. Without reducing, `2/2` and
/// `1/1` would be the same instant with different fields, and equal Times
/// would hash differently.
fn time_exact(
    num: num_bigint::BigInt,
    den: num_bigint::BigInt,
    offset: Option<i32>,
) -> RubyValue {
    use num_bigint::BigInt;
    use num_integer::Integer;
    let (num, den) = if den < BigInt::from(0) { (-num, -den) } else { (num, den) };
    let g = num.gcd(&den);
    let (num, den) = if g > BigInt::from(1) { (num / &g, den / &g) } else { (num, den) };
    RubyValue::Object(Arc::new(RTime {
        num,
        den,
        offset: parking_lot::Mutex::new(offset),
    }))
}

/// A Time from whole seconds + nanoseconds -- the exact-rational constructor
/// with a denominator of 1e9. Every caller whose input IS integral
/// nanoseconds (`Time.now`, the civil constructors, `File.mtime`) uses this;
/// a Float epoch must go through `float_exact_parts` instead, or its
/// sub-nanosecond tail is lost (see `RTime::num`).
fn time_value(sec: i64, nsec: u32, offset: Option<i32>) -> RubyValue {
    use num_bigint::BigInt;
    let billion = BigInt::from(1_000_000_000u32);
    time_exact(BigInt::from(sec) * &billion + BigInt::from(nsec), billion, offset)
}

/// Build a LOCAL Time from raw epoch parts -- for the other builtins that
/// answer a Time (`File.mtime`), so they don't need `RTime`'s internals.
pub(crate) fn time_from_parts(sec: i64, nsec: u32) -> RubyValue {
    time_value(sec, nsec, None)
}

fn recv_time(recv: &RubyValue) -> &RTime {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RTime>()
            .expect("Time table row dispatched on a non-Time receiver"),
        _ => panic!("Time table row dispatched on a non-Object receiver"),
    }
}

/// A broken-down civil time, mirroring the `libc::tm` fields the renderers and
/// accessors read (so every consumer of `Civil` is unchanged): `tm_year` is
/// years since 1900, `tm_mon` is 0-based, `tm_wday` is 0=Sunday, `tm_yday` is
/// 0-based, `tm_isdst` is > 0 when DST is in effect.
#[derive(Default)]
struct Tm {
    tm_year: i32,
    tm_mon: i32,
    tm_mday: i32,
    tm_hour: i32,
    tm_min: i32,
    tm_sec: i32,
    tm_wday: i32,
    tm_yday: i32,
    tm_isdst: i32,
}

/// The broken-down civil fields of an instant, in the Time's own zone.
struct Civil {
    tm: Tm,
    /// The zone offset actually in effect at this instant (DST-resolved for
    /// a local Time).
    offset: i32,
    /// The zone abbreviation (`"EST"`), empty for a fixed-offset Time.
    zone: String,
}

/// Break `t` into civil fields in the Time's own zone. A UTC/fixed-offset Time
/// shifts the epoch by its baked-in offset; a local one asks jiff for the zone
/// offset/DST/abbreviation in effect at this instant (the only OS-dependent
/// step). Either way the shifted instant is broken down with pure integer date
/// math, so the two paths agree field-for-field.
fn civil(t: &RTime) -> Civil {
    let secs = t.sec();
    let (offset, isdst, zone) = match t.offset() {
        Some(o) if o == RTime::UTC => (0, 0, "UTC".to_string()),
        // A fixed numeric offset (including `+00:00`) has no zone NAME.
        Some(off) => (off, 0, String::new()),
        None => local_zone(secs),
    };
    let mut tm = broken_down(secs + offset as i64);
    tm.tm_isdst = isdst;
    Civil { tm, offset, zone }
}

/// The system local zone's `(offset east of UTC, isdst flag, abbreviation)` at
/// a UTC instant. Falls back to UTC for instants outside jiff's representable
/// range (years beyond ±9999), where a local zone is undefined anyway.
fn local_zone(secs: i64) -> (i32, i32, String) {
    let Ok(ts) = jiff::Timestamp::from_second(secs) else {
        return (0, 0, "UTC".to_string());
    };
    let tz = jiff::tz::TimeZone::system();
    let info = tz.to_offset_info(ts);
    let isdst = i32::from(info.dst().is_dst());
    (info.offset().seconds(), isdst, info.abbreviation().to_string())
}

/// Break epoch seconds into UTC civil fields via Howard Hinnant's days<->civil
/// algorithm (proleptic Gregorian, no leap seconds -- exactly what `gmtime`
/// computes). Pure integer arithmetic.
fn broken_down(secs: i64) -> Tm {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    // 1970-01-01 (day 0) was a Thursday, which is 4 in the 0=Sunday numbering.
    let wday = (days + 4).rem_euclid(7);
    let yday = days - days_from_civil(year, 1, 1);
    Tm {
        tm_year: (year - 1900) as i32,
        tm_mon: (month - 1) as i32,
        tm_mday: day as i32,
        tm_hour: (rem / 3600) as i32,
        tm_min: (rem % 3600 / 60) as i32,
        tm_sec: (rem % 60) as i32,
        tm_wday: wday as i32,
        tm_yday: yday as i32,
        tm_isdst: 0,
    }
}

/// Days since 1970-01-01 for a proleptic-Gregorian `y/m/d` (`m` in 1..=12);
/// Hinnant's `days_from_civil`. Linear in `d`, so an out-of-range day still
/// yields the correct running day count (the caller's normalization rule).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

/// Inverse of [`days_from_civil`]: the `(year, month, day)` for a
/// days-since-epoch count.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `+hhmm`/`-hhmm` -- strftime's `%z` and `to_s`'s trailing offset.
///
/// `with_seconds` adds a third `ss` field, but ONLY when the offset isn't a
/// whole number of minutes. That is `inspect`'s rendering, not `to_s`'s:
/// oracle-verified that `Time.new(2000,1,1,0,0,0,123)` (a 123-second offset)
/// answers `+0002` from `to_s` but `+000203` from `inspect`, while a
/// whole-minute offset renders `+0002` from both.
fn offset_str(off: i32, with_seconds: bool) -> String {
    let sign = if off < 0 { '-' } else { '+' };
    let a = off.abs();
    let (h, m, s) = (a / 3600, (a % 3600) / 60, a % 60);
    if with_seconds && s != 0 {
        format!("{sign}{h:02}{m:02}{s:02}")
    } else {
        format!("{sign}{h:02}{m:02}")
    }
}

/// strftime's colon-offset directives: `%:z` -> `+HH:MM`, `%::z` ->
/// `+HH:MM:SS`, `%:::z` -> the minimal colon form.
fn offset_str_colon(off: i32, colons: usize) -> String {
    let sign = if off < 0 { '-' } else { '+' };
    let a = off.abs();
    let (h, m, s) = (a / 3600, (a % 3600) / 60, a % 60);
    match colons {
        1 => format!("{sign}{h:02}:{m:02}"),
        2 => format!("{sign}{h:02}:{m:02}:{s:02}"),
        _ if s != 0 => format!("{sign}{h:02}:{m:02}:{s:02}"),
        _ if m != 0 => format!("{sign}{h:02}:{m:02}"),
        _ => format!("{sign}{h:02}"),
    }
}

/// `Time#to_s`/`#inspect`: `2023-11-14 17:13:20 -0500`, or `... UTC` for a
/// UTC Time -- oracle-verified, including that the two agree (CRuby's
/// `inspect` adds sub-second digits only when nsec is nonzero, which this
/// does too).
fn render(t: &RTime, with_subsec: bool) -> String {
    let c = civil(t);
    let subsec = if with_subsec && t.nsec() != 0 {
        // CRuby trims trailing zeros: `.5`, not `.500000000`.
        format!(".{}", format!("{:09}", t.nsec()).trim_end_matches('0'))
    } else {
        String::new()
    };
    let tail = if t.is_utc() {
        "UTC".to_string()
    } else {
        offset_str(c.offset, with_subsec)
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}{} {}",
        c.tm.tm_year + 1900,
        c.tm.tm_mon + 1,
        c.tm.tm_mday,
        c.tm.tm_hour,
        c.tm.tm_min,
        c.tm.tm_sec,
        subsec,
        tail
    )
}

const DAY_NAMES: [&str; 7] = [
    "Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday",
];
const MONTH_NAMES: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September",
    "October", "November", "December",
];

/// Ruby's `strftime` -- hand-ported directive table (the Ruby-specific part;
/// C's own strftime lacks Ruby's flags and several directives).
///
/// Supports the `-` (no padding) and `0`/`_` (pad with zero/space) flags that
/// Ruby adds on top of C's set. An UNKNOWN directive is emitted verbatim,
/// including its `%`, which is what Ruby does rather than raising.
fn strftime(t: &RTime, fmt: &str) -> String {
    let c = civil(t);
    let tm = &c.tm;
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        // Flags, then an optional field WIDTH, then the directive. `:` flags
        // only precede `z` (`%:z` etc.); `^`/`#` upcase/swapcase the result.
        let mut pad: Option<char> = None;
        let mut colons = 0usize;
        let (mut upcase, mut swapcase) = (false, false);
        while let Some(&f) = chars.peek() {
            match f {
                '-' | '0' | '_' => pad = Some(f),
                '^' => upcase = true,
                '#' => swapcase = true,
                ':' => colons += 1,
                _ => break,
            }
            chars.next();
        }
        let mut width: Option<usize> = None;
        while let Some(d) = chars.peek().and_then(|c| c.to_digit(10)) {
            width = Some(width.unwrap_or(0) * 10 + d as usize);
            chars.next();
        }
        let Some(d) = chars.next() else {
            out.push('%');
            break;
        };
        // `num` applies the flag to a numeric directive: `-` drops padding,
        // `_` pads with spaces, otherwise zero-padded to `width` (an explicit
        // `%<n>X` width overrides the directive's default).
        let num = |v: i64, default_width: usize| -> String {
            let w = width.unwrap_or(default_width);
            match pad {
                Some('-') => v.to_string(),
                Some('_') => format!("{:>w$}", v, w = w),
                _ => format!("{:0w$}", v, w = w),
            }
        };
        // `%N`/`%L`'s fractional seconds to `digits` places: the 9-digit
        // nanosecond string, truncated or right-zero-padded to width.
        let frac = |digits: usize| -> String {
            let nine = format!("{:09}", t.nsec());
            if digits <= 9 {
                nine[..digits].to_string()
            } else {
                format!("{nine}{}", "0".repeat(digits - 9))
            }
        };
        let mut piece = match d {
            'Y' => num(tm.tm_year as i64 + 1900, 1),
            'y' => num((tm.tm_year as i64 + 1900) % 100, 2),
            'C' => num((tm.tm_year as i64 + 1900) / 100, 2),
            'm' => num(tm.tm_mon as i64 + 1, 2),
            'd' => num(tm.tm_mday as i64, 2),
            'e' => format!("{:>2}", tm.tm_mday),
            'j' => num(tm.tm_yday as i64 + 1, 3),
            'H' => num(tm.tm_hour as i64, 2),
            'k' => format!("{:>2}", tm.tm_hour),
            'I' => num(if tm.tm_hour % 12 == 0 { 12 } else { (tm.tm_hour % 12) as i64 }, 2),
            'l' => format!("{:>2}", if tm.tm_hour % 12 == 0 { 12 } else { tm.tm_hour % 12 }),
            'M' => num(tm.tm_min as i64, 2),
            'S' => num(tm.tm_sec as i64, 2),
            'L' => frac(width.unwrap_or(3)),
            'N' => frac(width.unwrap_or(9)),
            'z' if colons > 0 => offset_str_colon(c.offset, colons),
            'z' => offset_str(c.offset, false),
            'Z' => c.zone.clone(),
            'a' => DAY_NAMES[tm.tm_wday as usize][..3].to_string(),
            'A' => DAY_NAMES[tm.tm_wday as usize].to_string(),
            'b' | 'h' => MONTH_NAMES[tm.tm_mon as usize][..3].to_string(),
            'B' => MONTH_NAMES[tm.tm_mon as usize].to_string(),
            'p' => (if tm.tm_hour < 12 { "AM" } else { "PM" }).to_string(),
            'P' => (if tm.tm_hour < 12 { "am" } else { "pm" }).to_string(),
            'u' => (if tm.tm_wday == 0 { 7 } else { tm.tm_wday as i64 }).to_string(),
            'w' => (tm.tm_wday as i64).to_string(),
            's' => t.sec().to_string(),
            // The compound directives, in terms of the above.
            'F' => strftime(t, "%Y-%m-%d"),
            'T' | 'X' => strftime(t, "%H:%M:%S"),
            'D' | 'x' => strftime(t, "%m/%d/%y"),
            'R' => strftime(t, "%H:%M"),
            'r' => strftime(t, "%I:%M:%S %p"),
            'c' => strftime(t, "%a %b %e %H:%M:%S %Y"),
            'n' => "\n".to_string(),
            't' => "\t".to_string(),
            '%' => "%".to_string(),
            // Unknown: emit verbatim, `%` included (Ruby's own behavior --
            // it does not raise).
            other => format!("%{other}"),
        };
        if upcase {
            piece = piece.to_uppercase();
        } else if swapcase {
            piece = piece
                .chars()
                .map(|c| if c.is_uppercase() { c.to_ascii_lowercase() } else { c.to_ascii_uppercase() })
                .collect();
        }
        out.push_str(&piece);
    }
    out
}

/// `t + delta` / `t - delta` (`sign` picks), computed EXACTLY.
///
/// Not `epoch_arg` then integer add: rounding the delta to `(sec, nsec)`
/// FIRST already truncates it, and the error survives the arithmetic --
/// `Time.at(100) - 1.3` would answer nsec 700000000 where Ruby says
/// 699999999, because 1.3's true nanoseconds are 300000000.0000000444 and
/// dropping that remainder before subtracting rounds the wrong way. So the
/// whole sum is taken over exact integers, in units of `den`-ths of a
/// nanosecond, and floored ONCE at the end.
fn shift(t: &RTime, delta: &RubyValue, sign: i64) -> Result<RubyValue, Signal> {
    use num_bigint::BigInt;
    let (num, den) = match delta {
        RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Rational(_) => {
            crate::builtins::rational::as_ratio(delta)
        }
        RubyValue::Float(f) if f.is_finite() => crate::builtins::float::float_exact_parts(*f),
        RubyValue::Float(f) => {
            return Err(raise_error(
                "FloatDomainError",
                RubyValue::Float(*f).to_display_string(),
            ))
        }
        other => {
            return Err(raise_error(
                "TypeError",
                format!(
                    "can't convert {} into an exact number",
                    crate::builtins::class_name_of(other)
                ),
            ))
        }
    };
    // Everything over the common denominator `den`, in nanoseconds.
    // Exact rational addition over a common denominator -- NO rounding at
    // any point, so the sub-nanosecond tail the receiver carries survives
    // (see `RTime::num`).
    let new_num = &t.num * &den + BigInt::from(sign) * num * &t.den;
    let new_den = &t.den * &den;
    Ok(time_exact(new_num, new_den, t.offset()))
}

/// A count of seconds as an EXACT `(num, den)` -- `Time.at`'s argument and
/// `+`/`-`'s delta agree on this reading.
fn exact_seconds(v: &RubyValue) -> Result<(num_bigint::BigInt, num_bigint::BigInt), Signal> {
    match v {
        RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Rational(_) => {
            Ok(crate::builtins::rational::as_ratio(v))
        }
        RubyValue::Float(f) if f.is_finite() => Ok(crate::builtins::float::float_exact_parts(*f)),
        RubyValue::Float(f) => Err(raise_error(
            "FloatDomainError",
            RubyValue::Float(*f).to_display_string(),
        )),
        // `Time.at(another_time)` copies its exact instant.
        RubyValue::Object(o) if o.as_any().downcast_ref::<RTime>().is_some() => {
            let t = o.as_any().downcast_ref::<RTime>().unwrap();
            Ok((t.num.clone(), t.den.clone()))
        }
        other => Err(raise_error(
            "TypeError",
            format!(
                "can't convert {} into an exact number",
                crate::builtins::class_name_of(other)
            ),
        )),
    }
}

/// A `BigInt`'s nearest f64 -- via its decimal rendering, which is exact
/// enough for `to_f` (the value is a Time's second count, far inside f64's
/// integral range) and avoids a `num-traits` conversion import.
fn bigint_to_f64(v: &num_bigint::BigInt) -> f64 {
    v.to_string().parse().unwrap_or(f64::NAN)
}

/// The leading civil-field arguments as Integers -- shared by the three
/// civil constructors, which agree on their first six arguments and differ
/// only in what a 7th means.
fn int_parts(args: &[RubyValue], take: usize) -> Result<Vec<i64>, Signal> {
    args.iter()
        .take(take)
        .map(|a| match a {
            RubyValue::Int(i) => Ok(*i),
            // A String component is parsed as a base-10 integer (`Time.utc(
            // "2020", "3")`), matching CRuby's forced-decimal reading.
            RubyValue::Str(s) => {
                let t = s.lock().to_utf8_lossy().trim().to_string();
                t.parse::<i64>().map_err(|_| {
                    raise_error("ArgumentError", format!("argument out of range: {t:?}"))
                })
            }
            other => Err(raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Integer",
                    crate::builtins::convert_name_of(other)
                ),
            )),
        })
        .collect()
}

/// When `Time.utc`/`gm`/`local`'s seconds field (index 5) is fractional
/// (a Rational or finite Float), split it into the integer civil components
/// (with the seconds floored) and the exact sub-second `(num, den)`. Returns
/// `None` for a plain integer/absent seconds field, so the caller keeps its
/// existing integer path untouched.
fn frac_seconds(
    args: &[RubyValue],
) -> Result<Option<(Vec<i64>, num_bigint::BigInt, num_bigint::BigInt)>, Signal> {
    use num_integer::Integer;
    use num_traits::ToPrimitive;
    let (num, den) = match args.get(5) {
        Some(RubyValue::Rational(_)) => crate::builtins::rational::as_ratio(&args[5]),
        Some(RubyValue::Float(f)) if f.is_finite() => crate::builtins::float::float_exact_parts(*f),
        _ => return Ok(None),
    };
    let mut parts = int_parts(&args[..5], 5)?;
    let whole = num.div_floor(&den);
    let frac_num = &num - &whole * &den; // [0, den)
    parts.push(whole.to_i64().unwrap_or(0));
    Ok(Some((parts, frac_num, den)))
}

/// The civil constructors also accept the 10-argument `Time#to_a` order
/// (sec, min, hour, mday, mon, year, wday, yday, isdst, zone). Normalize that
/// into the forward (year, mon, mday, hour, min, sec) order; every other arity
/// passes through unchanged. Returns owned values so the caller borrows a slice.
fn normalize_civil_args(args: &[RubyValue]) -> Vec<RubyValue> {
    if args.len() == 10 {
        vec![
            args[5].clone(),
            args[4].clone(),
            args[3].clone(),
            args[2].clone(),
            args[1].clone(),
            args[0].clone(),
        ]
    } else {
        args.to_vec()
    }
}

/// Build a Time from integer civil parts, an exact sub-second, and an optional
/// fixed offset (`None` = system local, like `Time.local`).
fn build_civil_time(
    parts: &[i64],
    frac_num: num_bigint::BigInt,
    frac_den: num_bigint::BigInt,
    offset: Option<i32>,
) -> RubyValue {
    let as_utc = civil_to_epoch_utc(parts);
    let (instant, store) = match offset {
        // UTC-flagged: the components already ARE UTC, so no shift.
        Some(o) if o == RTime::UTC => (as_utc, Some(RTime::UTC)),
        Some(off) => (as_utc - off as i64, Some(off)),
        None => {
            let probe = RTime {
                num: num_bigint::BigInt::from(as_utc),
                den: num_bigint::BigInt::from(1),
                offset: parking_lot::Mutex::new(None),
            };
            (as_utc - civil(&probe).offset as i64, None)
        }
    };
    time_exact(
        num_bigint::BigInt::from(instant) * &frac_den + &frac_num,
        frac_den,
        store,
    )
}

/// `Time.new`'s string form: `"YYYY-MM-DD HH:MM:SS[.frac] [offset]"`, where the
/// offset is `+HH:MM` / `-HH:MM` / `UTC` / `Z`, or absent (local time). A
/// missing time part is `no time information`; anything else unparseable is
/// `can't parse: "..."` -- both CRuby's messages.
fn parse_time_string(input: &str) -> Result<RubyValue, Signal> {
    use num_bigint::BigInt;
    let cant = || raise_error("ArgumentError", format!("can't parse: {input:?}"));
    let mut tokens = input.trim().split_whitespace();
    let mut date = tokens.next().ok_or_else(cant)?.split('-');
    let year: i64 = date.next().and_then(|x| x.parse().ok()).ok_or_else(cant)?;
    let mon: i64 = date.next().and_then(|x| x.parse().ok()).ok_or_else(cant)?;
    let day: i64 = date.next().and_then(|x| x.parse().ok()).ok_or_else(cant)?;
    if date.next().is_some() {
        return Err(cant());
    }
    let Some(time) = tokens.next() else {
        return Err(raise_error("ArgumentError", "no time information".to_string()));
    };
    let (hms, frac_str) = match time.split_once('.') {
        Some((h, f)) => (h, Some(f)),
        None => (time, None),
    };
    let mut hms = hms.split(':');
    let hour: i64 = hms.next().and_then(|x| x.parse().ok()).ok_or_else(cant)?;
    let min: i64 = hms.next().and_then(|x| x.parse().ok()).ok_or_else(cant)?;
    let sec: i64 = hms.next().and_then(|x| x.parse().ok()).ok_or_else(cant)?;
    let (frac_num, frac_den) = match frac_str {
        Some(f) if !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()) => {
            (f.parse::<BigInt>().map_err(|_| cant())?, BigInt::from(10).pow(f.len() as u32))
        }
        Some(_) => return Err(cant()),
        None => (BigInt::from(0), BigInt::from(1)),
    };
    let offset = match tokens.next() {
        None => None,
        // "UTC"/"Z" mark a UTC time (renders `UTC`); a numeric `+00:00` is a
        // fixed offset (renders `+0000`) and flows through parse_offset.
        Some("UTC") | Some("Z") => Some(RTime::UTC),
        Some(z) => Some(parse_offset(z)?),
    };
    Ok(build_civil_time(&[year, mon, day, hour, min, sec], frac_num, frac_den, offset))
}

/// A `utc_offset` in seconds, range-checked as real Ruby does: strictly
/// within a day either way (`Time.new(.., 86400)` is an ArgumentError, and
/// `86399` is fine) -- oracle-verified.
/// Resolve an optional `utc_offset` argument (to `#getlocal`/`#localtime`):
/// `None` -> system-local (`None`); an Integer -> seconds east of UTC; a String
/// -> a `"+HH:MM"`-style offset. Anything else is an ArgumentError.
fn offset_arg(v: Option<&RubyValue>) -> Result<Option<i32>, Signal> {
    match v {
        None | Some(RubyValue::Nil) => Ok(None),
        Some(RubyValue::Int(o)) => Ok(Some(check_offset(*o)?)),
        Some(RubyValue::Str(s)) => Ok(Some(parse_offset(&s.lock().to_utf8_lossy())?)),
        Some(other) => Err(raise_error(
            "ArgumentError",
            format!("\"+HH:MM\" expected for utc_offset: {}", other.to_display_string()),
        )),
    }
}

fn check_offset(off: i64) -> Result<i32, Signal> {
    if !(-86400 < off && off < 86400) {
        return Err(raise_error(
            "ArgumentError",
            "utc_offset out of range".to_string(),
        ));
    }
    Ok(off as i32)
}

/// A `"+HH:MM"` / `"-HH:MM:SS"` / `"UTC"` / `"Z"` offset String, as seconds
/// east of UTC -- `Time.new`'s 7th argument may be spelled this way.
fn parse_offset(s: &str) -> Result<i32, Signal> {
    let bad = || {
        raise_error(
            "ArgumentError",
            format!("\"+HH:MM\", \"-HH:MM\", \"UTC\" or \"A\"..\"I\",\"K\"..\"Z\" expected for utc_offset: {s}"),
        )
    };
    if s == "UTC" || s == "Z" {
        return Ok(0);
    }
    let sign = match s.as_bytes().first() {
        Some(b'+') => 1,
        Some(b'-') => -1,
        _ => return Err(bad()),
    };
    let mut parts = s[1..].split(':');
    let h: i64 = parts.next().ok_or_else(bad)?.parse().map_err(|_| bad())?;
    let m: i64 = parts.next().unwrap_or("0").parse().map_err(|_| bad())?;
    let sec: i64 = parts.next().unwrap_or("0").parse().map_err(|_| bad())?;
    check_offset(sign * (h * 3600 + m * 60 + sec))
}

/// A `Time.utc`/`Time.local` 7th argument: sub-second time in MICROSECONDS,
/// returned as whole NANOSECONDS so a fractional microsecond survives -- CRuby
/// keeps it (`Time.utc(...,500.5).nsec` is 500500, `Rational(1,2)` -> 500 nsec).
/// An Integer is range-checked to `[0, 1_000_000)`; a Float or Rational may be
/// fractional. Real Ruby range-checks (`1_000_000` us is an ArgumentError, not a
/// silent carry into the next second).
fn subsec_nsec_arg(v: Option<&RubyValue>) -> Result<u32, Signal> {
    match v {
        None => Ok(0),
        Some(RubyValue::Int(u)) if (0..1_000_000).contains(u) => Ok(*u as u32 * 1000),
        Some(RubyValue::Int(_)) => Err(raise_error(
            "ArgumentError",
            "subsecx out of range".to_string(),
        )),
        Some(v @ (RubyValue::Float(_) | RubyValue::Rational(_))) => {
            let usec = match v {
                RubyValue::Float(f) => *f,
                RubyValue::Rational(r) => crate::builtins::rational::rat_to_f64(r),
                _ => unreachable!(),
            };
            if !(0.0..1_000_000.0).contains(&usec) {
                return Err(raise_error("ArgumentError", "subsecx out of range".to_string()));
            }
            Ok((usec * 1000.0) as u32)
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

/// `Time.utc(y, mo, d, h, mi, s)` -- civil fields to epoch seconds, the UTC
/// inverse of `broken_down`. Out-of-range fields normalize (an over-large
/// month rolls into the year; day/hour/min/sec overflow just accumulate as
/// seconds), matching `timegm` -- Ruby itself raises instead, a pre-existing
/// documented divergence (the TODO on `time_utc`).
fn civil_to_epoch_utc(parts: &[i64]) -> i64 {
    let get = |i: usize, dflt: i64| parts.get(i).copied().unwrap_or(dflt);
    let (mut year, month) = (get(0, 1970), get(1, 1));
    // Normalize an out-of-range month into the year so `days_from_civil` sees
    // `m` in 1..=12; the day/time overflow needs no pre-normalization because
    // the epoch is just a running second count.
    year += (month - 1).div_euclid(12);
    let month = (month - 1).rem_euclid(12) + 1;
    let days = days_from_civil(year, month, get(2, 1));
    days * 86_400 + get(3, 0) * 3600 + get(4, 0) * 60 + get(5, 0)
}

builtin_methods! {
    pub(crate) fn lookup_class;

    "now" => fn time_now(_recv, args, _block) {
        arity!(args, 0);
        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before the Unix epoch");
        Ok(time_value(d.as_secs() as i64, d.subsec_nanos(), None))
    }
    // `Time.at(sec)` / `Time.at(sec, frac[, unit])` -- the second argument is a
    // fractional count in `unit` (default `:microsecond`; also `:millisecond`,
    // `:nanosecond`), added to the base seconds exactly.
    "at" => fn time_at(_recv, args, _block) {
        use num_bigint::BigInt;
        // A trailing `in:` keyword hash supplies the DISPLAY utc_offset (the
        // instant itself is the absolute epoch value, so no shift -- unlike
        // `Time.new`, whose components are local to that offset). Split it off
        // before the positional (seconds, subsec, unit) arguments.
        let (args, in_offset) = match args.last() {
            Some(RubyValue::Hash(h)) => {
                let off = crate::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("in")));
                (&args[..args.len() - 1], (!off.is_nil()).then_some(off))
            }
            _ => (args, None),
        };
        arity!(args, 1..=3);
        let offset = match &in_offset {
            None => None,
            Some(RubyValue::Int(off)) => Some(check_offset(*off)?),
            Some(RubyValue::Str(s)) => Some(parse_offset(&s.lock().to_utf8_lossy())?),
            Some(other) => return Err(raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Integer (utc_offset)",
                    crate::builtins::convert_name_of(other)
                ),
            )),
        };
        let (base_num, base_den) = exact_seconds(&args[0])?;
        let (num, den) = match args.get(1) {
            None => (base_num, base_den),
            Some(frac) => {
                let scale = match args.get(2) {
                    None => 1_000_000i64,
                    Some(RubyValue::Symbol(s)) => match s.name().as_str() {
                        "millisecond" => 1_000,
                        "microsecond" | "usec" => 1_000_000,
                        "nanosecond" | "nsec" => 1_000_000_000,
                        other => return Err(raise_error(
                            "ArgumentError",
                            format!("unexpected unit: {other}"),
                        )),
                    },
                    Some(other) => return Err(raise_error(
                        "ArgumentError",
                        format!("unexpected unit: {}", crate::builtins::class_name_of(other)),
                    )),
                };
                let (cnum, cden) = exact_seconds(frac)?;
                let frac_den = &cden * BigInt::from(scale);
                let total_num = &base_num * &frac_den + &cnum * &base_den;
                let total_den = &base_den * &frac_den;
                (total_num, total_den)
            }
        };
        Ok(time_exact(num, den, offset))
    }
    // `Time.utc(y, mo, d, h, mi, s)` / `Time.gm(...)`. A 7th argument is
    // MICROSECONDS (not the offset -- that is `Time.new`'s 7th; the two
    // constructors genuinely differ, oracle-verified).
    // TODO(plan P-B): out-of-range fields (`Time.utc(2023, 13, 1)`) are
    // normalized by `civil_to_epoch_utc` (-> 2024-01-01); real Ruby raises
    // ArgumentError ("mon out of range"). Needs a range check per field
    // before the call. Also unsupported: the string-month form
    // (`Time.utc(2023, "nov", 1)`).
    "utc"[0] | "gm" => fn time_utc(_recv, args, _block) {
        arity!(args, 1..=10);
        let norm = normalize_civil_args(args);
        let args = norm.as_slice();
        if let Some((parts, frac_num, frac_den)) = frac_seconds(args)? {
            let epoch = civil_to_epoch_utc(&parts);
            return Ok(time_exact(num_bigint::BigInt::from(epoch) * &frac_den + &frac_num, frac_den, Some(RTime::UTC)));
        }
        let parts = int_parts(args, 6)?;
        let nsec = subsec_nsec_arg(args.get(6))?;
        Ok(time_value(civil_to_epoch_utc(&parts), nsec, Some(RTime::UTC)))
    }
    // `Time.new(y, mo, d, h, mi, s, utc_offset)` -- the 7th argument is the
    // OFFSET, in seconds or as a `"+HH:MM"` String, unlike `Time.utc`'s
    // microseconds. With no offset given it is local time, like `Time.local`.
    // TODO(plan P-B): the `in:` keyword form isn't handled.
    "new" => fn time_new(recv, args, _block) {
        arity!(args, 0..=8);
        // `Time.new("2021-12-25 10:00:00 +09:00")` parses a time string.
        if let Some(RubyValue::Str(s)) = args.first() {
            return parse_time_string(&s.lock().to_utf8_lossy());
        }
        // A trailing `in:` keyword hash supplies the utc_offset (like the 7th
        // positional argument); split it off before reading the components.
        let (args, in_offset) = match args.last() {
            Some(RubyValue::Hash(h)) => {
                let off = crate::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("in")));
                (&args[..args.len() - 1], Some(off))
            }
            _ => (args, None),
        };
        if args.is_empty() && in_offset.is_none() {
            return time_now(recv, &[], None);
        }
        let parts = int_parts(args, 6)?;
        let as_utc = civil_to_epoch_utc(&parts);
        // An `in:` keyword offset takes the place of a 7th positional argument.
        let offset_arg = in_offset.as_ref().or_else(|| args.get(6));
        match offset_arg {
            None | Some(RubyValue::Nil) => {
                // Local: the same UTC-instant-then-shift rule `Time.local`
                // uses (see its own note on the DST-transition edge).
                let probe = RTime { num: num_bigint::BigInt::from(as_utc), den: num_bigint::BigInt::from(1), offset: parking_lot::Mutex::new(None) };
                let off = civil(&probe).offset as i64;
                Ok(time_value(as_utc - off, 0, None))
            }
            Some(RubyValue::Int(off)) => {
                let off = check_offset(*off)?;
                Ok(time_value(as_utc - off as i64, 0, Some(off)))
            }
            Some(RubyValue::Str(s)) => {
                let off = parse_offset(&s.lock().to_utf8_lossy())?;
                Ok(time_value(as_utc - off as i64, 0, Some(off)))
            }
            Some(other) => Err(raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Integer (utc_offset)",
                    crate::builtins::convert_name_of(other)
                ),
            )),
        }
    }
    // `Time.local`/`Time.mktime` -- the same civil fields read as LOCAL
    // time. `timegm` gives the UTC instant for those fields; subtracting the
    // offset in effect THERE converts it to the local reading. (Computing
    // the offset at the UTC instant rather than the local one is off by an
    // hour for civil times inside a DST transition; that edge is a
    // documented approximation, not a silent one.)
    "local" | "mktime" => fn time_local(_recv, args, _block) {
        arity!(args, 1..=10);
        let norm = normalize_civil_args(args);
        let args = norm.as_slice();
        let frac = frac_seconds(args)?;
        let parts = match &frac {
            Some((parts, ..)) => parts.clone(),
            None => int_parts(args, 6)?,
        };
        let as_utc = civil_to_epoch_utc(&parts);
        let probe = RTime { num: num_bigint::BigInt::from(as_utc), den: num_bigint::BigInt::from(1), offset: parking_lot::Mutex::new(None) };
        let off = civil(&probe).offset as i64;
        match frac {
            Some((_, frac_num, frac_den)) => Ok(time_exact(
                num_bigint::BigInt::from(as_utc - off) * &frac_den + &frac_num,
                frac_den,
                None,
            )),
            None => {
                let nsec = subsec_nsec_arg(args.get(6))?;
                Ok(time_value(as_utc - off, nsec, None))
            }
        }
    }
}

/// Rounding mode for `Time#round`/`#floor`/`#ceil`.
enum Rounding {
    Floor,
    Ceil,
    Round,
}

/// The sub-second precision argument (`ndigits`, default 0), as a non-negative
/// count of decimal places.
fn round_ndigits(args: &[RubyValue]) -> Result<u32, Signal> {
    match args.first() {
        None => Ok(0),
        Some(RubyValue::Int(n)) => Ok((*n).max(0) as u32),
        Some(other) => Err(crate::dispatch::raise_error(
            "TypeError",
            format!(
                "no implicit conversion of {} into Integer",
                crate::builtins::convert_name_of(other)
            ),
        )),
    }
}

/// A new Time with the instant reduced to `10**ndigits`-of-a-second precision.
/// `round` is half-up toward +Infinity (`Time.at(-0.5).round` is `0`, not `-1`
/// -- oracle-verified): `floor(value*scale + 1/2) / scale`.
fn time_reduce(t: &RTime, args: &[RubyValue], kind: Rounding) -> Result<RubyValue, Signal> {
    use num_integer::Integer;
    let scale = num_bigint::BigInt::from(10u32).pow(round_ndigits(args)?);
    let n = &t.num * &scale;
    let d = &t.den;
    let two = num_bigint::BigInt::from(2);
    let q = match kind {
        Rounding::Floor => n.div_floor(d),
        Rounding::Ceil => n.div_ceil(d),
        Rounding::Round => (&two * &n + d).div_floor(&(&two * d)),
    };
    Ok(time_exact(q, scale, t.offset()))
}

/// `[(key, value)]` for every field `Time#deconstruct_keys` can answer, in
/// CRuby's order. Reused by `deconstruct_keys` (whole or filtered).
fn time_field_pairs(t: &RTime) -> Vec<(&'static str, RubyValue)> {
    let c = civil(t);
    let (frac_num, frac_den) = t.frac();
    let subsec = if frac_num == num_bigint::BigInt::from(0) {
        RubyValue::Int(0)
    } else {
        crate::builtins::rational::rational_new(frac_num, frac_den).expect("nonzero denominator")
    };
    let zone = if c.zone.is_empty() {
        RubyValue::Nil
    } else {
        RubyValue::Str(crate::collections::string_new(c.zone.clone()))
    };
    vec![
        ("year", RubyValue::Int(c.tm.tm_year as i64 + 1900)),
        ("month", RubyValue::Int(c.tm.tm_mon as i64 + 1)),
        ("day", RubyValue::Int(c.tm.tm_mday as i64)),
        ("yday", RubyValue::Int(c.tm.tm_yday as i64 + 1)),
        ("wday", RubyValue::Int(c.tm.tm_wday as i64)),
        ("hour", RubyValue::Int(c.tm.tm_hour as i64)),
        ("min", RubyValue::Int(c.tm.tm_min as i64)),
        ("sec", RubyValue::Int(c.tm.tm_sec as i64)),
        ("subsec", subsec),
        ("dst", RubyValue::Bool(c.tm.tm_isdst > 0)),
        ("zone", zone),
    ]
}

builtin_methods! {
    pub(crate) fn lookup;

    "to_i"[0] | "tv_sec"[0] => fn to_i(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_time(recv).sec()))
    }
    "to_f"[0] => fn to_f(recv, args, _block) {
        arity!(args, 0);
        let t = recv_time(recv);
        // From the exact rational, not from `sec + nsec/1e9`: that rounds
        // twice and can't round-trip a Float epoch (`Time.at(1.25).to_f`).
        let (n, d) = (t.num.clone(), t.den.clone());
        Ok(RubyValue::Float(bigint_to_f64(&n) / bigint_to_f64(&d)))
    }
    "nsec"[0] | "tv_nsec"[0] => fn nsec(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_time(recv).nsec() as i64))
    }
    "usec"[0] | "tv_usec"[0] => fn usec(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int((recv_time(recv).nsec() / 1000) as i64))
    }
    // The fraction of a second, EXACTLY: a Rational (`Time.at(0.5).subsec`
    // is `(1/2)`, not 0.5), or Integer 0 for a whole second -- oracle-
    // verified. `rational_new` reduces, which is what turns 500000000/1e9
    // into 1/2.
    "subsec"[0] => fn subsec(recv, args, _block) {
        arity!(args, 0);
        // The EXACT fraction, whatever its denominator -- `Time.at(10.8).subsec`
        // is `(225179981368525/281474976710656)`, the double's true value, not
        // a nanosecond approximation of it (oracle-verified).
        let (n, d) = recv_time(recv).frac();
        if n == num_bigint::BigInt::from(0) {
            return Ok(RubyValue::Int(0));
        }
        crate::builtins::rational::rational_new(n, d)
    }
    "year"[0] => fn year(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(civil(recv_time(recv)).tm.tm_year as i64 + 1900))
    }
    "month"[0] | "mon"[0] => fn month(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(civil(recv_time(recv)).tm.tm_mon as i64 + 1))
    }
    "day"[0] | "mday"[0] => fn day(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(civil(recv_time(recv)).tm.tm_mday as i64))
    }
    "hour"[0] => fn hour(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(civil(recv_time(recv)).tm.tm_hour as i64))
    }
    "min"[0] => fn min(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(civil(recv_time(recv)).tm.tm_min as i64))
    }
    "sec"[0] => fn sec(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(civil(recv_time(recv)).tm.tm_sec as i64))
    }
    "wday"[0] => fn wday(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(civil(recv_time(recv)).tm.tm_wday as i64))
    }
    "yday"[0] => fn yday(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(civil(recv_time(recv)).tm.tm_yday as i64 + 1))
    }
    "utc_offset"[0] | "gmt_offset"[0] | "gmtoff"[0] => fn utc_offset(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(civil(recv_time(recv)).offset as i64))
    }
    "zone"[0] => fn zone(recv, args, _block) {
        arity!(args, 0);
        let z = civil(recv_time(recv)).zone;
        // A fixed-offset (non-UTC) Time has no zone NAME -- nil, not "".
        if z.is_empty() {
            return Ok(RubyValue::Nil);
        }
        Ok(RubyValue::Str(crate::collections::string_new(z)))
    }
    // `tm_isdst` is tri-state in C (>0 in effect, 0 not, <0 unknown); Ruby
    // reports a plain bool, so anything that isn't a positive answer is
    // false -- the same `> 0` test `to_a`/`strftime` already use above.
    "isdst"[0] | "dst?"[0] => fn isdst(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(civil(recv_time(recv)).tm.tm_isdst > 0))
    }
    "utc?"[0] | "gmt?"[0] => fn utc_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv_time(recv).is_utc()))
    }
    // The MUTATING converters: they change which zone the receiver RENDERS
    // in and answer self, leaving the instant alone. Callers observe the
    // mutation (`t.utc; t.to_s` renders UTC), which is why `offset` is
    // interior-mutable -- see `RTime`.
    "utc"[0] | "gmtime"[0] => fn to_utc_bang(recv, args, _block) {
        arity!(args, 0);
        *recv_time(recv).offset.lock() = Some(RTime::UTC);
        Ok(recv.clone())
    }
    "localtime" => fn to_local_bang(recv, args, _block) {
        arity!(args, 0..=1);
        // No arg -> system-local (offset None); an Integer/String arg fixes it.
        *recv_time(recv).offset.lock() = offset_arg(args.first())?;
        Ok(recv.clone())
    }
    // ...and their non-mutating counterparts, which answer a fresh Time.
    "getutc"[0] | "getgm"[0] => fn getutc(recv, args, _block) {
        arity!(args, 0);
        let t = recv_time(recv);
        Ok(time_value(t.sec(), t.nsec(), Some(RTime::UTC)))
    }
    "getlocal" => fn getlocal(recv, args, _block) {
        arity!(args, 0..=1);
        let t = recv_time(recv);
        // No arg -> system-local; an Integer/String arg fixes the utc_offset.
        Ok(time_value(t.sec(), t.nsec(), offset_arg(args.first())?))
    }
    "to_s"[0] => fn to_s(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::collections::string_new(render(recv_time(recv), false))))
    }
    "inspect"[0] => fn inspect(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::collections::string_new(render(recv_time(recv), true))))
    }
    "strftime"[1] => fn strftime_row(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Str(f) = &args[0] else {
            return Err(raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into String",
                    crate::builtins::convert_name_of(&args[0])
                ),
            ));
        };
        let fmt = f.lock().to_utf8_lossy().into_owned();
        Ok(RubyValue::Str(crate::collections::string_new(strftime(recv_time(recv), &fmt))))
    }
    // `t + n` -> a Time n seconds later; `t - other_time` -> a Float count of
    // seconds BETWEEN them, but `t - n` -> a Time. The argument's type picks.
    "+"[1] => fn plus(recv, args, _block) {
        arity!(args, 1);
        shift(recv_time(recv), &args[0], 1)
    }
    "-"[1] => fn minus(recv, args, _block) {
        arity!(args, 1);
        let t = recv_time(recv);
        if let RubyValue::Object(o) = &args[0] {
            if let Some(other) = o.as_any().downcast_ref::<RTime>() {
                let a = t.sec() as f64 + t.nsec() as f64 / 1e9;
                let b = other.sec() as f64 + other.nsec() as f64 / 1e9;
                return Ok(RubyValue::Float(a - b));
            }
        }
        shift(t, &args[0], -1)
    }
    // Drives Comparable (`<`, `between?`, `clamp`) -- see the module docs.
    "<=>"[1] => fn cmp(recv, args, _block) {
        arity!(args, 1);
        let t = recv_time(recv);
        let RubyValue::Object(o) = &args[0] else {
            return Ok(RubyValue::Nil);
        };
        let Some(other) = o.as_any().downcast_ref::<RTime>() else {
            return Ok(RubyValue::Nil);
        };
        // Exact cross-multiplication (`den` is always positive after
        // `time_exact` canonicalizes), NOT a `(sec, nsec)` compare -- that
        // would call two instants a sub-nanosecond apart equal.
        Ok(RubyValue::Int(
            match (&t.num * &other.den).cmp(&(&other.num * &t.den)) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            },
        ))
    }
    "=="[1] | "eql?"[1] => fn eq(recv, args, _block) {
        arity!(args, 1);
        let t = recv_time(recv);
        if let RubyValue::Object(o) = &args[0] {
            if let Some(other) = o.as_any().downcast_ref::<RTime>() {
                // The canonical (reduced) fields compare directly -- see
                // `time_exact`.
                return Ok(RubyValue::Bool(t.num == other.num && t.den == other.den));
            }
        }
        Ok(RubyValue::Bool(false))
    }
    "hash"[0] => fn hash(recv, args, _block) {
        arity!(args, 0);
        let t = recv_time(recv);
        // Must agree with `==` above: derived from the canonical instant
        // alone, never from the rendering offset (`t == t.getutc` is true, so
        // they must hash alike).
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        t.num.hash(&mut h);
        t.den.hash(&mut h);
        Ok(RubyValue::Int(h.finish() as i64))
    }
    "sunday?"[0] => fn sunday_p(recv, args, _block) { arity!(args, 0); Ok(RubyValue::Bool(civil(recv_time(recv)).tm.tm_wday == 0)) }
    "monday?"[0] => fn monday_p(recv, args, _block) { arity!(args, 0); Ok(RubyValue::Bool(civil(recv_time(recv)).tm.tm_wday == 1)) }
    "tuesday?"[0] => fn tuesday_p(recv, args, _block) { arity!(args, 0); Ok(RubyValue::Bool(civil(recv_time(recv)).tm.tm_wday == 2)) }
    "wednesday?"[0] => fn wednesday_p(recv, args, _block) { arity!(args, 0); Ok(RubyValue::Bool(civil(recv_time(recv)).tm.tm_wday == 3)) }
    "thursday?"[0] => fn thursday_p(recv, args, _block) { arity!(args, 0); Ok(RubyValue::Bool(civil(recv_time(recv)).tm.tm_wday == 4)) }
    "friday?"[0] => fn friday_p(recv, args, _block) { arity!(args, 0); Ok(RubyValue::Bool(civil(recv_time(recv)).tm.tm_wday == 5)) }
    "saturday?"[0] => fn saturday_p(recv, args, _block) { arity!(args, 0); Ok(RubyValue::Bool(civil(recv_time(recv)).tm.tm_wday == 6)) }

    // `asctime`/`ctime`: the fixed C `ctime` shape, in the Time's own zone.
    "asctime"[0] | "ctime"[0] => fn asctime(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::collections::string_new(
            strftime(recv_time(recv), "%a %b %e %H:%M:%S %Y"),
        )))
    }
    // `[sec, min, hour, mday, mon, year, wday, yday, isdst, zone]`.
    "to_a"[0] => fn to_a(recv, args, _block) {
        arity!(args, 0);
        let c = civil(recv_time(recv));
        let zone = if c.zone.is_empty() {
            RubyValue::Nil
        } else {
            RubyValue::Str(crate::collections::string_new(c.zone.clone()))
        };
        Ok(RubyValue::Array(crate::collections::array_new(vec![
            RubyValue::Int(c.tm.tm_sec as i64),
            RubyValue::Int(c.tm.tm_min as i64),
            RubyValue::Int(c.tm.tm_hour as i64),
            RubyValue::Int(c.tm.tm_mday as i64),
            RubyValue::Int(c.tm.tm_mon as i64 + 1),
            RubyValue::Int(c.tm.tm_year as i64 + 1900),
            RubyValue::Int(c.tm.tm_wday as i64),
            RubyValue::Int(c.tm.tm_yday as i64 + 1),
            RubyValue::Bool(c.tm.tm_isdst > 0),
            zone,
        ])))
    }
    // The exact instant as `Rational` seconds since the epoch (always a
    // Rational, even for a whole second: `Time.at(100).to_r == (100/1)`).
    "to_r"[0] => fn to_r(recv, args, _block) {
        arity!(args, 0);
        let t = recv_time(recv);
        crate::builtins::rational::rational_new(t.num.clone(), t.den.clone())
    }
    "round" => fn round(recv, args, _block) {
        arity!(args, 0..=1);
        time_reduce(recv_time(recv), args, Rounding::Round)
    }
    "floor" => fn floor(recv, args, _block) {
        arity!(args, 0..=1);
        time_reduce(recv_time(recv), args, Rounding::Floor)
    }
    "ceil" => fn ceil(recv, args, _block) {
        arity!(args, 0..=1);
        time_reduce(recv_time(recv), args, Rounding::Ceil)
    }
    // ISO 8601 / `xmlschema`: `YYYY-MM-DDTHH:MM:SS`, an optional `.fff`
    // fractional part (`fraction_digits`), and the zone (`Z` for UTC else
    // `+HH:MM`).
    "xmlschema" | "iso8601" => fn xmlschema(recv, args, _block) {
        arity!(args, 0..=1);
        let t = recv_time(recv);
        let mut s = strftime(t, "%Y-%m-%dT%H:%M:%S");
        let digits = round_ndigits(args)?;
        if digits > 0 {
            use num_integer::Integer;
            let (n, d) = t.frac();
            let scaled = (n * num_bigint::BigInt::from(10u32).pow(digits)).div_floor(&d);
            s.push('.');
            s.push_str(&format!("{frac:0>width$}", frac = scaled.to_string(), width = digits as usize));
        }
        // A UTC-flagged Time uses `Z`; a fixed numeric offset (including
        // `+00:00`) and a local Time both spell out the offset.
        if t.is_utc() {
            s.push('Z');
        } else {
            s.push_str(&offset_str_colon(civil(t).offset, 1));
        }
        Ok(RubyValue::Str(crate::collections::string_new(s)))
    }
    // A pattern-matching view: `nil` -> every field, an Array -> only the
    // requested keys (in the requested order), CRuby's shape.
    "deconstruct_keys"[1] => fn deconstruct_keys(recv, args, _block) {
        arity!(args, 1);
        let all = time_field_pairs(recv_time(recv));
        let pairs: Vec<(RubyValue, RubyValue)> = match &args[0] {
            RubyValue::Nil => all
                .into_iter()
                .map(|(k, v)| (RubyValue::Symbol(crate::Symbol::intern(k)), v))
                .collect(),
            RubyValue::Array(keys) => {
                let mut out = Vec::new();
                for key in keys.lock().iter() {
                    if let RubyValue::Symbol(s) = key {
                        let name = s.name();
                        if let Some((_, v)) = all.iter().find(|(k, _)| *k == name.as_str()) {
                            out.push((key.clone(), v.clone()));
                        }
                    }
                }
                out
            }
            other => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "wrong argument type {} (expected Array or nil)",
                        crate::builtins::class_name_of(other)
                    ),
                ))
            }
        };
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed instant: 2023-11-14 22:13:20 UTC. Every assertion below was
    /// read off `ruby 4.0.5` for this same epoch second.
    const EPOCH: i64 = 1_700_000_000;

    fn utc_at(sec: i64) -> RubyValue {
        time_value(sec, 0, Some(RTime::UTC))
    }

    #[test]
    fn to_i_and_to_f_answer_the_epoch() {
        let t = utc_at(EPOCH);
        assert!(matches!(to_i(&t, &[], None).unwrap(), RubyValue::Int(EPOCH)));
        let RubyValue::Float(f) = to_f(&t, &[], None).unwrap() else {
            panic!()
        };
        assert_eq!(f, EPOCH as f64);
    }

    /// The civil fields of a UTC Time -- zone-independent, so this is safe to
    /// assert regardless of the machine's TZ.
    #[test]
    fn utc_civil_fields_match_the_oracle() {
        let t = utc_at(EPOCH);
        let f = |g: fn(&RubyValue, &[RubyValue], Option<RubyValue>) -> Result<RubyValue, Signal>| {
            let RubyValue::Int(i) = g(&t, &[], None).unwrap() else {
                panic!()
            };
            i
        };
        assert_eq!(f(year), 2023);
        assert_eq!(f(month), 11);
        assert_eq!(f(day), 14);
        assert_eq!(f(hour), 22);
        assert_eq!(f(min), 13);
        assert_eq!(f(sec), 20);
        assert_eq!(f(wday), 2, "a Tuesday");
        assert_eq!(f(yday), 318);
        assert_eq!(f(utc_offset), 0);
    }

    #[test]
    fn utc_renders_with_a_utc_suffix() {
        let t = utc_at(EPOCH);
        assert_eq!(
            to_s(&t, &[], None).unwrap().to_display_string(),
            "2023-11-14 22:13:20 UTC"
        );
        assert_eq!(
            zone(&t, &[], None).unwrap().to_display_string(),
            "UTC"
        );
        assert!(matches!(utc_p(&t, &[], None).unwrap(), RubyValue::Bool(true)));
    }

    #[test]
    fn the_epoch_itself_renders_as_1970() {
        assert_eq!(
            to_s(&utc_at(0), &[], None).unwrap().to_display_string(),
            "1970-01-01 00:00:00 UTC"
        );
    }

    /// A fixed-offset Time renders `-0500` and has no zone NAME.
    #[test]
    fn a_fixed_offset_time_renders_its_offset_and_has_no_zone_name() {
        let t = time_value(EPOCH, 0, Some(-5 * 3600));
        assert_eq!(
            to_s(&t, &[], None).unwrap().to_display_string(),
            "2023-11-14 17:13:20 -0500"
        );
        assert!(matches!(zone(&t, &[], None).unwrap(), RubyValue::Nil));
        assert!(matches!(utc_p(&t, &[], None).unwrap(), RubyValue::Bool(false)));
    }

    /// The strftime directives, against the oracle's own output for this
    /// instant in UTC.
    #[test]
    fn strftime_directives_match_the_oracle() {
        let t = utc_at(EPOCH);
        let f = |fmt: &str| {
            let arg = RubyValue::Str(crate::collections::string_new(fmt.to_string()));
            strftime_row(&t, &[arg], None).unwrap().to_display_string()
        };
        assert_eq!(f("%Y-%m-%d %H:%M:%S"), "2023-11-14 22:13:20");
        assert_eq!(f("%F %T"), "2023-11-14 22:13:20");
        assert_eq!(f("%z"), "+0000");
        assert_eq!(f("%Z"), "UTC");
        assert_eq!(f("%j"), "318");
        assert_eq!(f("%a %A"), "Tue Tuesday");
        assert_eq!(f("%b %B"), "Nov November");
        assert_eq!(f("%p %I"), "PM 10");
        assert_eq!(f("%y %C"), "23 20");
        assert_eq!(f("%s"), "1700000000");
        assert_eq!(f("%u %w"), "2 2");
    }

    /// Ruby's padding flags: `-` drops it, `_` uses spaces.
    #[test]
    fn strftime_padding_flags() {
        let t = utc_at(EPOCH);
        let f = |fmt: &str| {
            let arg = RubyValue::Str(crate::collections::string_new(fmt.to_string()));
            strftime_row(&t, &[arg], None).unwrap().to_display_string()
        };
        assert_eq!(f("%-m/%-d"), "11/14");
        assert_eq!(f("%-H"), "22");
        // A single-digit field is where the flags actually differ.
        let jan = utc_at(1_704_067_200); // 2024-01-01 00:00:00 UTC
        let g = |fmt: &str| {
            let arg = RubyValue::Str(crate::collections::string_new(fmt.to_string()));
            strftime_row(&jan, &[arg], None).unwrap().to_display_string()
        };
        assert_eq!(g("%m"), "01");
        assert_eq!(g("%-m"), "1");
        assert_eq!(g("%_m"), " 1");
    }

    /// A literal `%%` is one percent; an unknown directive comes out verbatim
    /// rather than raising (Ruby's own behavior).
    #[test]
    fn strftime_handles_percent_and_unknown_directives() {
        let t = utc_at(EPOCH);
        let f = |fmt: &str| {
            let arg = RubyValue::Str(crate::collections::string_new(fmt.to_string()));
            strftime_row(&t, &[arg], None).unwrap().to_display_string()
        };
        assert_eq!(f("100%%"), "100%");
        assert_eq!(f("%Q"), "%Q");
    }

    /// `t + n` is a Time; `t - other` is a Float of seconds; `t - n` is a Time.
    #[test]
    fn arithmetic_picks_its_answer_from_the_argument() {
        let t = utc_at(EPOCH);
        let later = plus(&t, &[RubyValue::Int(60)], None).unwrap();
        assert!(matches!(
            to_i(&later, &[], None).unwrap(),
            RubyValue::Int(x) if x == EPOCH + 60
        ));

        let RubyValue::Float(d) = minus(&utc_at(100), &[utc_at(40)], None).unwrap() else {
            panic!("Time - Time is a Float")
        };
        assert_eq!(d, 60.0);

        let earlier = minus(&t, &[RubyValue::Int(20)], None).unwrap();
        assert!(matches!(
            to_i(&earlier, &[], None).unwrap(),
            RubyValue::Int(x) if x == EPOCH - 20
        ));
    }

    /// Sub-second arithmetic normalizes nsec across the second boundary.
    ///
    /// The exact values are the ORACLE's for these same expressions, and they
    /// are not the naive ones: `Time.at(10.8) - 0.9` is nsec 900000000 rather
    /// than 899999999 precisely because the receiver keeps 10.8's true value
    /// (10.8000000000000007105...), not a nanosecond rounding of it. An
    /// earlier `(sec, nsec)` representation got this wrong; see `RTime::num`.
    #[test]
    fn sub_second_arithmetic_carries_and_borrows() {
        // Exactly 10.8s (integral nanoseconds), not the double 10.8.
        let t = time_value(10, 800_000_000, Some(0));
        let sum = plus(&t, &[RubyValue::Float(0.5)], None).unwrap();
        assert!(matches!(to_i(&sum, &[], None).unwrap(), RubyValue::Int(11)));
        assert!(matches!(nsec(&sum, &[], None).unwrap(), RubyValue::Int(300_000_000)));

        // The DOUBLE 10.8 -- whose tail survives into the difference.
        let from_float =
            time_at(&RubyValue::Class(TIME_CLASS), &[RubyValue::Float(10.8)], None).unwrap();
        let diff = minus(&from_float, &[RubyValue::Float(0.9)], None).unwrap();
        assert!(matches!(to_i(&diff, &[], None).unwrap(), RubyValue::Int(9)));
        assert!(matches!(nsec(&diff, &[], None).unwrap(), RubyValue::Int(900_000_000)));
    }

    /// A Float epoch is stored EXACTLY, so `subsec` answers the double's true
    /// fraction (denominator a power of two) rather than a nanosecond
    /// approximation -- `Time.at(10.8).subsec` is
    /// `(225179981368525/281474976710656)`, oracle-verified, and notably NOT
    /// `Rational(8, 10)`.
    #[test]
    fn a_float_epoch_keeps_its_exact_fraction() {
        let t = time_at(&RubyValue::Class(TIME_CLASS), &[RubyValue::Float(10.8)], None).unwrap();
        assert_eq!(
            subsec(&t, &[], None).unwrap().inspect_string(),
            "(225179981368525/281474976710656)"
        );
        // ...while `nsec` is the truncated VIEW of that same fraction.
        assert!(matches!(nsec(&t, &[], None).unwrap(), RubyValue::Int(800_000_000)));
    }

    #[test]
    fn at_accepts_a_float_and_keeps_the_fraction() {
        let t = time_at(&RubyValue::Class(TIME_CLASS), &[RubyValue::Float(1_700_000_000.5)], None)
            .unwrap();
        let RubyValue::Float(f) = to_f(&t, &[], None).unwrap() else {
            panic!()
        };
        assert_eq!(f, 1_700_000_000.5);
        assert!(matches!(nsec(&t, &[], None).unwrap(), RubyValue::Int(500_000_000)));
        assert!(matches!(usec(&t, &[], None).unwrap(), RubyValue::Int(500_000)));
    }

    /// `inspect` shows sub-second digits (trimmed) where `to_s` doesn't.
    #[test]
    fn inspect_shows_trimmed_subseconds_and_to_s_does_not() {
        let t = time_value(EPOCH, 500_000_000, Some(RTime::UTC));
        assert_eq!(
            inspect(&t, &[], None).unwrap().to_display_string(),
            "2023-11-14 22:13:20.5 UTC"
        );
        assert_eq!(
            to_s(&t, &[], None).unwrap().to_display_string(),
            "2023-11-14 22:13:20 UTC"
        );
    }

    #[test]
    fn utc_constructor_round_trips_through_to_i() {
        let cls = RubyValue::Class(TIME_CLASS);
        let t = time_utc(
            &cls,
            &[
                RubyValue::Int(2023),
                RubyValue::Int(11),
                RubyValue::Int(14),
                RubyValue::Int(22),
                RubyValue::Int(13),
                RubyValue::Int(20),
            ],
            None,
        )
        .unwrap();
        assert!(matches!(to_i(&t, &[], None).unwrap(), RubyValue::Int(EPOCH)));
        assert_eq!(
            to_s(&t, &[], None).unwrap().to_display_string(),
            "2023-11-14 22:13:20 UTC"
        );
    }

    /// `<=>` orders by instant and drives Comparable; `==` ignores the
    /// rendering offset (`t == t.getutc`).
    #[test]
    fn comparison_is_by_instant_not_by_offset() {
        let a = utc_at(EPOCH);
        let b = utc_at(EPOCH + 1);
        assert!(matches!(cmp(&a, &[b.clone()], None).unwrap(), RubyValue::Int(-1)));
        assert!(matches!(cmp(&b, &[a.clone()], None).unwrap(), RubyValue::Int(1)));
        assert!(matches!(cmp(&a, &[a.clone()], None).unwrap(), RubyValue::Int(0)));

        let same_instant_other_offset = time_value(EPOCH, 0, Some(-5 * 3600));
        assert!(matches!(
            eq(&a, &[same_instant_other_offset.clone()], None).unwrap(),
            RubyValue::Bool(true)
        ));
        // Equal Times hash equally.
        assert_eq!(
            hash(&a, &[], None).unwrap().inspect_string(),
            hash(&same_instant_other_offset, &[], None).unwrap().inspect_string()
        );
    }

    /// `<=>` against a non-Time is nil, which is what makes Comparable raise
    /// its "comparison failed" ArgumentError rather than crash.
    #[test]
    fn comparison_against_a_non_time_is_nil() {
        assert!(matches!(
            cmp(&utc_at(EPOCH), &[RubyValue::Int(5)], None).unwrap(),
            RubyValue::Nil
        ));
        assert!(matches!(
            eq(&utc_at(EPOCH), &[RubyValue::Int(5)], None).unwrap(),
            RubyValue::Bool(false)
        ));
    }

    #[test]
    fn getutc_answers_a_utc_copy_without_mutating() {
        let local = time_value(EPOCH, 0, None);
        let u = getutc(&local, &[], None).unwrap();
        assert!(matches!(utc_p(&u, &[], None).unwrap(), RubyValue::Bool(true)));
        // Same instant.
        assert!(matches!(to_i(&u, &[], None).unwrap(), RubyValue::Int(EPOCH)));
        // The receiver is untouched.
        assert!(matches!(utc_p(&local, &[], None).unwrap(), RubyValue::Bool(false)));
    }

    #[test]
    fn weekday_predicates() {
        let t = utc_at(EPOCH); // a Tuesday
        assert!(matches!(tuesday_p(&t, &[], None).unwrap(), RubyValue::Bool(true)));
        assert!(matches!(monday_p(&t, &[], None).unwrap(), RubyValue::Bool(false)));
        assert!(matches!(sunday_p(&t, &[], None).unwrap(), RubyValue::Bool(false)));
    }

    #[test]
    fn now_is_after_the_fixed_epoch_and_is_local() {
        let n = time_now(&RubyValue::Class(TIME_CLASS), &[], None).unwrap();
        let RubyValue::Int(secs) = to_i(&n, &[], None).unwrap() else {
            panic!()
        };
        assert!(secs > EPOCH, "clock is before 2023");
        assert!(matches!(utc_p(&n, &[], None).unwrap(), RubyValue::Bool(false)));
    }

    #[test]
    fn lookup_tables_find_their_names() {
        assert!(lookup_class("now").is_some());
        assert!(lookup_class("at").is_some());
        assert!(lookup_class("utc").is_some());
        assert!(lookup_class("nope").is_none());
        assert!(lookup("strftime").is_some());
        assert!(lookup("to_i").is_some());
        assert!(lookup("nope").is_none());
    }
}

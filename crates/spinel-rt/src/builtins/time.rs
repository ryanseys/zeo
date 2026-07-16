//! `Time` (CRuby time.c) -- an instant, as seconds+nanoseconds since the
//! epoch plus the offset it renders in.
//!
//! Backed by libc (`localtime_r`/`timegm`/`tzset`), NOT a datetime crate:
//! the OS already owns the zone database and the DST rules, and Ruby's
//! `strftime` directive set and zone semantics are specific enough that a
//! crate's own opinions would have to be fought rather than used. The
//! civil<->epoch conversion is libc's; the strftime directive table below is
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
    /// distinct from `Some(0)` (explicitly UTC), because a local Time's
    /// offset depends on its own instant (DST), so it can't be baked in at
    /// construction. `utc?` is `self.offset() == Some(0)`.
    ///
    /// Interior-mutable because CRuby's `Time#utc`/`#gmtime`/`#localtime`
    /// convert the receiver IN PLACE and answer self (as opposed to the
    /// `get*` copies) -- which callers observe: `t.utc; t.to_s` renders UTC.
    /// Only the RENDERING zone is mutable; the instant never changes, which
    /// is why `==`/`hash`/`<=>` are unaffected by it.
    offset: parking_lot::Mutex<Option<i32>>,
}

impl RTime {
    fn offset(&self) -> Option<i32> {
        *self.offset.lock()
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

/// The broken-down civil fields of an instant, in the Time's own zone.
struct Civil {
    tm: libc::tm,
    /// The zone offset actually in effect at this instant (DST-resolved for
    /// a local Time).
    offset: i32,
    /// The zone abbreviation (`"EST"`), empty for a fixed-offset Time.
    zone: String,
}

/// Break `t` into civil fields. A UTC/fixed-offset Time shifts the epoch
/// seconds and reads them with `gmtime_r`; a local one asks `localtime_r`,
/// which is what resolves DST and names the zone.
fn civil(t: &RTime) -> Civil {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    match t.offset() {
        Some(off) => {
            let shifted = t.sec() + off as i64;
            // SAFETY: `shifted` is a valid time_t and `tm` a valid out-param.
            unsafe { libc::gmtime_r(&shifted, &mut tm) };
            let zone = if off == 0 { "UTC".to_string() } else { String::new() };
            Civil { tm, offset: off, zone }
        }
        None => {
            let secs = t.sec();
            // `localtime_r` initializes the zone state from TZ itself on
            // both glibc and macOS, so no explicit `tzset` is needed (and
            // it is the reentrant reader, unlike `localtime`).
            // SAFETY: valid time_t in, valid out-param.
            unsafe { libc::localtime_r(&secs, &mut tm) };
            let offset = tm.tm_gmtoff as i32;
            // SAFETY: `tm_zone` points into libc's static zone strings after
            // localtime_r; valid for the process lifetime, NUL-terminated.
            let zone = if tm.tm_zone.is_null() {
                String::new()
            } else {
                unsafe { std::ffi::CStr::from_ptr(tm.tm_zone) }
                    .to_string_lossy()
                    .into_owned()
            };
            Civil { tm, offset, zone }
        }
    }
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
    let tail = if t.offset() == Some(0) {
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
/// libc's own strftime lacks Ruby's flags and several directives).
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
        // Flags, then the directive.
        let mut pad: Option<char> = None;
        while let Some(&f) = chars.peek() {
            match f {
                '-' | '0' | '_' => {
                    pad = Some(f);
                    chars.next();
                }
                _ => break,
            }
        }
        let Some(d) = chars.next() else {
            out.push('%');
            break;
        };
        // `num` applies the flag to a numeric directive: `-` drops padding,
        // `_` pads with spaces, otherwise zero-padded to `width`.
        let num = |v: i64, width: usize| -> String {
            match pad {
                Some('-') => v.to_string(),
                Some('_') => format!("{:>width$}", v, width = width),
                _ => format!("{:0width$}", v, width = width),
            }
        };
        match d {
            'Y' => out.push_str(&(tm.tm_year as i64 + 1900).to_string()),
            'y' => out.push_str(&num((tm.tm_year as i64 + 1900) % 100, 2)),
            'C' => out.push_str(&num((tm.tm_year as i64 + 1900) / 100, 2)),
            'm' => out.push_str(&num(tm.tm_mon as i64 + 1, 2)),
            'd' => out.push_str(&num(tm.tm_mday as i64, 2)),
            'e' => out.push_str(&format!("{:>2}", tm.tm_mday)),
            'j' => out.push_str(&num(tm.tm_yday as i64 + 1, 3)),
            'H' => out.push_str(&num(tm.tm_hour as i64, 2)),
            'k' => out.push_str(&format!("{:>2}", tm.tm_hour)),
            'I' => {
                let h12 = match tm.tm_hour % 12 {
                    0 => 12,
                    h => h,
                };
                out.push_str(&num(h12 as i64, 2));
            }
            'l' => {
                let h12 = match tm.tm_hour % 12 {
                    0 => 12,
                    h => h,
                };
                out.push_str(&format!("{:>2}", h12));
            }
            'M' => out.push_str(&num(tm.tm_min as i64, 2)),
            'S' => out.push_str(&num(tm.tm_sec as i64, 2)),
            'L' => out.push_str(&format!("{:03}", t.nsec() / 1_000_000)),
            'N' => out.push_str(&format!("{:09}", t.nsec())),
            'z' => out.push_str(&offset_str(c.offset, false)),
            'Z' => out.push_str(&c.zone),
            'a' => out.push_str(&DAY_NAMES[tm.tm_wday as usize][..3]),
            'A' => out.push_str(DAY_NAMES[tm.tm_wday as usize]),
            'b' | 'h' => out.push_str(&MONTH_NAMES[tm.tm_mon as usize][..3]),
            'B' => out.push_str(MONTH_NAMES[tm.tm_mon as usize]),
            'p' => out.push_str(if tm.tm_hour < 12 { "AM" } else { "PM" }),
            'P' => out.push_str(if tm.tm_hour < 12 { "am" } else { "pm" }),
            'u' => out.push_str(&(if tm.tm_wday == 0 { 7 } else { tm.tm_wday as i64 }).to_string()),
            'w' => out.push_str(&(tm.tm_wday as i64).to_string()),
            's' => out.push_str(&t.sec().to_string()),
            // The compound directives, in terms of the above.
            'F' => out.push_str(&strftime(t, "%Y-%m-%d")),
            'T' | 'X' => out.push_str(&strftime(t, "%H:%M:%S")),
            'D' | 'x' => out.push_str(&strftime(t, "%m/%d/%y")),
            'R' => out.push_str(&strftime(t, "%H:%M")),
            'r' => out.push_str(&strftime(t, "%I:%M:%S %p")),
            'c' => out.push_str(&strftime(t, "%a %b %e %H:%M:%S %Y")),
            'n' => out.push('\n'),
            't' => out.push('\t'),
            '%' => out.push('%'),
            // Unknown: emit verbatim, `%` included (Ruby's own behavior --
            // it does not raise).
            other => {
                out.push('%');
                out.push(other);
            }
        }
    }
    out
}

/// A Float's seconds/nanoseconds, computed EXACTLY from the double's own
/// binary value rather than in floating point.
///
/// This is the difference between `Time.at(0.7).nsec` answering 699999999
/// (Ruby, and now us) and 700000000 (what `((f - f.floor()) * 1e9) as u32`
/// gives): the subtraction and multiply each round back to the nearest
/// double, landing exactly on 0.7e9 and erasing the deficit the literal
/// actually carries. So the fraction is taken over exact integers instead --
/// `nsec = floor(rem * 1e9 / den)`, where `num/den` IS the double.
fn float_epoch(f: f64) -> (i64, u32) {
    use num_bigint::BigInt;
    use num_integer::Integer;
    let (num, den) = crate::builtins::float::float_exact_parts(f);
    // Floored division, so a negative epoch still leaves a remainder in
    // `0..den` -- Ruby's own normalization (`Time.at(-0.5)` is second -1
    // plus 500000000ns, not second 0 minus half).
    let (sec, rem) = num.div_mod_floor(&den);
    let nsec = (rem * BigInt::from(1_000_000_000u32)) / den;
    let sec = i64::try_from(sec).unwrap_or(i64::MAX);
    let nsec = u32::try_from(nsec).unwrap_or(999_999_999);
    (sec, nsec.min(999_999_999))
}

fn args_float_name(v: &RubyValue) -> String {
    v.to_display_string()
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
        RubyValue::Int(i) => (BigInt::from(*i), BigInt::from(1)),
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
    use num_bigint::BigInt;
    match v {
        RubyValue::Int(i) => Ok((BigInt::from(*i), BigInt::from(1))),
        RubyValue::Float(f) if f.is_finite() => Ok(crate::builtins::float::float_exact_parts(*f)),
        RubyValue::Float(f) => Err(raise_error(
            "FloatDomainError",
            RubyValue::Float(*f).to_display_string(),
        )),
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
            other => Err(raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Integer",
                    crate::builtins::class_name_of(other)
                ),
            )),
        })
        .collect()
}

/// A `utc_offset` in seconds, range-checked as real Ruby does: strictly
/// within a day either way (`Time.new(.., 86400)` is an ArgumentError, and
/// `86399` is fine) -- oracle-verified.
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

/// A `Time.utc`/`Time.local` 7th argument: MICROSECONDS, which real Ruby
/// range-checks (`Time.utc(2000,1,1,0,0,0,1000000)` is an ArgumentError, not
/// a silent carry into the next second).
fn usec_arg(v: Option<&RubyValue>) -> Result<u32, Signal> {
    match v {
        None => Ok(0),
        Some(RubyValue::Int(u)) if (0..1_000_000).contains(u) => Ok(*u as u32),
        Some(RubyValue::Int(_)) => Err(raise_error(
            "ArgumentError",
            "subsecx out of range".to_string(),
        )),
        Some(other) => Err(raise_error(
            "TypeError",
            format!(
                "no implicit conversion of {} into Integer",
                crate::builtins::class_name_of(other)
            ),
        )),
    }
}

/// `Time.utc(y, mo, d, h, mi, s)` -- civil fields to epoch seconds, via
/// libc's `timegm` (the UTC inverse of `gmtime`).
fn civil_to_epoch_utc(parts: &[i64]) -> i64 {
    let get = |i: usize, dflt: i64| parts.get(i).copied().unwrap_or(dflt);
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = (get(0, 1970) - 1900) as libc::c_int;
    tm.tm_mon = (get(1, 1) - 1) as libc::c_int;
    tm.tm_mday = get(2, 1) as libc::c_int;
    tm.tm_hour = get(3, 0) as libc::c_int;
    tm.tm_min = get(4, 0) as libc::c_int;
    tm.tm_sec = get(5, 0) as libc::c_int;
    // SAFETY: `tm` is fully initialized above; `timegm` reads it and
    // normalizes out-of-range fields (Ruby raises for those instead -- see
    // the TODO on `time_utc`).
    unsafe { libc::timegm(&mut tm) as i64 }
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
    "at" => fn time_at(_recv, args, _block) {
        arity!(args, 1..=2);
        let (num, den) = exact_seconds(&args[0])?;
        Ok(time_exact(num, den, None))
    }
    // `Time.utc(y, mo, d, h, mi, s)` / `Time.gm(...)`. A 7th argument is
    // MICROSECONDS (not the offset -- that is `Time.new`'s 7th; the two
    // constructors genuinely differ, oracle-verified).
    // TODO(plan P-B): out-of-range fields (`Time.utc(2023, 13, 1)`) are
    // normalized by libc's timegm (-> 2024-01-01); real Ruby raises
    // ArgumentError ("mon out of range"). Needs a range check per field
    // before the call. Also unsupported: the string-month form
    // (`Time.utc(2023, "nov", 1)`) and the 10-argument to_a-style form.
    "utc" | "gm" => fn time_utc(_recv, args, _block) {
        arity!(args, 1..=7);
        let parts = int_parts(args, 6)?;
        let usec = usec_arg(args.get(6))?;
        Ok(time_value(civil_to_epoch_utc(&parts), usec * 1000, Some(0)))
    }
    // `Time.new(y, mo, d, h, mi, s, utc_offset)` -- the 7th argument is the
    // OFFSET, in seconds or as a `"+HH:MM"` String, unlike `Time.utc`'s
    // microseconds. With no offset given it is local time, like `Time.local`.
    // TODO(plan P-B): the `in:` keyword form isn't handled.
    "new" => fn time_new(recv, args, _block) {
        arity!(args, 0..=7);
        if args.is_empty() {
            return time_now(recv, &[], None);
        }
        let parts = int_parts(args, 6)?;
        let as_utc = civil_to_epoch_utc(&parts);
        match args.get(6) {
            None => {
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
                let off = parse_offset(&s.lock())?;
                Ok(time_value(as_utc - off as i64, 0, Some(off)))
            }
            Some(other) => Err(raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into Integer (utc_offset)",
                    crate::builtins::class_name_of(other)
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
        arity!(args, 1..=7);
        let parts = int_parts(args, 6)?;
        let usec = usec_arg(args.get(6))?;
        let as_utc = civil_to_epoch_utc(&parts);
        let probe = RTime { num: num_bigint::BigInt::from(as_utc), den: num_bigint::BigInt::from(1), offset: parking_lot::Mutex::new(None) };
        let off = civil(&probe).offset as i64;
        Ok(time_value(as_utc - off, usec * 1000, None))
    }
}

builtin_methods! {
    pub(crate) fn lookup;

    "to_i" | "tv_sec" => fn to_i(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_time(recv).sec()))
    }
    "to_f" => fn to_f(recv, args, _block) {
        arity!(args, 0);
        let t = recv_time(recv);
        // From the exact rational, not from `sec + nsec/1e9`: that rounds
        // twice and can't round-trip a Float epoch (`Time.at(1.25).to_f`).
        let (n, d) = (t.num.clone(), t.den.clone());
        Ok(RubyValue::Float(bigint_to_f64(&n) / bigint_to_f64(&d)))
    }
    "nsec" | "tv_nsec" => fn nsec(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_time(recv).nsec() as i64))
    }
    "usec" | "tv_usec" => fn usec(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int((recv_time(recv).nsec() / 1000) as i64))
    }
    // The fraction of a second, EXACTLY: a Rational (`Time.at(0.5).subsec`
    // is `(1/2)`, not 0.5), or Integer 0 for a whole second -- oracle-
    // verified. `rational_new` reduces, which is what turns 500000000/1e9
    // into 1/2.
    "subsec" => fn subsec(recv, args, _block) {
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
    "year" => fn year(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(civil(recv_time(recv)).tm.tm_year as i64 + 1900))
    }
    "month" | "mon" => fn month(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(civil(recv_time(recv)).tm.tm_mon as i64 + 1))
    }
    "day" | "mday" => fn day(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(civil(recv_time(recv)).tm.tm_mday as i64))
    }
    "hour" => fn hour(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(civil(recv_time(recv)).tm.tm_hour as i64))
    }
    "min" => fn min(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(civil(recv_time(recv)).tm.tm_min as i64))
    }
    "sec" => fn sec(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(civil(recv_time(recv)).tm.tm_sec as i64))
    }
    "wday" => fn wday(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(civil(recv_time(recv)).tm.tm_wday as i64))
    }
    "yday" => fn yday(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(civil(recv_time(recv)).tm.tm_yday as i64 + 1))
    }
    "utc_offset" | "gmt_offset" | "gmtoff" => fn utc_offset(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(civil(recv_time(recv)).offset as i64))
    }
    "zone" => fn zone(recv, args, _block) {
        arity!(args, 0);
        let z = civil(recv_time(recv)).zone;
        // A fixed-offset (non-UTC) Time has no zone NAME -- nil, not "".
        if z.is_empty() {
            return Ok(RubyValue::Nil);
        }
        Ok(RubyValue::Str(crate::collections::string_new(z)))
    }
    "utc?" | "gmt?" => fn utc_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv_time(recv).offset() == Some(0)))
    }
    // The MUTATING converters: they change which zone the receiver RENDERS
    // in and answer self, leaving the instant alone. Callers observe the
    // mutation (`t.utc; t.to_s` renders UTC), which is why `offset` is
    // interior-mutable -- see `RTime`.
    "utc" | "gmtime" => fn to_utc_bang(recv, args, _block) {
        arity!(args, 0);
        *recv_time(recv).offset.lock() = Some(0);
        Ok(recv.clone())
    }
    "localtime" => fn to_local_bang(recv, args, _block) {
        arity!(args, 0..=1);
        *recv_time(recv).offset.lock() = None;
        Ok(recv.clone())
    }
    // ...and their non-mutating counterparts, which answer a fresh Time.
    "getutc" | "getgm" => fn getutc(recv, args, _block) {
        arity!(args, 0);
        let t = recv_time(recv);
        Ok(time_value(t.sec(), t.nsec(), Some(0)))
    }
    "getlocal" => fn getlocal(recv, args, _block) {
        arity!(args, 0);
        let t = recv_time(recv);
        Ok(time_value(t.sec(), t.nsec(), None))
    }
    "to_s" => fn to_s(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::collections::string_new(render(recv_time(recv), false))))
    }
    "inspect" => fn inspect(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::collections::string_new(render(recv_time(recv), true))))
    }
    "strftime" => fn strftime_row(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Str(f) = &args[0] else {
            return Err(raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into String",
                    crate::builtins::class_name_of(&args[0])
                ),
            ));
        };
        let fmt = f.lock().clone();
        Ok(RubyValue::Str(crate::collections::string_new(strftime(recv_time(recv), &fmt))))
    }
    // `t + n` -> a Time n seconds later; `t - other_time` -> a Float count of
    // seconds BETWEEN them, but `t - n` -> a Time. The argument's type picks.
    "+" => fn plus(recv, args, _block) {
        arity!(args, 1);
        shift(recv_time(recv), &args[0], 1)
    }
    "-" => fn minus(recv, args, _block) {
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
    "<=>" => fn cmp(recv, args, _block) {
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
    "==" | "eql?" => fn eq(recv, args, _block) {
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
    "hash" => fn hash(recv, args, _block) {
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
    "sunday?" => fn sunday_p(recv, args, _block) { arity!(args, 0); Ok(RubyValue::Bool(civil(recv_time(recv)).tm.tm_wday == 0)) }
    "monday?" => fn monday_p(recv, args, _block) { arity!(args, 0); Ok(RubyValue::Bool(civil(recv_time(recv)).tm.tm_wday == 1)) }
    "tuesday?" => fn tuesday_p(recv, args, _block) { arity!(args, 0); Ok(RubyValue::Bool(civil(recv_time(recv)).tm.tm_wday == 2)) }
    "wednesday?" => fn wednesday_p(recv, args, _block) { arity!(args, 0); Ok(RubyValue::Bool(civil(recv_time(recv)).tm.tm_wday == 3)) }
    "thursday?" => fn thursday_p(recv, args, _block) { arity!(args, 0); Ok(RubyValue::Bool(civil(recv_time(recv)).tm.tm_wday == 4)) }
    "friday?" => fn friday_p(recv, args, _block) { arity!(args, 0); Ok(RubyValue::Bool(civil(recv_time(recv)).tm.tm_wday == 5)) }
    "saturday?" => fn saturday_p(recv, args, _block) { arity!(args, 0); Ok(RubyValue::Bool(civil(recv_time(recv)).tm.tm_wday == 6)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed instant: 2023-11-14 22:13:20 UTC. Every assertion below was
    /// read off `ruby 4.0.5` for this same epoch second.
    const EPOCH: i64 = 1_700_000_000;

    fn utc_at(sec: i64) -> RubyValue {
        time_value(sec, 0, Some(0))
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
        let t = time_value(EPOCH, 500_000_000, Some(0));
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

//! `date` (CRuby's bundled `date` gem) -- `Date` and `DateTime`.
//! `require "date"` activates it (a built-in feature; see `ext/mod.rs`).
//!
//! The backing object mirrors `ext/date/date_core.c`'s tagged union: a
//! *simple* date is a bare Julian Day Number, a *complex* one adds a day
//! fraction (`df` seconds + `sf` nanoseconds) and a UTC offset (`of`). Both
//! shapes live in one [`RDate`] behind a `complex` flag, because the flag is
//! NOT the class: `Date#to_datetime` answers a **simple** `DateTime`
//! (`date_core.c:date_to_datetime` copies the simple half through), and
//! `Date + Rational(1,2)` answers a **complex** `Date`.
//!
//! `jd` and `df` are stored as UTC, exactly as CRuby stores them; the local
//! fields every accessor reads come from `RDate::local`, which folds `of` in.
//!
//! `sg` is the calendar-reform start day (`Date::ITALY` by default), so dates
//! before 1582-10-15 render on the Julian calendar -- `Date.new(1,1,1).jd` is
//! 1721424, not the proleptic-Gregorian 1721426.

mod datetime;
mod parse;
mod strptime;

use crate::builtins::{arg_error, type_error};
use crate::dispatch::{RObj, RubyObject};
use crate::{ClassId, RubyValue, Signal, string_new};
use num_bigint::BigInt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zeo_abi::{DATE_CLASS, DATETIME_CLASS};
use zeo_macros::ruby_class;

/// `Date::ITALY` -- the Gregorian reform as Italy took it (1582-10-15), and
/// the default `start` for every constructor.
const ITALY: f64 = 2299161.0;
/// `Date::ENGLAND` -- 1752-09-14.
const ENGLAND: f64 = 2361222.0;

const SECS_PER_DAY: i64 = 86_400;
const NS_PER_SEC: i128 = 1_000_000_000;
const NS_PER_DAY: i128 = 86_400 * NS_PER_SEC;
/// JDN of the Unix epoch (1970-01-01).
const EPOCH_JD: i64 = 2_440_588;

pub struct RDate {
    /// Julian Day Number, **as UTC**.
    jd: i64,
    /// Seconds into the day, **as UTC** (0..86399).
    df: i32,
    /// Nanoseconds (0..999_999_999).
    sf: i32,
    /// Offset east of UTC, in seconds.
    of: i32,
    /// Calendar-reform start day: the first JDN rendered on the Gregorian
    /// calendar. `+INF` is `Date::JULIAN`, `-INF` is `Date::GREGORIAN`.
    sg: f64,
    /// CRuby's `ComplexDateData` tag -- carries a time of day and an offset.
    complex: bool,
    class_id: ClassId,
    frozen: AtomicBool,
}

impl RDate {
    fn simple(jd: i64, sg: f64, class_id: ClassId) -> Arc<RDate> {
        RDate::raw(jd, 0, 0, 0, sg, false, class_id)
    }

    fn raw(
        jd: i64,
        df: i32,
        sf: i32,
        of: i32,
        sg: f64,
        complex: bool,
        class_id: ClassId,
    ) -> Arc<RDate> {
        Arc::new(RDate {
            jd,
            df,
            sf,
            of,
            sg,
            complex,
            class_id,
            frozen: AtomicBool::new(false),
        })
    }

    /// The `(jd, seconds-into-day)` pair every field accessor reads: UTC
    /// shifted by the offset (`date_core.c:jd_utc_to_local`).
    fn local(&self) -> (i64, i32) {
        let t = self.df as i64 + self.of as i64;
        (
            self.jd + t.div_euclid(SECS_PER_DAY),
            t.rem_euclid(SECS_PER_DAY) as i32,
        )
    }

    fn local_jd(&self) -> i64 {
        self.local().0
    }

    fn civil(&self) -> (i64, i64, i64) {
        jd_to_civil(self.local_jd(), self.sg)
    }

    /// Local `(hour, min, sec)`.
    fn hms(&self) -> (i32, i32, i32) {
        let s = self.local().1;
        (s / 3600, s % 3600 / 60, s % 60)
    }

    /// The instant as a count of nanoseconds since JDN 0, in UTC -- the one
    /// exact scale `+`, `-`, `<=>`, `ajd` and `amjd` all work on.
    fn total_ns(&self) -> i128 {
        self.jd as i128 * NS_PER_DAY + self.df as i128 * NS_PER_SEC + self.sf as i128
    }

    /// Epoch seconds (UTC), for `%s`/`%Q`.
    fn epoch(&self) -> i64 {
        (self.jd - EPOCH_JD) * SECS_PER_DAY + self.df as i64
    }

    /// The same instant rebuilt from an exact nanosecond count, keeping this
    /// date's offset, reform start and class. Stays simple only if the result
    /// lands on midnight AND this one was simple already.
    fn at_ns(&self, ns: i128) -> Arc<RDate> {
        let jd = ns.div_euclid(NS_PER_DAY);
        let rem = ns.rem_euclid(NS_PER_DAY);
        let df = (rem / NS_PER_SEC) as i32;
        let sf = (rem % NS_PER_SEC) as i32;
        let complex = self.complex || df != 0 || sf != 0;
        RDate::raw(jd as i64, df, sf, self.of, self.sg, complex, self.class_id)
    }
}

impl RubyObject for RDate {
    fn class_id(&self) -> ClassId {
        self.class_id
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
        let d = RDate::raw(
            self.jd,
            self.df,
            self.sf,
            self.of,
            self.sg,
            self.complex,
            self.class_id,
        );
        if copy_frozen && self.is_frozen() {
            d.set_frozen();
        }
        d
    }
}

const DAY_NAMES: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The JDN of a proleptic-Gregorian `(year, month, day)`.
fn gregorian_civil_to_jd(y: i64, m: i64, d: i64) -> i64 {
    let a = (14 - m).div_euclid(12);
    let y2 = y + 4800 - a;
    let m2 = m + 12 * a - 3;
    d + (153 * m2 + 2) / 5 + 365 * y2 + y2.div_euclid(4) - y2.div_euclid(100) + y2.div_euclid(400)
        - 32045
}

/// The proleptic-Gregorian `(year, month, day)` of a JDN.
fn gregorian_jd_to_civil(jd: i64) -> (i64, i64, i64) {
    let a = jd + 32044;
    let b = (4 * a + 3).div_euclid(146097);
    let c = a - (146097 * b).div_euclid(4);
    let d2 = (4 * c + 3).div_euclid(1461);
    let e = c - (1461 * d2).div_euclid(4);
    let m2 = (5 * e + 2) / 153;
    let day = e - (153 * m2 + 2) / 5 + 1;
    let month = m2 + 3 - 12 * (m2 / 10);
    let year = 100 * b + d2 - 4800 + m2 / 10;
    (year, month, day)
}

/// `date_core.c:c_civil_to_jd` -- Gregorian, falling back to the Julian
/// calendar for a day before the reform `sg`.
fn civil_to_jd(y: i64, m: i64, d: i64, sg: f64) -> i64 {
    let jd = gregorian_civil_to_jd(y, m, d);
    if (jd as f64) < sg {
        let (y2, m2) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
        return (365.25 * (y2 + 4716) as f64).floor() as i64
            + (30.6001 * (m2 + 1) as f64).floor() as i64
            + d
            - 1524;
    }
    jd
}

/// `date_core.c:c_jd_to_civil`.
fn jd_to_civil(jd: i64, sg: f64) -> (i64, i64, i64) {
    if (jd as f64) >= sg {
        return gregorian_jd_to_civil(jd);
    }
    let a = jd as f64;
    let b = a + 1524.0;
    let c = ((b - 122.1) / 365.25).floor();
    let d = (365.25 * c).floor();
    let e = ((b - d) / 30.6001).floor();
    let dom = b - d - (30.6001 * e).floor();
    let (m, y) = if e <= 13.0 {
        (e - 1.0, c - 4716.0)
    } else {
        (e - 13.0, c - 4715.0)
    };
    (y as i64, m as i64, dom as i64)
}

/// The reform-aware conversions at the DEFAULT start, for the scanners: a
/// parsed string names a date on `Date::ITALY`'s calendar unless a caller
/// passes another `start`.
fn civil_to_jd_italy(y: i64, m: i64, d: i64) -> i64 {
    civil_to_jd(y, m, d, ITALY)
}

fn jd_to_civil_italy(jd: i64) -> (i64, i64, i64) {
    jd_to_civil(jd, ITALY)
}

/// `Date#wday`: 0 = Sunday .. 6 = Saturday. JDN 0 is a Monday, so `+1` aligns
/// the modulo to Sunday-origin.
fn jd_wday(jd: i64) -> i64 {
    (jd + 1).rem_euclid(7)
}

/// `date_core.c:c_find_fdoy` -- the JDN of the first day of `y` that the
/// calendar actually has (the reform swallows ten days in October 1582, but
/// never January 1st).
fn find_fdoy(y: i64, sg: f64) -> i64 {
    for d in 1..=31 {
        let jd = civil_to_jd(y, 1, d, sg);
        if jd_to_civil(jd, sg) == (y, 1, d) {
            return jd;
        }
    }
    civil_to_jd(y, 1, 1, sg)
}

/// Day-of-year (1-based).
fn jd_yday(jd: i64, sg: f64) -> i64 {
    let (y, _, _) = jd_to_civil(jd, sg);
    jd - find_fdoy(y, sg) + 1
}

/// `date_core.c:c_commercial_to_jd` -- the ISO week date `(cwyear, cweek,
/// cwday)` as a JDN.
fn commercial_to_jd(y: i64, w: i64, d: i64, sg: f64) -> i64 {
    let base = find_fdoy(y, sg) + 3;
    (base - (base).rem_euclid(7)) + 7 * (w - 1) + (d - 1)
}

/// `date_core.c:c_jd_to_commercial`.
fn jd_to_commercial(jd: i64, sg: f64) -> (i64, i64, i64) {
    let (y2, _, _) = jd_to_civil(jd - 3, sg);
    let mut cwyear = y2 + 1;
    let mut start = commercial_to_jd(y2 + 1, 1, 1, sg);
    if jd < start {
        cwyear = y2;
        start = commercial_to_jd(y2, 1, 1, sg);
    }
    let cweek = 1 + (jd - start).div_euclid(7);
    let cwday = match (jd + 1).rem_euclid(7) {
        0 => 7,
        n => n,
    };
    (cwyear, cweek, cwday)
}

/// The last day the calendar gives `(y, m)`.
fn last_mday(y: i64, m: i64, sg: f64) -> i64 {
    for d in (1..=31).rev() {
        if jd_to_civil(civil_to_jd(y, m, d, sg), sg) == (y, m, d) {
            return d;
        }
    }
    31
}

fn is_leap(y: i64) -> bool {
    y.rem_euclid(4) == 0 && (y.rem_euclid(100) != 0 || y.rem_euclid(400) == 0)
}

fn date_of(recv: &RubyValue) -> &RDate {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RDate>()
            .expect("the Date table only dispatches on Date receivers"),
        _ => unreachable!("the Date table only dispatches on Date receivers"),
    }
}

/// The other operand as a `Date`, or `None` for anything else.
fn other_date(v: &RubyValue) -> Option<&RDate> {
    match v {
        RubyValue::Object(o) => o.as_any().downcast_ref::<RDate>(),
        _ => None,
    }
}

/// A day count as an exact nanosecond offset. `Date + 1` stays whole; a Float
/// or Rational carries a day FRACTION, which is what turns a simple date
/// complex (`date_core.c:d_lite_plus`).
fn day_offset_ns(v: &RubyValue) -> Option<i128> {
    match v {
        RubyValue::Int(i) => Some(*i as i128 * NS_PER_DAY),
        RubyValue::Float(f) => Some((f * NS_PER_DAY as f64).round() as i128),
        RubyValue::Rational(r) => {
            let num: i128 = i128::try_from(&r.num).ok()?;
            let den: i128 = i128::try_from(&r.den).ok()?;
            Some(num * NS_PER_DAY / den)
        }
        RubyValue::BigInt(b) => Some(i128::try_from(&**b).ok()? * NS_PER_DAY),
        _ => None,
    }
}

/// An exact nanosecond count as the Rational number of DAYS CRuby answers.
fn ns_to_day_rational(ns: i128) -> Result<RubyValue, Signal> {
    crate::builtins::rational::rational_new(BigInt::from(ns), BigInt::from(NS_PER_DAY))
}

/// `%z` in the colon form -- `Date`'s `zone`, and its `%Z`.
fn offset_string(of: i32) -> String {
    let sign = if of < 0 { '-' } else { '+' };
    let a = of.unsigned_abs();
    format!("{sign}{:02}:{:02}", a / 3600, a % 3600 / 60)
}

/// The `Broken` instant `builtins::time`'s directive table renders. `date`
/// resolves its own civil fields (they honour the reform start), then hands
/// them to the one strftime engine rather than keeping a second copy.
fn broken(d: &RDate) -> crate::builtins::time::Broken {
    let (y, m, day) = d.civil();
    let (h, mi, s) = d.hms();
    let local_jd = d.local_jd();
    crate::builtins::time::Broken::from_date(
        y,
        m,
        day,
        h,
        mi,
        s,
        jd_wday(local_jd) as i32,
        (jd_yday(local_jd, d.sg) - 1) as i32,
        d.of,
        d.epoch(),
        d.sf as u32,
    )
}

fn strftime_date(d: &RDate, fmt: &str) -> String {
    crate::builtins::time::render_strftime(&broken(d), fmt)
}

fn str_val(d: &RDate, fmt: &str) -> Result<RubyValue, Signal> {
    Ok(RubyValue::Str(string_new(strftime_date(d, fmt))))
}

/// The format argument of `strftime`, through the conversion protocol.
fn fmt_arg(v: &RubyValue) -> Result<String, Signal> {
    Ok(crate::builtins::convert::to_rstr(v)?
        .lock()
        .to_utf8_lossy()
        .into_owned())
}

/// `date_core.c:jisx0301_date_format` -- the Japanese-era prefix, or the plain
/// ISO date for anything before the Meiji era.
fn jisx0301(d: &RDate) -> String {
    let jd = d.local_jd();
    let (y, _, _) = d.civil();
    let era = if jd < 2405160 {
        None
    } else if jd < 2419614 {
        Some(('M', 1867))
    } else if jd < 2424875 {
        Some(('T', 1911))
    } else if jd < 2447535 {
        Some(('S', 1925))
    } else if jd < 2458605 {
        Some(('H', 1988))
    } else {
        Some(('R', 2018))
    };
    match era {
        None => strftime_date(d, "%Y-%m-%d"),
        Some((c, base)) => strftime_date(d, &format!("{c}{:02}.%m.%d", y - base)),
    }
}

/// `#<Date: 2024-02-29 ((2460370j,0s,0n),+0s,2299161j)>` -- CRuby's own
/// rendering, which exposes the whole backing tuple.
fn inspect_date(recv: &RubyValue) -> Result<RubyValue, Signal> {
    let d = date_of(recv);
    let body = crate::dispatch::send_value(recv, crate::Symbol::intern("to_s"), &[], None)?;
    let body = body.to_display_string();
    let sg = if d.sg.is_infinite() {
        (if d.sg > 0.0 { "Inf" } else { "-Inf" }).to_string()
    } else {
        format!("{}", d.sg as i64)
    };
    let name = crate::dispatch::class_name(d.class_id).unwrap_or_else(|| "Date".into());
    Ok(RubyValue::Str(string_new(format!(
        "#<{name}: {body} (({}j,{}s,{}n),{:+}s,{sg}j)>",
        d.jd, d.df, d.sf, d.of
    ))))
}

/// `Date.new`'s civil components. NOT the generic implicit-conversion
/// protocol: the date library takes any numeric (a Float truncates) and
/// rejects the rest with its own "invalid year (not numeric)" TypeError shape
/// (oracle-verified; even `to_int` ducks are rejected).
fn numeric_at(args: &[RubyValue], i: usize, default: i64, name: &str) -> Result<i64, Signal> {
    match args.get(i) {
        None | Some(RubyValue::Nil) if i > 0 => Ok(default),
        None => Ok(default),
        Some(RubyValue::Int(v)) => Ok(*v),
        Some(RubyValue::Float(f)) => Ok(f.trunc() as i64),
        Some(RubyValue::Rational(r)) => {
            let n = i64::try_from(&r.num).unwrap_or(0);
            let d = i64::try_from(&r.den).unwrap_or(1);
            Ok(n / d.max(1))
        }
        Some(_) => Err(type_error!("invalid {name} (not numeric)")),
    }
}

/// The `start` argument every constructor accepts last: a Float JDN, with
/// `Date::JULIAN`/`Date::GREGORIAN` as the infinities.
fn start_arg(args: &[RubyValue], i: usize) -> Result<f64, Signal> {
    match args.get(i) {
        None | Some(RubyValue::Nil) => Ok(ITALY),
        Some(RubyValue::Int(v)) => Ok(*v as f64),
        Some(RubyValue::Float(f)) => Ok(*f),
        Some(other) => Err(type_error!(
            "no implicit conversion to float from {}",
            crate::dispatch::class_name(other.class_id())
                .unwrap_or_default()
                .to_lowercase()
        )),
    }
}

/// A civil `(y, m, d)` with CRuby's negative-index rule (`-1` is the last
/// month / the last day of the month), validated against the calendar.
fn civil_checked(y: i64, m: i64, d: i64, sg: f64) -> Result<i64, Signal> {
    let m = if m < 0 { 13 + m } else { m };
    if !(1..=12).contains(&m) {
        return Err(date_error("invalid date"));
    }
    let d = if d < 0 {
        last_mday(y, m, sg) + 1 + d
    } else {
        d
    };
    let jd = civil_to_jd(y, m, d, sg);
    if jd_to_civil(jd, sg) != (y, m, d) {
        return Err(date_error("invalid date"));
    }
    Ok(jd)
}

/// `Date::Error` -- a subclass of `ArgumentError` the `date` gem defines. zeo
/// has no feature-gated exception class yet, so this raises the parent with
/// CRuby's message; a `rescue ArgumentError` (and `rescue => e`) catches it
/// identically, only `rescue Date::Error` does not exist.
fn date_error(msg: &str) -> Signal {
    arg_error!("{msg}")
}

/// The current UTC calendar day, as a JDN (a documented divergence from CRuby's
/// LOCAL date near midnight in a non-UTC zone); untestable deterministically.
fn today_jd() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    EPOCH_JD + secs.div_euclid(SECS_PER_DAY)
}

/// The class a `Date`-family constructor allocates through: the RECEIVER's own
/// id, which is CRuby's rule (`rb_class_new_instance`-style, `klass` threaded
/// into the allocation) and the whole reason a `class DateTimeWithOffset <
/// DateTime` works -- `DateTimeWithOffset.jd(n)` has to answer a
/// `DateTimeWithOffset`, not a `Date`. Naming only `Date`/`DateTime` here
/// silently demoted every subclass to `Date`.
fn class_of(recv: &RubyValue) -> ClassId {
    match recv {
        RubyValue::Class(cid) => *cid,
        _ => DATE_CLASS,
    }
}

/// Does this class want the complex representation? `DateTime` and its
/// subclasses do; `Date` does not.
fn wants_complex(cid: ClassId) -> bool {
    cid == DATETIME_CLASS || crate::dispatch::ancestors_contain(cid, DATETIME_CLASS)
}

/// `Date#>>` -- shift by `n` months, clamping the day down to one the target
/// month has (`date_core.c:d_lite_rshift`).
fn month_shift(d: &RDate, n: i64) -> Arc<RDate> {
    let (y, m, mday) = d.civil();
    let t = y * 12 + (m - 1) + n;
    let (ny, nm) = (t.div_euclid(12), t.rem_euclid(12) + 1);
    let mut day = mday;
    let jd = loop {
        let jd = civil_to_jd(ny, nm, day, d.sg);
        if jd_to_civil(jd, d.sg) == (ny, nm, day) {
            break jd;
        }
        day -= 1;
    };
    // Keep the time of day: `>>` is `+ (target_jd - local_jd)` in CRuby.
    d.at_ns(d.total_ns() + (jd - d.local_jd()) as i128 * NS_PER_DAY)
}

/// `step`/`upto`/`downto`, all one walk (`date_core.c:d_lite_step`).
fn walk(
    recv: &RubyValue,
    limit: &RubyValue,
    step: i64,
    block: &crate::rproc::RProc,
) -> Result<RubyValue, Signal> {
    let d = date_of(recv);
    let Some(limit) = other_date(limit) else {
        return Err(type_error!("expected numeric or date"));
    };
    if step == 0 {
        return Err(arg_error!("step can't be 0"));
    }
    let (limit_ns, mut cur) = (limit.total_ns(), d.total_ns());
    let delta = step as i128 * NS_PER_DAY;
    while (step > 0 && cur <= limit_ns) || (step < 0 && cur >= limit_ns) {
        let v = RubyValue::Object(d.at_ns(cur));
        match block.call(std::slice::from_ref(&v)) {
            Err(Signal::Break(bv)) => return Ok(bv),
            other => other?,
        };
        cur += delta;
    }
    Ok(recv.clone())
}

/// The `(jd, df, sf, of)` a parsed fragment hash denotes -- the shared tail of
/// `Date.parse`/`Date.strptime` and their `DateTime` twins.
fn from_fragments(
    pairs: &[(RubyValue, RubyValue)],
    y: i64,
    m: i64,
    d: i64,
    sg: f64,
) -> (i64, i32, i32, i32) {
    let get = |key: &str| -> Option<i64> {
        pairs.iter().find_map(|(k, v)| match (k, v) {
            (RubyValue::Symbol(s), RubyValue::Int(n)) if s.name_str() == key => Some(*n),
            _ => None,
        })
    };
    let of = get("offset").unwrap_or(0) as i32;
    let local_secs = (get("hour").unwrap_or(0) * 3600
        + get("min").unwrap_or(0) * 60
        + get("sec").unwrap_or(0)) as i32;
    let sf = pairs
        .iter()
        .find_map(|(k, v)| match (k, v) {
            (RubyValue::Symbol(s), RubyValue::Rational(r)) if s.name_str() == "sec_fraction" => {
                let n = i128::try_from(&r.num).ok()?;
                let den = i128::try_from(&r.den).ok()?;
                Some((n * NS_PER_SEC / den) as i32)
            }
            _ => None,
        })
        .unwrap_or(0);
    // Stored as UTC: subtract the offset the string carried.
    let utc = local_secs as i64 - of as i64;
    let jd = civil_to_jd(y, m, d, sg) + utc.div_euclid(SECS_PER_DAY);
    (jd, utc.rem_euclid(SECS_PER_DAY) as i32, sf, of)
}

/// The calendar date `Date.parse` builds out of [`parse::date_parse`]'s fields.
///
/// A component ABOVE the most significant one the string gave comes from today,
/// and one below it takes its minimum -- CRuby's completion rule, and the reason
/// `Date.parse("Aug 31")` lands in the current year while `Date.parse("Aug
/// 2000")` lands on the first of the month. A string with no date in it at all,
/// or one whose fields name no real day, is an ArgumentError.
fn parse_date(pairs: &[(RubyValue, RubyValue)], sg: f64) -> Result<(i64, i64, i64), Signal> {
    let field = |key: &str| parse::field(pairs, key);
    // The ISO WEEK date names a different calendar, and an ordinal date counts
    // days rather than naming a month; both convert to a JDN directly.
    if let (Some(cwyear), Some(cweek)) = (field("cwyear"), field("cweek")) {
        return Ok(jd_to_civil(
            commercial_to_jd(cwyear, cweek, field("cwday").unwrap_or(1), sg),
            sg,
        ));
    }
    if let (Some(y), Some(yday)) = (field("year"), field("yday")) {
        return Ok(jd_to_civil(find_fdoy(y, sg) + yday - 1, sg));
    }
    let (this_year, this_month, _) = jd_to_civil(today_jd(), sg);
    let (y, m, d) = match (field("year"), field("mon"), field("mday")) {
        (Some(y), mon, mday) => (y, mon.unwrap_or(1), mday.unwrap_or(1)),
        (None, Some(mon), mday) => (this_year, mon, mday.unwrap_or(1)),
        (None, None, Some(mday)) => (this_year, this_month, mday),
        (None, None, None) => return Err(date_error("invalid date")),
    };
    if jd_to_civil(civil_to_jd(y, m, d, sg), sg) != (y, m, d) {
        return Err(date_error("invalid date"));
    }
    Ok((y, m, d))
}

/// `Date.parse`/`DateTime.parse`: scan, complete, and build in the receiver's
/// class (a `DateTime` keeps the clock, a `Date` drops it).
fn build_parsed(recv: &RubyValue, text: &str, sg: f64) -> Result<RubyValue, Signal> {
    let pairs = parse::date_parse(text, true);
    let (y, m, d) = parse_date(&pairs, sg)?;
    Ok(build_from(recv, &pairs, y, m, d, sg))
}

fn build_from(
    recv: &RubyValue,
    pairs: &[(RubyValue, RubyValue)],
    y: i64,
    m: i64,
    d: i64,
    sg: f64,
) -> RubyValue {
    let cid = class_of(recv);
    if wants_complex(cid) {
        let (jd, df, sf, of) = from_fragments(pairs, y, m, d, sg);
        RubyValue::Object(RDate::raw(jd, df, sf, of, sg, true, cid))
    } else {
        RubyValue::Object(RDate::simple(civil_to_jd(y, m, d, sg), sg, cid))
    }
}

/// `Date.strptime`/`DateTime.strptime`.
fn build_strptime(recv: &RubyValue, text: &str, fmt: &str, sg: f64) -> Result<RubyValue, Signal> {
    let pairs = strptime::date_strptime(text, fmt).ok_or_else(|| date_error("invalid date"))?;
    let (y, m, d) = strptime::civil_from_fragments(&pairs, jd_to_civil(today_jd(), sg).0)
        .ok_or_else(|| date_error("invalid date"))?;
    Ok(build_from(recv, &pairs, y, m, d, sg))
}

ruby_class! {
    Date = zeo_abi::DATE_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::COMPARABLE_CLASS;

    const ITALY = RubyValue::Int(2299161);
    const ENGLAND = RubyValue::Int(2361222);
    const JULIAN = RubyValue::Float(f64::INFINITY);
    const GREGORIAN = RubyValue::Float(f64::NEG_INFINITY);
    const VERSION = RubyValue::Str(string_new("3.5.1".into()));
    const DAYNAMES = RubyValue::Array(crate::array_new(
        DAY_NAMES.iter().map(|n| RubyValue::Str(string_new((*n).into()))).collect()));
    const ABBR_DAYNAMES = RubyValue::Array(crate::array_new(
        DAY_NAMES.iter().map(|n| RubyValue::Str(string_new(n[..3].into()))).collect()));
    const MONTHNAMES = RubyValue::Array(crate::array_new(
        std::iter::once(RubyValue::Nil)
            .chain(MONTH_NAMES.iter().map(|n| RubyValue::Str(string_new((*n).into()))))
            .collect()));
    const ABBR_MONTHNAMES = RubyValue::Array(crate::array_new(
        std::iter::once(RubyValue::Nil)
            .chain(MONTH_NAMES.iter().map(|n| RubyValue::Str(string_new(n[..3].into()))))
            .collect()));

    def "year" (recv) { Ok(RubyValue::Int(date_of(recv).civil().0)) }
    def "month" | "mon" (recv) { Ok(RubyValue::Int(date_of(recv).civil().1)) }
    def "day" | "mday" (recv) { Ok(RubyValue::Int(date_of(recv).civil().2)) }
    def "wday" (recv) { Ok(RubyValue::Int(jd_wday(date_of(recv).local_jd()))) }
    def "yday" (recv) {
        let d = date_of(recv);
        Ok(RubyValue::Int(jd_yday(d.local_jd(), d.sg)))
    }
    def "jd" (recv) { Ok(RubyValue::Int(date_of(recv).local_jd())) }
    def "mjd" (recv) { Ok(RubyValue::Int(date_of(recv).local_jd() - 2_400_001)) }
    def "ld" (recv) { Ok(RubyValue::Int(date_of(recv).local_jd() - 2_299_160)) }
    // The ASTRONOMICAL julian day: days since noon, so a midnight date is a
    // half-day behind its JDN.
    def "ajd" (recv) {
        ns_to_day_rational(date_of(recv).total_ns() - NS_PER_DAY / 2)
    }
    def "amjd" (recv) {
        ns_to_day_rational(date_of(recv).total_ns() - 2_400_001i128 * NS_PER_DAY)
    }
    def "day_fraction" (recv) {
        let d = date_of(recv);
        if !d.complex {
            return Ok(RubyValue::Int(0));
        }
        let (_, secs) = d.local();
        ns_to_day_rational(secs as i128 * NS_PER_SEC + d.sf as i128)
    }
    def "cwday" (recv) {
        let d = date_of(recv);
        Ok(RubyValue::Int(jd_to_commercial(d.local_jd(), d.sg).2))
    }
    def "cweek" (recv) {
        let d = date_of(recv);
        Ok(RubyValue::Int(jd_to_commercial(d.local_jd(), d.sg).1))
    }
    def "cwyear" (recv) {
        let d = date_of(recv);
        Ok(RubyValue::Int(jd_to_commercial(d.local_jd(), d.sg).0))
    }
    def "leap?" (recv) { Ok(RubyValue::Bool(is_leap(date_of(recv).civil().0))) }
    def "infinite?" (_recv) { Ok(RubyValue::Bool(false)) }

    def "julian?" (recv) {
        let d = date_of(recv);
        Ok(RubyValue::Bool((d.local_jd() as f64) < d.sg))
    }
    def "gregorian?" (recv) {
        let d = date_of(recv);
        Ok(RubyValue::Bool((d.local_jd() as f64) >= d.sg))
    }
    def "start" (recv) { Ok(RubyValue::Float(date_of(recv).sg)) }
    // `new_start` keeps the INSTANT and re-renders it: the jd is unchanged,
    // only the calendar the civil fields come from moves.
    def "new_start" (recv, arg?) {
        let d = date_of(recv);
        let sg = start_arg(std::slice::from_ref(&arg.cloned().unwrap_or(RubyValue::Nil)), 0)?;
        Ok(RubyValue::Object(RDate::raw(d.jd, d.df, d.sf, d.of, sg, d.complex, d.class_id)))
    }
    def "julian" (recv) {
        let d = date_of(recv);
        Ok(RubyValue::Object(RDate::raw(d.jd, d.df, d.sf, d.of, f64::INFINITY, d.complex, d.class_id)))
    }
    def "gregorian" (recv) {
        let d = date_of(recv);
        Ok(RubyValue::Object(RDate::raw(d.jd, d.df, d.sf, d.of, f64::NEG_INFINITY, d.complex, d.class_id)))
    }
    def "england" (recv) {
        let d = date_of(recv);
        Ok(RubyValue::Object(RDate::raw(d.jd, d.df, d.sf, d.of, ENGLAND, d.complex, d.class_id)))
    }
    def "italy" (recv) {
        let d = date_of(recv);
        Ok(RubyValue::Object(RDate::raw(d.jd, d.df, d.sf, d.of, ITALY, d.complex, d.class_id)))
    }

    def "to_s" | "iso8601" | "xmlschema" (recv) { str_val(date_of(recv), "%Y-%m-%d") }
    def "inspect" (recv) { inspect_date(recv) }
    def "strftime" cfunc (recv, arg?) {
        let fmt = match arg { Some(v) => fmt_arg(v)?, None => "%Y-%m-%d".to_string() };
        str_val(date_of(recv), &fmt)
    }
    def "ctime" | "asctime" (recv) { str_val(date_of(recv), "%a %b %e %H:%M:%S %Y") }
    def "rfc3339" (recv) { str_val(date_of(recv), "%Y-%m-%dT%H:%M:%S%:z") }
    def "rfc2822" | "rfc822" (recv) { str_val(date_of(recv), "%a, %-d %b %Y %T %z") }
    // `httpdate` is always GMT: CRuby re-offsets a copy to zero first.
    def "httpdate" (recv) {
        let d = date_of(recv);
        let utc = RDate::raw(d.jd, d.df, d.sf, 0, d.sg, d.complex, d.class_id);
        str_val(&utc, "%a, %d %b %Y %T GMT")
    }
    def "jisx0301" (recv) { Ok(RubyValue::Str(string_new(jisx0301(date_of(recv))))) }

    def "+" (recv, other) {
        let d = date_of(recv);
        let ns = day_offset_ns(other).ok_or_else(|| type_error!("expected numeric"))?;
        Ok(RubyValue::Object(d.at_ns(d.total_ns() + ns)))
    }
    // `date - n` -> a Date n days earlier; `date - other_date` -> the Rational
    // day difference (CRuby's result type).
    def "-" (recv, other) {
        let d = date_of(recv);
        if let Some(o) = other_date(other) {
            return ns_to_day_rational(d.total_ns() - o.total_ns());
        }
        let ns = day_offset_ns(other).ok_or_else(|| type_error!("expected numeric"))?;
        Ok(RubyValue::Object(d.at_ns(d.total_ns() - ns)))
    }
    // `>>`/`<<` shift by MONTHS. CRuby computes `year*12 + mon-1 + n` with
    // ordinary Integer arithmetic, so a non-numeric argument fails with the
    // COERCION message rather than date`s own "expected numeric".
    def ">>" (recv, other) {
        Ok(RubyValue::Object(month_shift(date_of(recv), month_count(other)?)))
    }
    def "<<" (recv, other) {
        Ok(RubyValue::Object(month_shift(date_of(recv), -month_count(other)?)))
    }
    def "next_day" (recv, arg?) {
        let d = date_of(recv);
        let n = arg.map_or(Ok(1), day_int)?;
        Ok(RubyValue::Object(d.at_ns(d.total_ns() + n as i128 * NS_PER_DAY)))
    }
    def "prev_day" (recv, arg?) {
        let d = date_of(recv);
        let n = arg.map_or(Ok(1), day_int)?;
        Ok(RubyValue::Object(d.at_ns(d.total_ns() - n as i128 * NS_PER_DAY)))
    }
    def "next_month" (recv, arg?) {
        let n = arg.map_or(Ok(1), day_int)?;
        Ok(RubyValue::Object(month_shift(date_of(recv), n)))
    }
    def "prev_month" (recv, arg?) {
        let n = arg.map_or(Ok(1), day_int)?;
        Ok(RubyValue::Object(month_shift(date_of(recv), -n)))
    }
    def "next_year" (recv, arg?) {
        let n = arg.map_or(Ok(1), day_int)?;
        Ok(RubyValue::Object(month_shift(date_of(recv), n * 12)))
    }
    def "prev_year" (recv, arg?) {
        let n = arg.map_or(Ok(1), day_int)?;
        Ok(RubyValue::Object(month_shift(date_of(recv), -n * 12)))
    }
    def "next" | "succ" (recv) {
        let d = date_of(recv);
        Ok(RubyValue::Object(d.at_ns(d.total_ns() + NS_PER_DAY)))
    }

    // `*args` because CRuby declares `step` with a rest list (arity -1), not
    // `(limit, step = 1)` -- `conformance/builtin-arity.tsv` gates the shape.
    def "step" (recv, *args, &block) {
        let Some(limit) = args.first() else {
            return Err(crate::dispatch::wrong_arity(0, "1..2"));
        };
        let n = match args.get(1) { Some(v) => day_int(v)?, None => 1 };
        let blk = crate::builtins::block_or_enum!(recv, args, block);
        walk(recv, limit, n, &blk)
    }
    def "upto" (recv, limit, &block) {
        let args = [limit.clone()];
        let blk = crate::builtins::block_or_enum!(recv, &args, block);
        walk(recv, limit, 1, &blk)
    }
    def "downto" (recv, limit, &block) {
        let args = [limit.clone()];
        let blk = crate::builtins::block_or_enum!(recv, &args, block);
        walk(recv, limit, -1, &blk)
    }

    def "<=>" (recv, other) {
        let a = date_of(recv).total_ns();
        let b = match other_date(other) {
            Some(o) => o.total_ns(),
            None => match other {
                // A bare number compares against the ASTRONOMICAL julian day.
                RubyValue::Int(n) => *n as i128 * NS_PER_DAY + NS_PER_DAY / 2,
                _ => return Ok(RubyValue::Nil),
            },
        };
        Ok(RubyValue::Int((a.cmp(&b) as i64).signum()))
    }
    def "===" (recv, other) {
        let a = date_of(recv).local_jd();
        Ok(RubyValue::Bool(match other_date(other) {
            Some(o) => o.local_jd() == a,
            None => matches!(other, RubyValue::Int(n) if *n == a),
        }))
    }
    def "eql?" (recv, other) {
        let a = date_of(recv).total_ns();
        Ok(RubyValue::Bool(other_date(other).is_some_and(|o| o.total_ns() == a)))
    }
    def "hash" (recv) {
        let ns = date_of(recv).total_ns();
        Ok(RubyValue::Int((ns as i64).wrapping_mul(0x9E37_79B9_7F4A_7C15u64 as i64)))
    }

    def "to_date" (recv) {
        let d = date_of(recv);
        Ok(RubyValue::Object(RDate::simple(d.local_jd(), d.sg, DATE_CLASS)))
    }
    // CRuby copies the SIMPLE half straight through, so a plain Date answers a
    // simple DateTime -- one whose `day_fraction` is the Integer 0.
    def "to_datetime" (recv) {
        let d = date_of(recv);
        Ok(RubyValue::Object(if d.complex {
            RDate::raw(d.jd, d.df, d.sf, d.of, d.sg, true, DATETIME_CLASS)
        } else {
            RDate::simple(d.local_jd(), d.sg, DATETIME_CLASS)
        }))
    }
    def "to_time" (recv) {
        let d = date_of(recv);
        let (y, m, day) = d.civil();
        crate::dispatch::send_value(
            &RubyValue::Class(zeo_abi::TIME_CLASS),
            crate::Symbol::intern("local"),
            &[RubyValue::Int(y), RubyValue::Int(m), RubyValue::Int(day)],
            None,
        )
    }
    def "deconstruct_keys" (recv, _keys) {
        let d = date_of(recv);
        let (y, m, day) = d.civil();
        let local_jd = d.local_jd();
        Ok(RubyValue::Hash(crate::collections::hash_new(vec![
            (RubyValue::Symbol(crate::Symbol::intern("year")), RubyValue::Int(y)),
            (RubyValue::Symbol(crate::Symbol::intern("month")), RubyValue::Int(m)),
            (RubyValue::Symbol(crate::Symbol::intern("day")), RubyValue::Int(day)),
            (RubyValue::Symbol(crate::Symbol::intern("yday")), RubyValue::Int(jd_yday(local_jd, d.sg))),
            (RubyValue::Symbol(crate::Symbol::intern("wday")), RubyValue::Int(jd_wday(local_jd))),
        ])))
    }
    def "marshal_dump" (recv) {
        let d = date_of(recv);
        Ok(RubyValue::Array(crate::array_new(vec![
            RubyValue::Int(0),
            RubyValue::Int(d.jd),
            RubyValue::Int(d.df as i64),
            RubyValue::Int(d.sf as i64),
            RubyValue::Int(d.of as i64),
            RubyValue::Float(d.sg),
        ])))
    }

    def "sunday?" (recv) { Ok(RubyValue::Bool(jd_wday(date_of(recv).local_jd()) == 0)) }
    def "monday?" (recv) { Ok(RubyValue::Bool(jd_wday(date_of(recv).local_jd()) == 1)) }
    def "tuesday?" (recv) { Ok(RubyValue::Bool(jd_wday(date_of(recv).local_jd()) == 2)) }
    def "wednesday?" (recv) { Ok(RubyValue::Bool(jd_wday(date_of(recv).local_jd()) == 3)) }
    def "thursday?" (recv) { Ok(RubyValue::Bool(jd_wday(date_of(recv).local_jd()) == 4)) }
    def "friday?" (recv) { Ok(RubyValue::Bool(jd_wday(date_of(recv).local_jd()) == 5)) }
    def "saturday?" (recv) { Ok(RubyValue::Bool(jd_wday(date_of(recv).local_jd()) == 6)) }

    // The clock accessors exist on `Date` too, but PRIVATE: only `DateTime`
    // publishes them (oracle: `Date.new.hour` is a NoMethodError naming a
    // private method).
    private def "hour" (recv) { Ok(RubyValue::Int(date_of(recv).hms().0 as i64)) }
    private def "min" | "minute" (recv) { Ok(RubyValue::Int(date_of(recv).hms().1 as i64)) }
    private def "sec" | "second" (recv) { Ok(RubyValue::Int(date_of(recv).hms().2 as i64)) }

    def self."new" | "civil" (recv, _year?, _month?, _mday?, _start?) {
        let sg = start_arg(__args, 3)?;
        let y = numeric_at(__args, 0, -4712, "year")?;
        let m = numeric_at(__args, 1, 1, "month")?;
        let d = numeric_at(__args, 2, 1, "day")?;
        let jd = civil_checked(y, m, d, sg)?;
        Ok(RubyValue::Object(RDate::simple(jd, sg, class_of(recv))))
    }
    def self."jd" (recv, _jd?, _start?) {
        let sg = start_arg(__args, 1)?;
        let jd = numeric_at(__args, 0, 0, "jd")?;
        Ok(RubyValue::Object(RDate::simple(jd, sg, class_of(recv))))
    }
    def self."ordinal" (recv, _year?, _yday?, _start?) {
        let sg = start_arg(__args, 2)?;
        let y = numeric_at(__args, 0, -4712, "year")?;
        let yd = numeric_at(__args, 1, 1, "yday")?;
        let jd = ordinal_jd(y, yd, sg).ok_or_else(|| date_error("invalid date"))?;
        Ok(RubyValue::Object(RDate::simple(jd, sg, class_of(recv))))
    }
    def self."commercial" (recv, _cwyear?, _cweek?, _cwday?, _start?) {
        let sg = start_arg(__args, 3)?;
        let y = numeric_at(__args, 0, -4712, "cwyear")?;
        let w = numeric_at(__args, 1, 1, "cweek")?;
        let d = numeric_at(__args, 2, 1, "cwday")?;
        let jd = commercial_jd(y, w, d, sg).ok_or_else(|| date_error("invalid date"))?;
        Ok(RubyValue::Object(RDate::simple(jd, sg, class_of(recv))))
    }
    def self."today" cfunc (recv, _start?) {
        let sg = start_arg(__args, 0)?;
        Ok(RubyValue::Object(RDate::simple(today_jd(), sg, class_of(recv))))
    }
    def self."parse" cfunc (recv, arg1?, _comp?, _start?) {
        let sg = start_arg(__args, 2)?;
        let text = match arg1 { Some(v) => fmt_arg(v)?, None => "-4712-01-01".to_string() };
        build_parsed(recv, &text, sg)
    }
    def self."iso8601" | "xmlschema" | "rfc3339" | "httpdate" | "jisx0301" | "rfc2822" | "rfc822"
        cfunc (recv, arg1?, _start?)
    {
        let sg = start_arg(__args, 1)?;
        let text = match arg1 { Some(v) => fmt_arg(v)?, None => "-4712-01-01".to_string() };
        build_parsed(recv, &text, sg)
    }
    // The heuristic scanner itself, which `Date.parse` and the `time` gem's
    // `Time.parse` both read their fields out of. See `parse.rs`.
    def self."_parse" cfunc (_recv, arg1, arg2?) {
        let text = fmt_arg(arg1)?;
        let comp = arg2.is_none_or(RubyValue::truthy);
        Ok(RubyValue::Hash(crate::collections::hash_new(parse::date_parse(&text, comp))))
    }
    // The format-DIRECTED scanner, which `Date.strptime` and the `time`
    // gem's `Time.strptime` read their fields out of. See `strptime.rs`.
    def self."_strptime" cfunc (_recv, arg1, arg2?) {
        let text = fmt_arg(arg1)?;
        let fmt = match arg2 { Some(v) => fmt_arg(v)?, None => "%F".to_string() };
        Ok(match strptime::date_strptime(&text, &fmt) {
            Some(pairs) => RubyValue::Hash(crate::collections::hash_new(pairs)),
            None => RubyValue::Nil,
        })
    }
    def self."strptime" cfunc (recv, arg1?, arg2?, _start?) {
        let sg = start_arg(__args, 2)?;
        let text = match arg1 { Some(v) => fmt_arg(v)?, None => "-4712-01-01".to_string() };
        let fmt = match arg2 { Some(v) => fmt_arg(v)?, None => "%F".to_string() };
        build_strptime(recv, &text, &fmt, sg)
    }
    def self."valid_date?" | "valid_civil?" cfunc (_recv, _year, _month, _mday, _start?) {
        // A non-numeric component answers false rather than raising
        // (oracle: `Date.valid_date?(2020, nil, 1)` is false).
        let sg = start_arg(__args, 3).unwrap_or(ITALY);
        let Ok(y) = numeric_at(__args, 0, -4712, "year") else { return Ok(RubyValue::Bool(false)) };
        let Ok(m) = numeric_at(__args, 1, 1, "month") else { return Ok(RubyValue::Bool(false)) };
        let Ok(d) = numeric_at(__args, 2, 1, "day") else { return Ok(RubyValue::Bool(false)) };
        Ok(RubyValue::Bool(civil_checked(y, m, d, sg).is_ok()))
    }
    def self."valid_ordinal?" cfunc (_recv, _year, _yday, _start?) {
        let sg = start_arg(__args, 2).unwrap_or(ITALY);
        let Ok(y) = numeric_at(__args, 0, -4712, "year") else { return Ok(RubyValue::Bool(false)) };
        let Ok(d) = numeric_at(__args, 1, 1, "yday") else { return Ok(RubyValue::Bool(false)) };
        Ok(RubyValue::Bool(ordinal_jd(y, d, sg).is_some()))
    }
    def self."valid_commercial?" cfunc (_recv, _cwyear, _cweek, _cwday, _start?) {
        let sg = start_arg(__args, 3).unwrap_or(ITALY);
        let Ok(y) = numeric_at(__args, 0, -4712, "cwyear") else { return Ok(RubyValue::Bool(false)) };
        let Ok(w) = numeric_at(__args, 1, 1, "cweek") else { return Ok(RubyValue::Bool(false)) };
        let Ok(d) = numeric_at(__args, 2, 1, "cwday") else { return Ok(RubyValue::Bool(false)) };
        Ok(RubyValue::Bool(commercial_jd(y, w, d, sg).is_some()))
    }
    def self."valid_jd?" cfunc (_recv, _jd, _start?) {
        Ok(RubyValue::Bool(numeric_at(__args, 0, 0, "jd").is_ok()))
    }
    def self."leap?" | "gregorian_leap?" (_recv, arg) {
        let RubyValue::Int(y) = crate::builtins::convert::to_int(arg)? else {
            return Err(type_error!("invalid year (not numeric)"));
        };
        Ok(RubyValue::Bool(is_leap(y)))
    }
    def self."julian_leap?" (_recv, arg) {
        let RubyValue::Int(y) = crate::builtins::convert::to_int(arg)? else {
            return Err(type_error!("invalid year (not numeric)"));
        };
        Ok(RubyValue::Bool(y.rem_euclid(4) == 0))
    }
}

/// `>>`/`<<`'s month count: plain Integer coercion, message and all.
fn month_count(v: &RubyValue) -> Result<i64, Signal> {
    match v {
        RubyValue::Int(i) => Ok(*i),
        RubyValue::Float(f) => Ok(f.trunc() as i64),
        other => Err(type_error!(
            "{} can't be coerced into Integer",
            crate::dispatch::class_name(other.class_id()).unwrap_or_default()
        )),
    }
}

/// A whole-day count argument (`next_day(3)`, `step`'s stride).
fn day_int(v: &RubyValue) -> Result<i64, Signal> {
    match v {
        RubyValue::Int(i) => Ok(*i),
        RubyValue::Float(f) => Ok(f.trunc() as i64),
        other => match crate::builtins::convert::to_int(other)? {
            RubyValue::Int(i) => Ok(i),
            _ => Err(type_error!("expected numeric")),
        },
    }
}

/// `Date.ordinal` -- day `yday` of `y`, with CRuby's negative-index rule.
fn ordinal_jd(y: i64, yday: i64, sg: f64) -> Option<i64> {
    let first = find_fdoy(y, sg);
    let len = find_fdoy(y + 1, sg) - first;
    let yday = if yday < 0 { len + 1 + yday } else { yday };
    if yday < 1 || yday > len {
        return None;
    }
    Some(first + yday - 1)
}

/// `Date.commercial` -- an ISO week date, with CRuby's negative-index rule and
/// its round-trip validity check.
fn commercial_jd(y: i64, w: i64, d: i64, sg: f64) -> Option<i64> {
    let d = if d < 0 { 8 + d } else { d };
    if !(1..=7).contains(&d) {
        return None;
    }
    let w = if w < 0 {
        let last = jd_to_commercial(commercial_to_jd(y + 1, 1, 1, sg) - 7, sg).1;
        last + 1 + w
    } else {
        w
    };
    let jd = commercial_to_jd(y, w, d, sg);
    if jd_to_commercial(jd, sg) != (y, w, d) {
        return None;
    }
    Some(jd)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i64, m: i64, d: i64) -> RubyValue {
        RubyValue::Object(RDate::simple(
            civil_to_jd(y, m, d, ITALY),
            ITALY,
            DATE_CLASS,
        ))
    }
    /// `Date`'s `ruby_class!`-generated methods have mangled Rust idents, so the
    /// tests reach them through the registered instance table.
    fn im(name: &str) -> crate::builtins::BuiltinMethodFn {
        let t = crate::builtins::registered_table(DATE_CLASS)
            .expect("Date is a registered builtin table")
            .instance
            .as_ref()
            .expect("Date has instance methods");
        (t.lookup)(name).unwrap_or_else(|| panic!("Date#{name} is defined"))
    }
    fn int(v: &RubyValue) -> i64 {
        match v {
            RubyValue::Int(i) => *i,
            other => panic!("expected Int, got {other:?}"),
        }
    }
    fn s(v: &RubyValue) -> String {
        match v {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => panic!("expected Str, got {other:?}"),
        }
    }
    fn call(name: &str, recv: &RubyValue, args: &[RubyValue]) -> RubyValue {
        im(name)(recv, args, None).unwrap()
    }

    #[test]
    fn civil_jd_round_trips_on_both_calendars() {
        for (y, m, d) in [(2000, 1, 1), (1970, 1, 1), (2024, 2, 29), (1, 1, 1)] {
            let jd = civil_to_jd(y, m, d, ITALY);
            assert_eq!(jd_to_civil(jd, ITALY), (y, m, d));
        }
        // The reform: 1582-10-04 (Julian) is followed directly by 1582-10-15.
        assert_eq!(civil_to_jd(1582, 10, 4, ITALY), 2299160);
        assert_eq!(civil_to_jd(1582, 10, 15, ITALY), 2299161);
        // A Julian date, so the proleptic-Gregorian answer (1721426) is wrong.
        assert_eq!(civil_to_jd(1, 1, 1, ITALY), 1721424);
    }

    #[test]
    fn fields_match_ruby() {
        let dt = date(2024, 3, 15); // a Friday
        assert_eq!(int(&call("year", &dt, &[])), 2024);
        assert_eq!(int(&call("month", &dt, &[])), 3);
        assert_eq!(int(&call("day", &dt, &[])), 15);
        assert_eq!(int(&call("wday", &dt, &[])), 5);
        assert_eq!(s(&call("to_s", &dt, &[])), "2024-03-15");
    }

    #[test]
    fn julian_day_family_matches_ruby() {
        let dt = date(2024, 2, 29);
        assert_eq!(int(&call("jd", &dt, &[])), 2460370);
        assert_eq!(int(&call("mjd", &dt, &[])), 60369);
        assert_eq!(int(&call("ld", &dt, &[])), 161210);
        // A simple date's day fraction is the INTEGER zero, not `(0/1)`.
        assert_eq!(int(&call("day_fraction", &dt, &[])), 0);
        match call("ajd", &dt, &[]) {
            RubyValue::Rational(r) => {
                assert_eq!(
                    (r.num.to_string(), r.den.to_string()),
                    ("4920739".into(), "2".into())
                );
            }
            other => panic!("expected Rational, got {other:?}"),
        }
    }

    #[test]
    fn commercial_week_matches_ruby() {
        assert_eq!(
            jd_to_commercial(civil_to_jd(2024, 2, 29, ITALY), ITALY),
            (2024, 9, 4)
        );
        // 2021-01-01 is a Friday, so it belongs to week 53 of 2020.
        assert_eq!(
            jd_to_commercial(civil_to_jd(2021, 1, 1, ITALY), ITALY).0,
            2020
        );
    }

    #[test]
    fn month_shift_clamps_the_day() {
        let jan31 = date(2024, 1, 31);
        assert_eq!(
            s(&call(
                "to_s",
                &call(">>", &jan31, &[RubyValue::Int(1)]),
                &[]
            )),
            "2024-02-29"
        );
        assert_eq!(
            s(&call(
                "to_s",
                &call("<<", &jan31, &[RubyValue::Int(1)]),
                &[]
            )),
            "2023-12-31"
        );
        assert_eq!(
            s(&call(
                "to_s",
                &call(">>", &jan31, &[RubyValue::Int(12)]),
                &[]
            )),
            "2025-01-31"
        );
    }

    #[test]
    fn arithmetic_advances_and_differences() {
        let a = date(2024, 1, 1);
        let plus10 = call("+", &a, &[RubyValue::Int(10)]);
        assert_eq!(s(&call("to_s", &plus10, &[])), "2024-01-11");
        // date - date -> Rational difference.
        assert!(matches!(
            call("-", &plus10, std::slice::from_ref(&a)),
            RubyValue::Rational(_)
        ));
    }

    #[test]
    fn strftime_covers_common_directives() {
        let dt = date(2024, 3, 15);
        let fmt = |f: &str| {
            s(&call(
                "strftime",
                &dt,
                &[RubyValue::Str(string_new(f.into()))],
            ))
        };
        assert_eq!(fmt("%Y-%m-%d (%A)"), "2024-03-15 (Friday)");
        assert_eq!(fmt("%b %-d, %Y"), "Mar 15, 2024");
        // The time-of-day directives render midnight, and `%Z` is the offset.
        assert_eq!(fmt("%H:%M:%S %z %:z %Z"), "00:00:00 +0000 +00:00 +00:00");
    }

    #[test]
    fn format_shorthands_match_ruby() {
        let dt = date(2024, 2, 29);
        assert_eq!(s(&call("ctime", &dt, &[])), "Thu Feb 29 00:00:00 2024");
        assert_eq!(s(&call("rfc3339", &dt, &[])), "2024-02-29T00:00:00+00:00");
        assert_eq!(
            s(&call("httpdate", &dt, &[])),
            "Thu, 29 Feb 2024 00:00:00 GMT"
        );
        assert_eq!(s(&call("jisx0301", &dt, &[])), "R06.02.29");
        assert_eq!(
            s(&call("rfc2822", &dt, &[])),
            "Thu, 29 Feb 2024 00:00:00 +0000"
        );
    }
}

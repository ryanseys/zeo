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
//! `clamp` all fall out of the `comparable` method table once `<=>`
//! exists -- only `<=>` is defined here.

use std::sync::Arc;
use zeo_macros::ruby_class;

use crate::builtins::{arg_error, type_error};
use crate::dispatch::{RObj, RubyObject};
use crate::{RubyValue, Signal};
use zeo_abi::{ClassId, TIME_CLASS};

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
fn time_exact(num: num_bigint::BigInt, den: num_bigint::BigInt, offset: Option<i32>) -> RubyValue {
    use num_bigint::BigInt;
    use num_integer::Integer;
    let (num, den) = if den < BigInt::from(0) {
        (-num, -den)
    } else {
        (num, den)
    };
    let g = num.gcd(&den);
    let (num, den) = if g > BigInt::from(1) {
        (num / &g, den / &g)
    } else {
        (num, den)
    };
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
    time_exact(
        BigInt::from(sec) * &billion + BigInt::from(nsec),
        billion,
        offset,
    )
}

/// Build a LOCAL Time from raw epoch parts -- for the other builtins that
/// answer a Time (`File.mtime`), so they don't need `RTime`'s internals.
pub(crate) fn time_from_parts(sec: i64, nsec: u32) -> RubyValue {
    time_value(sec, nsec, None)
}

/// A blank `Time` -- what `Time.allocate` answers, and what `Class#new`
/// allocates before running a reopened Ruby `initialize`.
///
/// `den` is zero, which no real instant can be (it is a rational denominator
/// and always positive), so the marker costs no field and no bytes.
fn time_uninit() -> RubyValue {
    use num_bigint::BigInt;
    RubyValue::Object(Arc::new(RTime {
        num: BigInt::from(0),
        den: BigInt::from(0),
        offset: parking_lot::Mutex::new(None),
    }))
}

/// The receiver's instant, or CRuby's refusal for one that was allocated and
/// never initialized.
///
/// Fallible because ruby's is: every `Time` row raises `uninitialized Time`
/// on a blank receiver, and only object IDENTITY answers without reading the
/// instant. A panic would be wrong twice over -- it is a rescuable TypeError
/// there, and a program reaches this by writing `Time.allocate`.
fn recv_time(recv: &RubyValue) -> Result<&RTime, Signal> {
    match recv {
        RubyValue::Object(o) => {
            let t = o
                .as_any()
                .downcast_ref::<RTime>()
                .expect("Time table row dispatched on a non-Time receiver");
            if t.den.sign() == num_bigint::Sign::NoSign {
                return Err(crate::builtins::type_error!("uninitialized Time"));
            }
            Ok(t)
        }
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
    // BEFORE the zone's first transition every zoneinfo file records LMT --
    // local MEAN time, the town-clock offset to the second (`-07:52:58` for
    // Los Angeles). CRuby never reports it: `localtime_r` fails that far back
    // and `find_time_t` falls back to a representative offset, so
    // `Time.local(0).utc_offset` is -28800 there. Answer the offset the FIRST
    // real rule established instead, which is what that fallback amounts to.
    if let Some(first) = tz.following(jiff::Timestamp::MIN).next()
        && ts < first.timestamp()
    {
        return (
            first.offset().seconds(),
            i32::from(first.dst().is_dst()),
            first.abbreviation().to_string(),
        );
    }
    let info = tz.to_offset_info(ts);
    let isdst = i32::from(info.dst().is_dst());
    (
        info.offset().seconds(),
        isdst,
        info.abbreviation().to_string(),
    )
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

/// strftime's `%z` family, rendered CRuby's way (`strftime.c:546`): the offset
/// runs through the same signed width/precision macro every numeric directive
/// uses, which is how the pad flags reach it at all.
///
/// That is also why the flagged answers look like artifacts rather than
/// intent, and they are still what the oracle prints:
///
/// * `_` pads the HOUR field with a space, which costs the field a digit --
///   `+0530` becomes ` +530`.
/// * `-` flips the sign of a UTC offset, so `%-z` on a UTC time is `-0000`.
///   Only a `utc?` Time, never a numeric `+00:00`, which is why this needs
///   `gmt` rather than a test on the offset being zero.
///
/// `left` is sticky (any `-` among the flags) while `pad` is last-wins, which
/// is what makes `%-_z` (` -000`) and `%_-z` (`-0000`) differ.
fn offset_str_flagged(
    off: i32,
    colons: usize,
    pad: Option<char>,
    left: bool,
    gmt: bool,
    width: Option<usize>,
) -> String {
    let negative = off < 0 || (gmt && left);
    let a = off.abs();
    let (h, m, s) = (a / 3600, (a % 3600) / 60, a % 60);
    let signed_h = if negative { -(h as i64) } else { h as i64 };
    let space = pad == Some('_');
    // An explicit `%<w>z` width widens the HOUR field (the minutes keep
    // their two digits), zero-filled through the same signed macro:
    // `%10z` on UTC is `+000000000`.
    let hw = width.map_or(3, |w| w.saturating_sub(2).max(3));
    let mut out = if space {
        format!("{signed_h:+hw$}")
    } else {
        format!("{signed_h:+0hw$}")
    };
    // An offset smaller than an hour has no sign of its own to carry (`-0` is
    // `0`), so a negative one is written over the field's rendered `+`.
    if negative && h == 0 {
        let at = usize::from(space);
        out.replace_range(at..=at, "-");
    }
    // `%:::z` stops at the coarsest field that leaves no remainder.
    if colons == 3 && m == 0 && s == 0 {
        return out;
    }
    if colons >= 1 {
        out.push(':');
    }
    out.push_str(&format!("{m:02}"));
    if colons == 3 && s == 0 {
        return out;
    }
    if colons >= 2 {
        out.push_str(&format!(":{s:02}"));
    }
    out
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
    // The four-digit pad covers the DIGITS, not the sign: ruby renders year -1
    // as "-0001", where a plain `{:04}` spends one of the four columns on the
    // minus and gives "-001".
    let year = c.tm.tm_year + 1900;
    let year = if year < 0 {
        format!("-{:04}", -(year as i64))
    } else {
        format!("{year:04}")
    };
    format!(
        "{}-{:02}-{:02} {:02}:{:02}:{:02}{} {}",
        year,
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

/// `%U`/`%W` -- CRuby's `weeknumber` (strftime.c): complete weeks elapsed
/// since the year's first `firstweekday`, 00..53. `yday` is 0-based, as the
/// `tm` field it reads is, and `wday` is 0=Sunday. `%U` counts from Sunday,
/// `%W` from Monday -- which is the whole difference, expressed by rotating
/// `wday` before the division.
fn weeknumber(yday: i32, wday: i32, first_is_monday: bool) -> i32 {
    let wday = if first_is_monday {
        if wday == 0 { 6 } else { wday - 1 }
    } else {
        wday
    };
    ((yday + 7 - wday) / 7).max(0)
}

/// `%V` -- the ISO 8601 week number, CRuby's `iso8601wknum`. Weeks start on
/// Monday and week 1 is the one holding the year's first Thursday, so a
/// year's opening days can belong to the LAST week of the year before (the
/// `weeknum == 0` recursion) and its closing days to week 1 of the next (the
/// December fixup).
fn iso8601_weeknum(year: i64, yday: i32, wday: i32, mon: i32, mday: i32) -> i32 {
    let mut weeknum = weeknumber(yday, wday, true);
    let mut jan1day = wday - (yday % 7);
    if jan1day < 0 {
        jan1day += 7;
    }
    match jan1day {
        // Monday: the plain count is already the ISO week.
        1 => {}
        // Tue/Wed/Thu: Jan 1 falls in the week holding the first Thursday.
        2..=4 => weeknum += 1,
        // Fri/Sat/Sun: those opening days belong to last year's final week.
        _ => {
            if weeknum == 0 {
                let ly = year - 1;
                let ly_wday = if jan1day == 0 { 6 } else { jan1day - 1 };
                let ly_yday = 364 + i32::from(is_leap_year(ly));
                weeknum = iso8601_weeknum(ly, ly_yday, ly_wday, 12, 31);
            }
        }
    }
    // A late-December Mon/Tue/Wed already sits in next year's week 1.
    if mon == 12
        && ((wday == 1 && (29..=31).contains(&mday))
            || (wday == 2 && (mday == 30 || mday == 31))
            || (wday == 3 && mday == 31))
    {
        weeknum = 1;
    }
    weeknum
}

fn is_leap_year(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

/// `%V` for a broken-down instant.
fn iso_week(tm: &Tm) -> i32 {
    iso8601_weeknum(
        tm.tm_year as i64 + 1900,
        tm.tm_yday,
        tm.tm_wday,
        tm.tm_mon + 1,
        tm.tm_mday,
    )
}

/// `%G`/`%g` -- the year the ISO week belongs to. December in week 1 has
/// already rolled over; January in week 52+ has not yet.
fn iso_year(tm: &Tm) -> i64 {
    let year = tm.tm_year as i64 + 1900;
    let week = iso_week(tm);
    match tm.tm_mon + 1 {
        12 if week == 1 => year + 1,
        1 if week >= 52 => year - 1,
        _ => year,
    }
}

/// Ruby's `strftime` -- hand-ported directive table (the Ruby-specific part;
/// C's own strftime lacks Ruby's flags and several directives).
///
/// Supports the `-` (no padding) and `0`/`_` (pad with zero/space) flags that
/// Ruby adds on top of C's set. An UNKNOWN directive is emitted verbatim,
/// including its `%`, which is what Ruby does rather than raising.
fn strftime(t: &RTime, fmt: &str) -> String {
    let c = civil(t);
    render_strftime(
        &Broken {
            tm: c.tm,
            offset: c.offset,
            zone: c.zone,
            is_utc: t.is_utc(),
            epoch: t.sec(),
            nsec: t.nsec(),
            date_mode: false,
        },
        fmt,
    )
}

/// The broken-down instant [`render_strftime`] reads: everything the directive
/// table needs and nothing about where it came from. `date` renders its own
/// reform-aware civil fields through this same oracle-swept engine rather than
/// carrying a second copy of the directive table.
pub(crate) struct Broken {
    tm: Tm,
    offset: i32,
    zone: String,
    is_utc: bool,
    epoch: i64,
    nsec: u32,
    /// `date`'s two extra directives (`%Q`, `%+`), which `Time#strftime`
    /// leaves verbatim.
    date_mode: bool,
}

impl Broken {
    /// A `date`-side instant: civil fields the caller already resolved against
    /// the calendar-reform start, plus the offset the date carries.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_date(
        year: i64,
        mon: i64,
        mday: i64,
        hour: i32,
        min: i32,
        sec: i32,
        wday: i32,
        yday: i32,
        offset: i32,
        epoch: i64,
        nsec: u32,
    ) -> Broken {
        Broken {
            tm: Tm {
                tm_year: (year - 1900) as i32,
                tm_mon: (mon - 1) as i32,
                tm_mday: mday as i32,
                tm_hour: hour,
                tm_min: min,
                tm_sec: sec,
                tm_wday: wday,
                tm_yday: yday,
                tm_isdst: 0,
            },
            offset,
            // `date` renders `%Z` as the COLON offset (`"+09:00"`), where a
            // Time answers a zone abbreviation -- oracle-verified.
            zone: offset_str_colon(offset, 1),
            is_utc: false,
            epoch,
            nsec,
            date_mode: true,
        }
    }
}

/// A format whose LAST directive never reached a conversion character
/// (`"%"`, `"abc%"`, `"%-"`). `Time#strftime` raises on one -- and names the
/// whole format in the message -- where `Date#strftime` echoes it verbatim,
/// which is why the check sits beside the Time row rather than in the shared
/// renderer.
fn incomplete_directive(fmt: &str) -> bool {
    let b = fmt.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] != b'%' {
            i += 1;
            continue;
        }
        i += 1;
        while i < b.len()
            && (matches!(b[i], b'-' | b'0' | b'_' | b'^' | b'#' | b':') || b[i].is_ascii_digit())
        {
            i += 1;
        }
        // `E`/`O` are locale MODIFIERS only when a directive follows; a
        // trailing `%E` is a complete (unknown) directive ruby echoes.
        if i + 1 < b.len() && matches!(b[i], b'E' | b'O') {
            i += 1;
        }
        if i >= b.len() {
            return true;
        }
        i += 1;
    }
    false
}

pub(crate) fn render_strftime(b: &Broken, fmt: &str) -> String {
    let tm = &b.tm;
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        // Flags, then an optional field WIDTH, then the directive. `:` flags
        // only precede `z` (`%:z` etc.); `^`/`#` upcase/swapcase the result.
        // `raw` keeps everything consumed after the `%`, so a combination
        // ruby itself refuses (a width after a `:` flag) can be re-emitted
        // verbatim.
        let mut pad: Option<char> = None;
        let mut colons = 0usize;
        let (mut upcase, mut swapcase) = (false, false);
        // `-` is remembered on its own as well as in `pad`: `%z` reads it as a
        // sign flip independently of which pad character came last, which is
        // what makes `%-_z` and `%_-z` differ.
        let mut left = false;
        let mut raw = String::new();
        while let Some(&f) = chars.peek() {
            match f {
                '-' | '0' | '_' => {
                    pad = Some(f);
                    left |= f == '-';
                }
                '^' => upcase = true,
                '#' => swapcase = true,
                ':' => colons += 1,
                // POSIX's locale modifiers. Ruby accepts and IGNORES them
                // (it has no locale alternatives), so `%Ey` renders `%y`.
                'E' | 'O' => {}
                _ => break,
            }
            raw.push(f);
            chars.next();
        }
        let mut width: Option<usize> = None;
        while let Some(d) = chars.peek().and_then(|c| c.to_digit(10)) {
            width = Some(width.unwrap_or(0) * 10 + d as usize);
            raw.push(char::from_digit(d, 10).unwrap());
            chars.next();
        }
        let Some(d) = chars.next() else {
            out.push('%');
            out.push_str(&raw);
            break;
        };
        // A width AFTER a colon flag is not a directive ruby accepts: `%:5z`
        // stays verbatim (the 13,800-case oracle sweep pins this boundary).
        if colons > 0 && width.is_some() {
            out.push('%');
            out.push_str(&raw);
            out.push(d);
            continue;
        }
        // `num_pad` applies the flag to a numeric directive: `-` drops padding,
        // `_` pads with spaces, `0` with zeros, and with no flag the
        // directive's own default pad applies (an explicit `%<n>X` width
        // overrides the directive's default WIDTH the same way).
        let num_pad = |v: i64, default_width: usize, default_pad: char| -> String {
            // `-` wins wherever it appears among the flags, so `%-_5Y` and
            // `%_-5Y` both drop the padding; `_` and `0` are last-wins between
            // themselves (`%_0m` zero-pads, `%0_m` space-pads).
            if left {
                return v.to_string();
            }
            let w = width.unwrap_or(default_width);
            match pad.unwrap_or(default_pad) {
                '_' | ' ' => format!("{:>w$}", v, w = w),
                _ => format!("{:0w$}", v, w = w),
            }
        };
        let num = |v: i64, default_width: usize| num_pad(v, default_width, '0');
        // `%e`/`%k`/`%l` are the space-padded twins of `%d`/`%H`/`%I`. The pad
        // is only their DEFAULT, so `%0e` still zero-pads.
        let num_sp = |v: i64| num_pad(v, 2, ' ');
        // `%Y` pads to four DIGITS, so a negative year carries its sign
        // outside the padding: year -1 is `-0001`, not `-001`. An explicit
        // width overrides that and counts the sign like every other field.
        let year = |v: i64| -> String {
            if left || width.is_some() {
                return num(v, 4);
            }
            let body = match pad.unwrap_or('0') {
                '_' | ' ' => format!("{:>4}", v.unsigned_abs()),
                _ => format!("{:04}", v.unsigned_abs()),
            };
            if v >= 0 {
                return body;
            }
            let at = body.find(|c: char| c != ' ').unwrap_or(0);
            format!("{}-{}", &body[..at], &body[at..])
        };
        // `%N`/`%L`'s fractional seconds to `digits` places: the 9-digit
        // nanosecond string, truncated or right-zero-padded to width.
        let frac = |digits: usize| -> String {
            let nine = format!("{:09}", b.nsec);
            if digits <= 9 {
                nine[..digits].to_string()
            } else {
                format!("{nine}{}", "0".repeat(digits - 9))
            }
        };
        let mut piece = match d {
            'Y' => year(tm.tm_year as i64 + 1900),
            'y' => num((tm.tm_year as i64 + 1900) % 100, 2),
            'C' => num((tm.tm_year as i64 + 1900) / 100, 2),
            'm' => num(tm.tm_mon as i64 + 1, 2),
            'd' => num(tm.tm_mday as i64, 2),
            'e' => num_sp(tm.tm_mday as i64),
            'j' => num(tm.tm_yday as i64 + 1, 3),
            'H' => num(tm.tm_hour as i64, 2),
            'k' => num_sp(tm.tm_hour as i64),
            'I' => num(
                if tm.tm_hour % 12 == 0 {
                    12
                } else {
                    (tm.tm_hour % 12) as i64
                },
                2,
            ),
            'l' => num_sp(if tm.tm_hour % 12 == 0 {
                12
            } else {
                (tm.tm_hour % 12) as i64
            }),
            // Week-of-year: `%U` counts from Sunday, `%W` from Monday, and
            // `%V`/`%G` are the ISO week-date pair -- what a weekly rollup or
            // a report bucket keys on.
            'U' => num(weeknumber(tm.tm_yday, tm.tm_wday, false) as i64, 2),
            'W' => num(weeknumber(tm.tm_yday, tm.tm_wday, true) as i64, 2),
            'V' => num(iso_week(tm) as i64, 2),
            // The ISO week-based YEAR, which is not the calendar year at the
            // turn: late December can already belong to week 1 of the next,
            // and early January to week 52/53 of the last.
            'G' | 'g' => {
                let y = iso_year(tm);
                if d == 'G' {
                    num(y, if y < 0 { 5 } else { 4 })
                } else {
                    num(y.rem_euclid(100), 2)
                }
            }
            'M' => num(tm.tm_min as i64, 2),
            'S' => num(tm.tm_sec as i64, 2),
            'L' => frac(width.unwrap_or(3)),
            'N' => frac(width.unwrap_or(9)),
            'z' => offset_str_flagged(b.offset, colons, pad, left, b.is_utc, width),
            'Z' => b.zone.clone(),
            'a' => DAY_NAMES[tm.tm_wday as usize][..3].to_string(),
            'A' => DAY_NAMES[tm.tm_wday as usize].to_string(),
            'b' | 'h' => MONTH_NAMES[tm.tm_mon as usize][..3].to_string(),
            'B' => MONTH_NAMES[tm.tm_mon as usize].to_string(),
            'p' => (if tm.tm_hour < 12 { "AM" } else { "PM" }).to_string(),
            'P' => (if tm.tm_hour < 12 { "am" } else { "pm" }).to_string(),
            // Numeric like every other number directive (`%5u` zero-pads),
            // just with a 1-digit natural width.
            'u' => num(
                if tm.tm_wday == 0 {
                    7
                } else {
                    tm.tm_wday as i64
                },
                1,
            ),
            'w' => num(tm.tm_wday as i64, 1),
            's' => num(b.epoch, 1),
            // The compound directives, in terms of the above.
            'F' => render_strftime(b, "%Y-%m-%d"),
            'T' | 'X' => render_strftime(b, "%H:%M:%S"),
            'D' | 'x' => render_strftime(b, "%m/%d/%y"),
            'R' => render_strftime(b, "%H:%M"),
            'r' => render_strftime(b, "%I:%M:%S %p"),
            'c' => render_strftime(b, "%a %b %e %H:%M:%S %Y"),
            // The VMS date, strftime.c's own recursive definition.
            'v' => render_strftime(b, "%e-%^b-%4Y"),
            // `date`-only: milliseconds since the epoch, and the `%+` date(1)
            // format. `Time#strftime` has neither and emits them verbatim.
            'Q' if b.date_mode => num(b.epoch * 1000 + (b.nsec / 1_000_000) as i64, 1),
            '+' if b.date_mode => render_strftime(b, "%a %b %e %H:%M:%S %Z %Y"),
            'n' => "\n".to_string(),
            't' => "\t".to_string(),
            '%' => "%".to_string(),
            // Unknown: emit verbatim, `%` included (Ruby's own behavior --
            // it does not raise).
            other => format!("%{other}"),
        };
        // `^` upcases whatever the directive produced. `#` is NOT swapcase:
        // each directive that honours it hardcodes a direction (strftime.c's
        // per-case `BIT_OF(UPPER)`/`BIT_OF(LOWER)`), and every numeric and
        // compound directive ignores it outright. So `%#A` is "THURSDAY", not
        // "tHURSDAY".
        let fold_upper = upcase || (swapcase && matches!(d, 'a' | 'A' | 'b' | 'h' | 'B' | 'P'));
        let fold_lower = !upcase && swapcase && matches!(d, 'p' | 'Z');
        if fold_upper {
            piece = piece.to_uppercase();
        } else if fold_lower {
            piece = piece.to_lowercase();
        }
        // An explicit width right-pads STRING directives too (`%10a` ->
        // "       Thu") -- space unless the `0` flag asked otherwise, `-`
        // dropping the pad as everywhere. Numeric directives already
        // consumed the width inside `num_pad`, so only the string/compound
        // set takes the generic pad.
        let string_directive = matches!(
            d,
            'a' | 'A'
                | 'b'
                | 'h'
                | 'B'
                | 'p'
                | 'P'
                | 'Z'
                | 'n'
                | 't'
                | '%'
                | 'F'
                | 'T'
                | 'X'
                | 'D'
                | 'x'
                | 'R'
                | 'r'
                | 'c'
                | 'v'
        );
        if string_directive
            && !left
            && let Some(w) = width
            && piece.chars().count() < w
        {
            let fill = if pad == Some('0') { '0' } else { ' ' };
            let missing = w - piece.chars().count();
            piece = format!("{}{piece}", fill.to_string().repeat(missing));
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
            return Err(crate::builtins::float_domain_error!(
                "{}",
                RubyValue::Float(*f).to_display_string()
            ));
        }
        other => match num_exact(other)? {
            Some(n) => return shift(t, &n, sign),
            None => {
                return Err(type_error!(
                    "can't convert {} into an exact number",
                    crate::builtins::class_name_of(other)
                ));
            }
        },
    };
    // Everything over the common denominator `den`, in nanoseconds.
    // Exact rational addition over a common denominator -- NO rounding at
    // any point, so the sub-nanosecond tail the receiver carries survives
    // (see `RTime::num`).
    let new_num = &t.num * &den + BigInt::from(sign) * num * &t.den;
    let new_den = &t.den * &den;
    Ok(time_exact(new_num, new_den, t.offset()))
}

/// CRuby's `num_exact` fallback for a value that is not already a number:
/// `to_r` first, then `to_int`. `None` when neither answers.
///
/// A value that also answers `to_str` is refused however good its `to_r` is.
/// That guard is CRuby's own, and it is load-bearing: `String#to_r` reads a
/// leading number and answers `(0/1)` for anything else, so without it
/// `Time.at("x")` would silently mean the epoch.
fn num_exact(v: &RubyValue) -> Result<Option<RubyValue>, Signal> {
    // `nil` and the booleans are refused by TYPE, before any protocol runs --
    // `nil.to_r` is `(0/1)`, so asking would read a missing argument as the
    // epoch.
    if matches!(v, RubyValue::Nil | RubyValue::Bool(_)) {
        return Ok(None);
    }
    if crate::builtins::convert::check_to_str(v)?.is_some() {
        return Ok(None);
    }
    match crate::builtins::convert::check_to_r(v)? {
        Some(r) => Ok(Some(r)),
        None => crate::builtins::convert::check_to_int(v),
    }
}

/// A count of seconds as an EXACT `(num, den)` -- `Time.at`'s argument and
/// `+`/`-`'s delta agree on this reading.
fn exact_seconds(v: &RubyValue) -> Result<(num_bigint::BigInt, num_bigint::BigInt), Signal> {
    match v {
        RubyValue::Int(_) | RubyValue::BigInt(_) | RubyValue::Rational(_) => {
            Ok(crate::builtins::rational::as_ratio(v))
        }
        RubyValue::Float(f) if f.is_finite() => Ok(crate::builtins::float::float_exact_parts(*f)),
        RubyValue::Float(f) => Err(crate::builtins::float_domain_error!(
            "{}",
            RubyValue::Float(*f).to_display_string()
        )),
        // `Time.at(another_time)` copies its exact instant.
        RubyValue::Object(o) if o.as_any().downcast_ref::<RTime>().is_some() => {
            let t = o.as_any().downcast_ref::<RTime>().unwrap();
            Ok((t.num.clone(), t.den.clone()))
        }
        other => match num_exact(other)? {
            Some(n) => exact_seconds(&n),
            None => Err(type_error!(
                "can't convert {} into an exact number",
                crate::builtins::class_name_of(other)
            )),
        },
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
/// The three-letter English month abbreviations the MONTH slot accepts, from
/// `month_arg`'s table, matched case-insensitively. Exactly three letters:
/// "December" is not one of them and falls through to the integer read, which
/// rejects it -- which is what ruby does too.
fn month_from_name(s: &str) -> Option<i64> {
    const NAMES: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    if s.len() != 3 {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    NAMES.iter().position(|n| *n == lower).map(|i| i as i64 + 1)
}

/// What an ABSENT civil field means, by slot: month and day count from 1,
/// hour/min/sec from 0. Slot 0 (the year) has no default and never asks.
fn civil_default(slot: usize) -> i64 {
    match slot {
        1 | 2 => 1,
        _ => 0,
    }
}

fn int_parts(args: &[RubyValue], take: usize) -> Result<Vec<i64>, Signal> {
    args.iter()
        .take(take)
        .enumerate()
        .map(|(slot, a)| match a {
            RubyValue::Int(i) => Ok(*i),
            // A String component is parsed as a base-10 integer (`Time.utc(
            // "2020", "3")`), matching CRuby's forced-decimal reading -- except
            // in the MONTH slot, which takes a name first.
            RubyValue::Str(s) => {
                let t = s.lock().to_utf8_lossy().trim().to_string();
                if slot == 1
                    && let Some(m) = month_from_name(&t)
                {
                    return Ok(m);
                }
                t.parse::<i64>()
                    .map_err(|_| arg_error!("argument out of range: {t:?}"))
            }
            // CRuby reads a nil component as ABSENT, so it takes its slot's
            // default -- `Time.utc(2000, 5, nil)` is the first of May. The
            // YEAR has no default and keeps the conversion error.
            RubyValue::Nil if slot > 0 => Ok(civil_default(slot)),
            // Floats truncate, `to_int` ducks convert.
            other => crate::builtins::convert::to_index(other),
        })
        .collect()
}

/// CRuby range-checks each civil field before normalizing overflow: month
/// 1..=12, day 1..=31, hour 0..=23, min 0..=59, sec 0..=60 (60 is the leap
/// second). Feb 30 / 23:59:60 stay in range and roll forward; month 13 / day 32
/// / hour 25 / min 60 are `ArgumentError`. `parts` is `[year, mon, day, hour,
/// min, sec]`, any trailing entries absent.
fn validate_civil_parts(parts: &[i64]) -> Result<(), Signal> {
    // The FIELD is named, as CRuby names it -- `mon out of range`, not one
    // generic text for every field. `mday` and `mon` are CRuby's own
    // spellings, not `day` and `month`.
    let ranges = [
        (1usize, "mon", 1, 12),
        (2, "mday", 1, 31),
        (3, "hour", 0, 23),
        (4, "min", 0, 59),
        (5, "sec", 0, 60),
    ];
    for (i, field, lo, hi) in ranges {
        if let Some(&v) = parts.get(i)
            && (v < lo || v > hi)
        {
            return Err(arg_error!("{field} out of range"));
        }
    }
    Ok(())
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
/// `Time.utc`/`local` accept one to eight civil fields, PLUS the ten-element
/// form `Time#to_a` produces (which `normalize_civil_args` reverses). CRuby
/// special-cases the ten the same way, and still reports `1..8` for the rest.
fn check_civil_argc(args: &[RubyValue]) -> Result<(), Signal> {
    match args.len() {
        10 => Ok(()),
        n => crate::builtins::check_arity(n, 1, Some(8)),
    }
}

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
    let cant = || arg_error!("can't parse: {input:?}");
    let mut tokens = input.split_whitespace();
    let mut date = tokens.next().ok_or_else(cant)?.split('-');
    let year: i64 = date.next().and_then(|x| x.parse().ok()).ok_or_else(cant)?;
    let mon: i64 = date.next().and_then(|x| x.parse().ok()).ok_or_else(cant)?;
    let day: i64 = date.next().and_then(|x| x.parse().ok()).ok_or_else(cant)?;
    if date.next().is_some() {
        return Err(cant());
    }
    let Some(time) = tokens.next() else {
        return Err(arg_error!("no time information"));
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
        Some(f) if !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()) => (
            f.parse::<BigInt>().map_err(|_| cant())?,
            BigInt::from(10).pow(f.len() as u32),
        ),
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
    Ok(build_civil_time(
        &[year, mon, day, hour, min, sec],
        frac_num,
        frac_den,
        offset,
    ))
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
        // `utc_offset_arg` reads the slot through `rb_check_string_type`
        // first, so anything with a `to_str` spells its own offset.
        Some(other) => match crate::builtins::convert::check_to_str(other)? {
            Some(RubyValue::Str(s)) => Ok(Some(parse_offset(&s.lock().to_utf8_lossy())?)),
            _ => Err(arg_error!(
                "\"+HH:MM\" expected for utc_offset: {}",
                other.to_display_string()
            )),
        },
    }
}

fn check_offset(off: i64) -> Result<i32, Signal> {
    if !(-86400 < off && off < 86400) {
        return Err(arg_error!("utc_offset out of range"));
    }
    Ok(off as i32)
}

/// A `"+HH:MM"` / `"-HH:MM:SS"` / `"+HHMM"` / `"UTC"` / `"Z"` offset String,
/// as seconds east of UTC -- `Time.new`'s 7th argument may be spelled any of
/// those ways. `"UTC"`/`"Z"` answer the [`RTime::UTC`] sentinel rather than a
/// plain zero: they name UTC ITSELF, so `#utc?` is true and `#zone` is "UTC",
/// where a `"+00:00"` Time is merely at offset zero.
fn parse_offset(s: &str) -> Result<i32, Signal> {
    let bad = || {
        arg_error!(
            "\"+HH:MM\", \"-HH:MM\", \"UTC\" or \"A\"..\"I\",\"K\"..\"Z\" expected for utc_offset: {s}"
        )
    };
    if s.eq_ignore_ascii_case("utc") || s == "Z" || s == "z" {
        return Ok(RTime::UTC);
    }
    let sign = match s.as_bytes().first() {
        Some(b'+') => 1,
        Some(b'-') => -1,
        _ => return Err(bad()),
    };
    let rest = &s[1..];
    let (h, m, sec): (i64, i64, i64) = if rest.contains(':') {
        let mut parts = rest.split(':');
        let num = |p: Option<&str>, d: i64| -> Result<i64, Signal> {
            match p {
                None => Ok(d),
                Some(t) => t.parse().map_err(|_| bad()),
            }
        };
        (
            num(parts.next(), 0)?,
            num(parts.next(), 0)?,
            num(parts.next(), 0)?,
        )
    } else {
        // The COMPACT form: `+HH`, `+HHMM`, `+HHMMSS` (`Time.new(.., "+0900")`).
        if !rest.bytes().all(|b| b.is_ascii_digit()) || !matches!(rest.len(), 2 | 4 | 6) {
            return Err(bad());
        }
        let at = |i: usize| -> i64 { rest[i..i + 2].parse().unwrap_or(0) };
        (
            at(0),
            if rest.len() >= 4 { at(2) } else { 0 },
            if rest.len() == 6 { at(4) } else { 0 },
        )
    };
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
        Some(RubyValue::Int(_)) => Err(arg_error!("subsecx out of range")),
        Some(v @ (RubyValue::Float(_) | RubyValue::Rational(_))) => {
            let usec = match v {
                RubyValue::Float(f) => *f,
                RubyValue::Rational(r) => crate::builtins::rational::rat_to_f64(r),
                _ => unreachable!("the arm pattern binds Float and Rational only"),
            };
            if !(0.0..1_000_000.0).contains(&usec) {
                return Err(arg_error!("subsecx out of range"));
            }
            Ok((usec * 1000.0) as u32)
        }
        // NOT the generic implicit-conversion shape: CRuby's time argument
        // check says "can't convert X into an exact number" (class-named --
        // `Time.at(0, nil)` spells NilClass; oracle-verified).
        Some(other) => Err(type_error!(
            "can't convert {} into an exact number",
            crate::builtins::class_name_of(other)
        )),
    }
}

/// `Time.utc(y, mo, d, h, mi, s)` -- civil fields to epoch seconds, the UTC
/// inverse of `broken_down`. Out-of-range fields normalize (an over-large
/// month rolls into the year; day/hour/min/sec overflow just accumulate as
/// seconds), matching `timegm`; the Ruby-visible range check is
/// `validate_civil_parts`, which every constructor runs first.
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

/// Rounding mode for `Time#round`/`#floor`/`#ceil`.
enum Rounding {
    Floor,
    Ceil,
    Round,
}

/// The sub-second precision argument (`ndigits`, default 0), as a non-negative
/// count of decimal places.
fn round_ndigits(ndigits: Option<&RubyValue>) -> Result<u32, Signal> {
    match ndigits {
        // An explicit nil precision is accepted as absent (oracle-verified).
        None | Some(RubyValue::Nil) => Ok(0),
        Some(v) => {
            // A NEGATIVE count is refused, not clamped: there is no such thing
            // as sub-second precision coarser than a second here, and ruby says
            // so rather than quietly rounding to whole seconds.
            let n = crate::builtins::convert::to_index(v)?;
            if n < 0 {
                return Err(arg_error!("negative ndigits given"));
            }
            Ok(n as u32)
        }
    }
}

/// A new Time with the instant reduced to `10**ndigits`-of-a-second precision.
/// `round` is half-up toward +Infinity (`Time.at(-0.5).round` is `0`, not `-1`
/// -- oracle-verified): `floor(value*scale + 1/2) / scale`.
fn time_reduce(
    t: &RTime,
    ndigits: Option<&RubyValue>,
    kind: Rounding,
) -> Result<RubyValue, Signal> {
    use num_integer::Integer;
    let scale = num_bigint::BigInt::from(10u32).pow(round_ndigits(ndigits)?);
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

// ------------------------------------------------------------------ marshal

/// CRuby's `Time#_dump` payload (`time.c` `time_mdump`): 8 bytes of
/// bit-packed UTC civil time -- word `p` = marker|utc|year|mon|mday|hour,
/// word `s` = min|sec|usec, both little-endian -- plus a year-extension tail
/// when the year leaves 1900..67435. The wire also hangs instance variables
/// beside the bytes (`nano_num`/`nano_den`, the 1.9.1-compat `submicro`,
/// `offset`, `zone`); zeo strings carry no ivars, so the marshal writer takes
/// this pair while the `_dump` ROW answers the bytes alone.
pub(crate) fn time_mdump(t: &RTime) -> (Vec<u8>, Vec<(String, RubyValue)>) {
    use num_bigint::BigInt;
    let utc_p = t.is_utc();
    let tm = broken_down(t.sec());
    let year = i64::from(tm.tm_year) + 1900;
    const MAX_YEAR: i64 = 1900 + 0xffff;
    let (year_field, year_extend) = if year > MAX_YEAR {
        (MAX_YEAR, Some(BigInt::from(year - MAX_YEAR)))
    } else if year < 1900 {
        (1900, Some(BigInt::from(1900 - year)))
    } else {
        (year, None)
    };
    // The fraction in nanoseconds: whole ns splits into usec + the three
    // sub-usec digits; anything below a nanosecond rides along exactly.
    let (fnum, fden) = t.frac();
    let nano_scaled = &fnum * BigInt::from(1_000_000_000u32);
    let whole_ns = &nano_scaled / &fden;
    let subnano_num = &nano_scaled - &whole_ns * &fden;
    let whole_ns = i64::try_from(&whole_ns).unwrap_or(0);
    let usec = (whole_ns / 1000) as u32;
    let nsec = whole_ns % 1000;

    let p: u32 = 1u32 << 31
        | u32::from(utc_p) << 30
        | (((year_field - 1900) as u32) & 0xffff) << 14
        | (tm.tm_mon as u32) << 10
        | (tm.tm_mday as u32) << 5
        | tm.tm_hour as u32;
    let s: u32 = (tm.tm_min as u32) << 26 | (tm.tm_sec as u32) << 20 | usec;
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(&p.to_le_bytes());
    bytes.extend_from_slice(&s.to_le_bytes());
    if let Some(ext) = &year_extend {
        let (_, mag) = ext.to_bytes_le();
        crate::builtins::marshal::marshal_long_into(mag.len() as i64, &mut bytes);
        bytes.extend_from_slice(&mag);
    }

    let ascii_str = |b: &[u8]| {
        RubyValue::Str(crate::string_from_bytes(
            b.to_vec(),
            crate::encoding::US_ASCII,
        ))
    };
    let mut ivars: Vec<(String, RubyValue)> = Vec::new();
    // `nano` = the sub-usec remainder as an exact value in nanoseconds; an
    // integral one dumps as `(n, 1)` like CRuby's Integer branch.
    let nano_num = BigInt::from(nsec) * &fden + &subnano_num;
    if nano_num != BigInt::from(0) {
        use num_integer::Integer;
        let g = nano_num.gcd(&fden);
        ivars.push((
            "nano_num".to_string(),
            crate::builtins::integer::int_value(&nano_num / &g),
        ));
        ivars.push((
            "nano_den".to_string(),
            crate::builtins::integer::int_value(&fden / &g),
        ));
    }
    if nsec != 0 {
        // Fixed-point packed BCD of the three sub-usec digits, second byte
        // dropped when zero -- 1.9.1 compatibility, byte-for-byte.
        let b0 = (((nsec / 100) as u8) << 4) | ((nsec / 10) % 10) as u8;
        let b1 = ((nsec % 10) as u8) << 4;
        let buf = if b1 == 0 { vec![b0] } else { vec![b0, b1] };
        ivars.push((
            "submicro".to_string(),
            RubyValue::Str(crate::string_from_bytes(buf, crate::encoding::ASCII_8BIT)),
        ));
    }
    let zone = if utc_p {
        ascii_str(b"UTC")
    } else {
        let c = civil(t);
        ivars.push(("offset".to_string(), RubyValue::Int(i64::from(c.offset))));
        if c.zone.is_empty() {
            RubyValue::Nil
        } else {
            ascii_str(c.zone.as_bytes())
        }
    };
    ivars.push(("zone".to_string(), zone));
    (bytes, ivars)
}

/// CRuby's `Time._load` (`time.c` `time_mload`): the inverse of
/// [`time_mdump`], including the pre-1.8 plain `(sec, usec)` format when the
/// marker bit is clear. A `zone` STRING re-localizes the loaded value (zeo:
/// `offset = None`, so the system zone re-resolves the abbreviation -- exact
/// on the dumping machine, divergent across zones, documented); a bare
/// `offset` stays fixed; the `gmt` bit stays UTC.
pub(crate) fn time_mload(bytes: &[u8], ivars: &[(String, RubyValue)]) -> Result<RubyValue, Signal> {
    use num_bigint::BigInt;
    if bytes.len() < 8 {
        return Err(type_error!("marshaled time format differ"));
    }
    let p = u32::from_le_bytes(bytes[0..4].try_into().expect("length checked"));
    let s = u32::from_le_bytes(bytes[4..8].try_into().expect("length checked"));
    if p & (1 << 31) == 0 {
        // Pre-1.8 dump: two plain words, local time.
        return Ok(time_value(i64::from(p), s.wrapping_mul(1000), None));
    }
    let ivar = |name: &str| ivars.iter().find(|(n, _)| n == name).map(|(_, v)| v);

    let utc_p = (p >> 30) & 1 == 1;
    let mut year = i64::from((p >> 14) & 0xffff) + 1900;
    if bytes.len() > 8 {
        let mut tail = &bytes[8..];
        let ysize = read_marshal_long(&mut tail)?;
        if ysize < 0 || ysize as usize > tail.len() {
            return Err(type_error!("marshaled time format differ"));
        }
        let ext = BigInt::from_bytes_le(num_bigint::Sign::Plus, &tail[..ysize as usize]);
        let ext = i64::try_from(&ext).unwrap_or(i64::MAX / 2);
        if year == 1900 {
            year -= ext;
        } else {
            year += ext;
        }
    }
    let mut mon = i64::from((p >> 10) & 0xf);
    if mon >= 12 {
        mon -= 12;
        year += 1;
    }
    let mday = i64::from((p >> 5) & 0x1f);
    let hour = i64::from(p & 0x1f);
    let min = i64::from((s >> 26) & 0x3f);
    let sec = i64::from((s >> 20) & 0x3f);
    let usec = i64::from(s & 0xfffff);

    let epoch = days_from_civil(year, mon + 1, mday) * 86_400 + hour * 3600 + min * 60 + sec;
    let billion = BigInt::from(1_000_000_000u32);

    // subsec = usec + nano (exact) -- nano_num/nano_den win over submicro.
    let (mut num, mut den) = (
        BigInt::from(epoch) * &billion + BigInt::from(usec * 1000),
        billion.clone(),
    );
    if let Some(nn) = ivar("nano_num") {
        let nn = to_bigint(nn)?;
        let nd = match ivar("nano_den") {
            Some(v) => to_bigint(v)?,
            None => BigInt::from(1),
        };
        num = num * &nd + nn;
        den *= nd;
    } else if let Some(RubyValue::Str(sm)) = ivar("submicro") {
        let b = sm.lock().bytes().to_vec();
        let mut nsec = 0i64;
        'bcd: {
            if let Some(&b0) = b.first() {
                let (h, t) = (i64::from(b0 >> 4), i64::from(b0 & 0xf));
                if h >= 10 {
                    break 'bcd;
                }
                nsec += h * 100;
                if t >= 10 {
                    break 'bcd;
                }
                nsec += t * 10;
                if let Some(&b1) = b.get(1) {
                    let u = i64::from(b1 >> 4);
                    if u >= 10 {
                        break 'bcd;
                    }
                    nsec += u;
                }
            }
        }
        num += BigInt::from(nsec);
    }

    let offset = if utc_p {
        Some(RTime::UTC)
    } else if matches!(ivar("zone"), Some(RubyValue::Str(_))) {
        None
    } else if let Some(off) = ivar("offset") {
        Some(i32::try_from(crate::builtins::convert::to_index(off)?).unwrap_or(0))
    } else {
        None
    };
    Ok(time_exact(num, den, offset))
}

fn to_bigint(v: &RubyValue) -> Result<num_bigint::BigInt, Signal> {
    Ok(match crate::builtins::convert::to_int(v)? {
        RubyValue::Int(n) => num_bigint::BigInt::from(n),
        RubyValue::BigInt(b) => (*b).clone(),
        _ => unreachable!("to_int post-checks its answer"),
    })
}

/// Marshal's variable-length LONG, read off the front of `tail` -- the
/// year-extension header [`time_mdump`] embeds inside the `u` data.
fn read_marshal_long(tail: &mut &[u8]) -> Result<i64, Signal> {
    let take = |tail: &mut &[u8]| -> Result<u8, Signal> {
        let (&b, rest) = tail
            .split_first()
            .ok_or_else(|| type_error!("marshaled time format differ"))?;
        *tail = rest;
        Ok(b)
    };
    let c = take(tail)? as i8;
    Ok(match c {
        0 => 0,
        1..=4 => {
            let mut x = 0i64;
            for i in 0..c as usize {
                x |= i64::from(take(tail)?) << (8 * i);
            }
            x
        }
        -4..=-1 => {
            let mut x = -1i64;
            for i in 0..(-c) as usize {
                x &= !(0xffi64 << (8 * i));
                x |= i64::from(take(tail)?) << (8 * i);
            }
            x
        }
        5.. => i64::from(c) - 5,
        _ => i64::from(c) + 5,
    })
}

ruby_class! {
    Time = zeo_abi::TIME_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::COMPARABLE_CLASS;

    allocate time_uninit;

    def self."now" params "in: nil" as time_now cfunc allocs (_recv) {
        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before the Unix epoch");
        Ok(time_value(d.as_secs() as i64, d.subsec_nanos(), None))
    }
    // `Time.at(sec)` / `Time.at(sec, frac[, unit])` -- the second argument is a
    // fractional count in `unit` (default `:microsecond`; also `:millisecond`,
    // `:nanosecond`), added to the base seconds exactly.
    // The `in:` keyword supplies the DISPLAY utc_offset (the instant itself is
    // the absolute epoch value, so no shift -- unlike `Time.new`, whose
    // components are local to that offset).
    // `in:` is a NAMED keyword, spelled `r#in` because ruby's name is a Rust
    // one. That is what keeps the arity range at ruby's 1..3 -- a `**kwrest`
    // would peel any trailing hash before the guard ran (so `Time.at({})`
    // lost its only argument) and a fourth optional SLOT reported 1..4.
    ruby def self."at" allocs (_recv, time, subsec?, unit?, r#in:?) {
        use num_bigint::BigInt;
        let in_offset = r#in.filter(|v| !v.is_nil());
        let offset = match &in_offset {
            None => None,
            Some(RubyValue::Int(off)) => Some(check_offset(*off)?),
            Some(RubyValue::Str(s)) => Some(parse_offset(&s.lock().to_utf8_lossy())?),
            Some(other) => return Err(type_error!("can't convert {} into an exact number",
                    crate::builtins::convert_name_of(other))),
        };
        let (base_num, base_den) = exact_seconds(time)?;
        let (num, den) = match subsec {
            None => (base_num, base_den),
            Some(frac) => {
                let scale = match unit {
                    None => 1_000_000i64,
                    Some(RubyValue::Symbol(s)) => match s.name().as_str() {
                        "millisecond" => 1_000,
                        "microsecond" | "usec" => 1_000_000,
                        "nanosecond" | "nsec" => 1_000_000_000,
                        other => return Err(arg_error!("unexpected unit: {other}")),
                    },
                    Some(other) => return Err(arg_error!("unexpected unit: {}", crate::builtins::class_name_of(other))),
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
    // constructors genuinely differ, oracle-verified). An out-of-range field
    // (`Time.utc(2023, 13, 1)`) raises ArgumentError ("mon out of range")
    // before `civil_to_epoch_utc` can normalize it.
    def self."utc" | "gm" cfunc allocs (_recv, *args) {
        check_civil_argc(args)?;
        let norm = normalize_civil_args(args);
        let args = norm.as_slice();
        if let Some((parts, frac_num, frac_den)) = frac_seconds(args)? {
            let epoch = civil_to_epoch_utc(&parts);
            return Ok(time_exact(num_bigint::BigInt::from(epoch) * &frac_den + &frac_num, frac_den, Some(RTime::UTC)));
        }
        let parts = int_parts(args, 6)?;
        validate_civil_parts(&parts)?;
        let nsec = subsec_nsec_arg(args.get(6))?;
        Ok(time_value(civil_to_epoch_utc(&parts), nsec, Some(RTime::UTC)))
    }
    // `Time.new(y, mo, d, h, mi, s, utc_offset)` -- the 7th argument is the
    // OFFSET, in seconds or as a `"+HH:MM"` String, unlike `Time.utc`'s
    // microseconds. With no offset given it is local time, like `Time.local`.
    // The `in:` keyword offset takes the place of the 7th positional argument.
    // Ruby reaches this through `Class#new`, which is what `Time.method(:new)`
    // reports owning -- `Time` declares only `initialize`.
    def self."new" inherits cfunc allocs (recv, year?, mon?, mday?, hour?, min?, sec?, zone?, **opts) {
        // `Time.new("2021-12-25 10:00:00 +09:00")` parses a time string.
        if let Some(RubyValue::Str(s)) = year {
            return parse_time_string(&s.lock().to_utf8_lossy());
        }
        let in_offset = opts.map(|h| match h {
            RubyValue::Hash(h) => crate::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("in"))),
            _ => RubyValue::Nil,
        });
        let args: Vec<RubyValue> = [year, mon, mday, hour, min, sec, zone]
            .iter()
            .take_while(|p| p.is_some())
            .filter_map(|p| p.cloned())
            .collect();
        if args.is_empty() && in_offset.is_none() {
            return time_now(recv, &[], None);
        }
        let args = args.as_slice();
        let parts = int_parts(args, 6)?;
        validate_civil_parts(&parts)?;
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
            Some(other) => Err(type_error!("can't convert {} into an exact number",
                    crate::builtins::convert_name_of(other))),
        }
    }
    // `Time.local`/`Time.mktime` -- the same civil fields read as LOCAL
    // time. `timegm` gives the UTC instant for those fields; subtracting the
    // offset in effect THERE converts it to the local reading. (Computing
    // the offset at the UTC instant rather than the local one is off by an
    // hour for civil times inside a DST transition; that edge is a
    // documented approximation, not a silent one.)
    def self."local" | "mktime" cfunc allocs (_recv, *args) {
        check_civil_argc(args)?;
        let norm = normalize_civil_args(args);
        let args = norm.as_slice();
        let frac = frac_seconds(args)?;
        let parts = match &frac {
            Some((parts, ..)) => parts.clone(),
            None => int_parts(args, 6)?,
        };
        validate_civil_parts(&parts)?;
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

    def "to_i" | "tv_sec" (recv) {
        Ok(RubyValue::Int(recv_time(recv)?.sec()))
    }
    def "to_f" (recv) {
        let t = recv_time(recv)?;
        // From the exact rational, not from `sec + nsec/1e9`: that rounds
        // twice and can't round-trip a Float epoch (`Time.at(1.25).to_f`).
        let (n, d) = (t.num.clone(), t.den.clone());
        Ok(RubyValue::Float(bigint_to_f64(&n) / bigint_to_f64(&d)))
    }
    def "nsec" | "tv_nsec" (recv) {
        Ok(RubyValue::Int(recv_time(recv)?.nsec() as i64))
    }
    def "usec" | "tv_usec" (recv) {
        Ok(RubyValue::Int((recv_time(recv)?.nsec() / 1000) as i64))
    }
    // The fraction of a second, EXACTLY: a Rational (`Time.at(0.5).subsec`
    // is `(1/2)`, not 0.5), or Integer 0 for a whole second -- oracle-
    // verified. `rational_new` reduces, which is what turns 500000000/1e9
    // into 1/2.
    def "subsec" (recv) {
        // The EXACT fraction, whatever its denominator -- `Time.at(10.8).subsec`
        // is `(225179981368525/281474976710656)`, the double's true value, not
        // a nanosecond approximation of it (oracle-verified).
        let (n, d) = recv_time(recv)?.frac();
        if n == num_bigint::BigInt::from(0) {
            return Ok(RubyValue::Int(0));
        }
        crate::builtins::rational::rational_new(n, d)
    }
    def "year" (recv) {
        Ok(RubyValue::Int(civil(recv_time(recv)?).tm.tm_year as i64 + 1900))
    }
    def "month" | "mon" (recv) {
        Ok(RubyValue::Int(civil(recv_time(recv)?).tm.tm_mon as i64 + 1))
    }
    def "day" | "mday" (recv) {
        Ok(RubyValue::Int(civil(recv_time(recv)?).tm.tm_mday as i64))
    }
    def "hour" (recv) {
        Ok(RubyValue::Int(civil(recv_time(recv)?).tm.tm_hour as i64))
    }
    def "min" (recv) {
        Ok(RubyValue::Int(civil(recv_time(recv)?).tm.tm_min as i64))
    }
    def "sec" (recv) {
        Ok(RubyValue::Int(civil(recv_time(recv)?).tm.tm_sec as i64))
    }
    def "wday" (recv) {
        Ok(RubyValue::Int(civil(recv_time(recv)?).tm.tm_wday as i64))
    }
    def "yday" (recv) {
        Ok(RubyValue::Int(civil(recv_time(recv)?).tm.tm_yday as i64 + 1))
    }
    def "utc_offset" | "gmt_offset" | "gmtoff" (recv) {
        Ok(RubyValue::Int(civil(recv_time(recv)?).offset as i64))
    }
    def "zone" (recv) {
        let z = civil(recv_time(recv)?).zone;
        // A fixed-offset (non-UTC) Time has no zone NAME -- nil, not "".
        if z.is_empty() {
            return Ok(RubyValue::Nil);
        }
        Ok(RubyValue::Str(crate::collections::string_new(z)))
    }
    // `tm_isdst` is tri-state in C (>0 in effect, 0 not, <0 unknown); Ruby
    // reports a plain bool, so anything that isn't a positive answer is
    // false -- the same `> 0` test `to_a`/`strftime` already use above.
    def "isdst" | "dst?" (recv) {
        Ok(RubyValue::Bool(civil(recv_time(recv)?).tm.tm_isdst > 0))
    }
    def "utc?" | "gmt?" (recv) {
        Ok(RubyValue::Bool(recv_time(recv)?.is_utc()))
    }
    // The MUTATING converters: they change which zone the receiver RENDERS
    // in and answer self, leaving the instant alone. Callers observe the
    // mutation (`t.utc; t.to_s` renders UTC), which is why `offset` is
    // interior-mutable -- see `RTime`.
    def "utc" | "gmtime" (recv) {
        *recv_time(recv)?.offset.lock() = Some(RTime::UTC);
        Ok(recv.clone())
    }
    def "localtime"(recv, arg?) {
        // No arg -> system-local (offset None); an Integer/String arg fixes it.
        *recv_time(recv)?.offset.lock() = offset_arg(arg)?;
        Ok(recv.clone())
    }
    // ...and their non-mutating counterparts, which answer a fresh Time.
    def "getutc" | "getgm" (recv) {
        let t = recv_time(recv)?;
        Ok(time_value(t.sec(), t.nsec(), Some(RTime::UTC)))
    }
    def "getlocal"(recv, arg?) {
        let t = recv_time(recv)?;
        // No arg -> system-local; an Integer/String arg fixes the utc_offset.
        Ok(time_value(t.sec(), t.nsec(), offset_arg(arg)?))
    }
    def "to_s" (recv) {
        Ok(RubyValue::Str(crate::collections::string_new(render(recv_time(recv)?, false))))
    }
    def "inspect" (recv) {
        Ok(RubyValue::Str(crate::collections::string_new(render(recv_time(recv)?, true))))
    }
    def "strftime" (recv, arg) {
        let __fmt_check = crate::builtins::convert::to_rstr(arg)?;
        let __fmt_check = __fmt_check.lock().to_utf8_lossy().into_owned();
        if incomplete_directive(&__fmt_check) {
            return Err(crate::builtins::arg_error!("invalid format: {__fmt_check}"));
        }
        let f = &crate::builtins::convert::to_rstr(arg)?;
        let fmt = f.lock().to_utf8_lossy().into_owned();
        Ok(RubyValue::Str(crate::collections::string_new(strftime(recv_time(recv)?, &fmt))))
    }
    // `t + n` -> a Time n seconds later; `t - other_time` -> a Float count of
    // seconds BETWEEN them, but `t - n` -> a Time. The argument's type picks.
    def "+" (recv, other) {
        // CRuby's own wording for the one operand that is nearly right:
        // adding two Times is meaningless, and it says so rather than
        // reporting a failed conversion.
        if let RubyValue::Object(o) = other
            && o.as_any().downcast_ref::<RTime>().is_some() {
                return Err(type_error!("time + time?"));
            }
        shift(recv_time(recv)?, other, 1)
    }
    def "-" (recv, other) {
        let t = recv_time(recv)?;
        if let RubyValue::Object(o) = other
            && let Some(other) = o.as_any().downcast_ref::<RTime>() {
                let a = t.sec() as f64 + t.nsec() as f64 / 1e9;
                let b = other.sec() as f64 + other.nsec() as f64 / 1e9;
                return Ok(RubyValue::Float(a - b));
            }
        shift(t, other, -1)
    }
    // Drives Comparable (`<`, `between?`, `clamp`) -- see the module docs.
    def "<=>" (recv, other) {
        let t = recv_time(recv)?;
        let RubyValue::Object(o) = other else {
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
    def "==" | "eql?" (recv, other) {
        let t = recv_time(recv)?;
        if let RubyValue::Object(o) = other
            && let Some(other) = o.as_any().downcast_ref::<RTime>() {
                // The canonical (reduced) fields compare directly -- see
                // `time_exact`.
                return Ok(RubyValue::Bool(t.num == other.num && t.den == other.den));
            }
        Ok(RubyValue::Bool(false))
    }
    def "hash" (recv) {
        let t = recv_time(recv)?;
        // Must agree with `==` above: derived from the canonical instant
        // alone, never from the rendering offset (`t == t.getutc` is true, so
        // they must hash alike).
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        t.num.hash(&mut h);
        t.den.hash(&mut h);
        Ok(RubyValue::Int(h.finish() as i64))
    }
    def "sunday?" (recv) { Ok(RubyValue::Bool(civil(recv_time(recv)?).tm.tm_wday == 0)) }
    def "monday?" (recv) { Ok(RubyValue::Bool(civil(recv_time(recv)?).tm.tm_wday == 1)) }
    def "tuesday?" (recv) { Ok(RubyValue::Bool(civil(recv_time(recv)?).tm.tm_wday == 2)) }
    def "wednesday?" (recv) { Ok(RubyValue::Bool(civil(recv_time(recv)?).tm.tm_wday == 3)) }
    def "thursday?" (recv) { Ok(RubyValue::Bool(civil(recv_time(recv)?).tm.tm_wday == 4)) }
    def "friday?" (recv) { Ok(RubyValue::Bool(civil(recv_time(recv)?).tm.tm_wday == 5)) }
    def "saturday?" (recv) { Ok(RubyValue::Bool(civil(recv_time(recv)?).tm.tm_wday == 6)) }

    // `asctime`/`ctime`: the fixed C `ctime` shape, in the Time's own zone.
    def "asctime" | "ctime" (recv) {
        Ok(RubyValue::Str(crate::collections::string_new(
            strftime(recv_time(recv)?, "%a %b %e %H:%M:%S %Y"),
        )))
    }
    // `[sec, min, hour, mday, mon, year, wday, yday, isdst, zone]`.
    def "to_a" (recv) {
        let c = civil(recv_time(recv)?);
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
    def "to_r" (recv) {
        let t = recv_time(recv)?;
        crate::builtins::rational::rational_new(t.num.clone(), t.den.clone())
    }
    def "round"(recv, ndigits?) {
        time_reduce(recv_time(recv)?, ndigits, Rounding::Round)
    }
    def "floor"(recv, ndigits?) {
        time_reduce(recv_time(recv)?, ndigits, Rounding::Floor)
    }
    def "ceil"(recv, ndigits?) {
        time_reduce(recv_time(recv)?, ndigits, Rounding::Ceil)
    }
    // ISO 8601 / `xmlschema`: `YYYY-MM-DDTHH:MM:SS`, an optional `.fff`
    // fractional part (`fraction_digits`), and the zone (`Z` for UTC else
    // `+HH:MM`).
    def "xmlschema" | "iso8601"(recv, fraction_digits?) {
        let t = recv_time(recv)?;
        let mut s = strftime(t, "%Y-%m-%dT%H:%M:%S");
        let digits = round_ndigits(fraction_digits)?;
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
    def "deconstruct_keys" (recv, arg) {
        let all = time_field_pairs(recv_time(recv)?);
        let pairs: Vec<(RubyValue, RubyValue)> = match arg {
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
                return Err(type_error!("wrong argument type {} (expected Array or nil)",
                        crate::builtins::check_type_name(other)))
            }
        };
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }

    // Every reachable zeo Time is constructed, and CRuby refuses to re-run
    // either hook on one -- the rows ARE that refusal.
    private def "initialize" params "year = nil, mon = nil, mday = nil, hour = nil, min = nil, sec = nil, zone = nil, in: nil, precision: 9" (_recv, *_args, &_block) {
        Err(type_error!("already initialized Time"))
    }
    private def "initialize_copy"(recv, other) {
        // The SOURCE is read first: copying a blank Time is refused for being
        // blank, not for the target being built already. `Time.allocate.dup`
        // says `uninitialized Time` in ruby, and the order is the only thing
        // that decides which message comes out.
        recv_time(other)?;
        // dup/clone's fresh copy already carries the source's payload
        // (`dup_object`), which is what CRuby's init_copy fills there. Only
        // the direct spelling on a built receiver refuses.
        if crate::builtins::kernel::in_copy_hook() {
            return Ok(recv.clone());
        }
        Err(type_error!("already initialized Time"))
    }

    // ---- ruby's private marshal pair. The row answers the raw bytes;
    // CRuby additionally hangs zone/offset/nano ivars on that string, which
    // zeo strings cannot carry -- the marshal writer/reader have a native
    // Time arm serving the FULL wire format, so only a hand-called `_dump`
    // sees the difference.
    private def "_dump" cfunc (recv, *_args) {
        let (bytes, _ivars) = time_mdump(recv_time(recv)?);
        Ok(RubyValue::Str(crate::string_from_bytes(
            bytes,
            crate::encoding::ASCII_8BIT,
        )))
    }
    private def self."_load"(_recv, data) {
        let RubyValue::Str(s) = data else {
            return Err(type_error!(
                "wrong argument type {} (expected String)",
                crate::builtins::check_type_name(data)
            ));
        };
        let bytes = s.lock().bytes().to_vec();
        time_mload(&bytes, &[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn imethod(n: &str) -> crate::builtins::BuiltinMethodFn {
        (crate::builtins::registered_table(zeo_abi::TIME_CLASS)
            .unwrap()
            .instance
            .as_ref()
            .unwrap()
            .lookup)(n)
        .unwrap()
    }
    fn cmethod(n: &str) -> crate::builtins::BuiltinMethodFn {
        (crate::builtins::registered_table(zeo_abi::TIME_CLASS)
            .unwrap()
            .class
            .as_ref()
            .unwrap()
            .lookup)(n)
        .unwrap()
    }
    fn ilookup(n: &str) -> Option<crate::builtins::BuiltinMethodFn> {
        (crate::builtins::registered_table(zeo_abi::TIME_CLASS)
            .unwrap()
            .instance
            .as_ref()
            .unwrap()
            .lookup)(n)
    }

    /// A fixed instant: 2023-11-14 22:13:20 UTC. Every assertion below was
    /// read off `ruby 4.0.6` for this same epoch second.
    const EPOCH: i64 = 1_700_000_000;

    fn utc_at(sec: i64) -> RubyValue {
        time_value(sec, 0, Some(RTime::UTC))
    }

    #[test]
    fn to_i_and_to_f_answer_the_epoch() {
        let t = utc_at(EPOCH);
        assert!(matches!(
            imethod("to_i")(&t, &[], None).unwrap(),
            RubyValue::Int(EPOCH)
        ));
        let RubyValue::Float(f) = imethod("to_f")(&t, &[], None).unwrap() else {
            panic!()
        };
        assert_eq!(f, EPOCH as f64);
    }

    /// The civil fields of a UTC Time -- zone-independent, so this is safe to
    /// assert regardless of the machine's TZ.
    #[test]
    fn utc_civil_fields_match_the_oracle() {
        let t = utc_at(EPOCH);
        let f = |g: crate::builtins::BuiltinMethodFn| {
            let RubyValue::Int(i) = g(&t, &[], None).unwrap() else {
                panic!()
            };
            i
        };
        assert_eq!(f(imethod("year")), 2023);
        assert_eq!(f(imethod("month")), 11);
        assert_eq!(f(imethod("day")), 14);
        assert_eq!(f(imethod("hour")), 22);
        assert_eq!(f(imethod("min")), 13);
        assert_eq!(f(imethod("sec")), 20);
        assert_eq!(f(imethod("wday")), 2, "a Tuesday");
        assert_eq!(f(imethod("yday")), 318);
        assert_eq!(f(imethod("utc_offset")), 0);
    }

    #[test]
    fn utc_renders_with_a_utc_suffix() {
        let t = utc_at(EPOCH);
        assert_eq!(
            imethod("to_s")(&t, &[], None).unwrap().to_display_string(),
            "2023-11-14 22:13:20 UTC"
        );
        assert_eq!(
            imethod("zone")(&t, &[], None).unwrap().to_display_string(),
            "UTC"
        );
        assert!(matches!(
            imethod("utc?")(&t, &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
    }

    #[test]
    fn the_epoch_itself_renders_as_1970() {
        assert_eq!(
            imethod("to_s")(&utc_at(0), &[], None)
                .unwrap()
                .to_display_string(),
            "1970-01-01 00:00:00 UTC"
        );
    }

    /// A fixed-offset Time renders `-0500` and has no zone NAME.
    #[test]
    fn a_fixed_offset_time_renders_its_offset_and_has_no_zone_name() {
        let t = time_value(EPOCH, 0, Some(-5 * 3600));
        assert_eq!(
            imethod("to_s")(&t, &[], None).unwrap().to_display_string(),
            "2023-11-14 17:13:20 -0500"
        );
        assert!(matches!(
            imethod("zone")(&t, &[], None).unwrap(),
            RubyValue::Nil
        ));
        assert!(matches!(
            imethod("utc?")(&t, &[], None).unwrap(),
            RubyValue::Bool(false)
        ));
    }

    /// The strftime directives, against the oracle's own output for this
    /// instant in UTC.
    #[test]
    fn strftime_directives_match_the_oracle() {
        let t = utc_at(EPOCH);
        let f = |fmt: &str| {
            let arg = RubyValue::Str(crate::collections::string_new(fmt.to_string()));
            imethod("strftime")(&t, &[arg], None)
                .unwrap()
                .to_display_string()
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
            imethod("strftime")(&t, &[arg], None)
                .unwrap()
                .to_display_string()
        };
        assert_eq!(f("%-m/%-d"), "11/14");
        assert_eq!(f("%-H"), "22");
        // A single-digit field is where the flags actually differ.
        let jan = utc_at(1_704_067_200); // 2024-01-01 00:00:00 UTC
        let g = |fmt: &str| {
            let arg = RubyValue::Str(crate::collections::string_new(fmt.to_string()));
            imethod("strftime")(&jan, &[arg], None)
                .unwrap()
                .to_display_string()
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
            imethod("strftime")(&t, &[arg], None)
                .unwrap()
                .to_display_string()
        };
        assert_eq!(f("100%%"), "100%");
        assert_eq!(f("%Q"), "%Q");
    }

    /// `t + n` is a Time; `t - other` is a Float of seconds; `t - n` is a Time.
    #[test]
    fn arithmetic_picks_its_answer_from_the_argument() {
        let t = utc_at(EPOCH);
        let later = imethod("+")(&t, &[RubyValue::Int(60)], None).unwrap();
        assert!(matches!(
            imethod("to_i")(&later, &[], None).unwrap(),
            RubyValue::Int(x) if x == EPOCH + 60
        ));

        let RubyValue::Float(d) = imethod("-")(&utc_at(100), &[utc_at(40)], None).unwrap() else {
            panic!("Time - Time is a Float")
        };
        assert_eq!(d, 60.0);

        let earlier = imethod("-")(&t, &[RubyValue::Int(20)], None).unwrap();
        assert!(matches!(
            imethod("to_i")(&earlier, &[], None).unwrap(),
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
        let sum = imethod("+")(&t, &[RubyValue::Float(0.5)], None).unwrap();
        assert!(matches!(
            imethod("to_i")(&sum, &[], None).unwrap(),
            RubyValue::Int(11)
        ));
        assert!(matches!(
            imethod("nsec")(&sum, &[], None).unwrap(),
            RubyValue::Int(300_000_000)
        ));

        // The DOUBLE 10.8 -- whose tail survives into the difference.
        let from_float = cmethod("at")(
            &RubyValue::Class(TIME_CLASS),
            &[RubyValue::Float(10.8)],
            None,
        )
        .unwrap();
        let diff = imethod("-")(&from_float, &[RubyValue::Float(0.9)], None).unwrap();
        assert!(matches!(
            imethod("to_i")(&diff, &[], None).unwrap(),
            RubyValue::Int(9)
        ));
        assert!(matches!(
            imethod("nsec")(&diff, &[], None).unwrap(),
            RubyValue::Int(900_000_000)
        ));
    }

    /// A Float epoch is stored EXACTLY, so `subsec` answers the double's true
    /// fraction (denominator a power of two) rather than a nanosecond
    /// approximation -- `Time.at(10.8).subsec` is
    /// `(225179981368525/281474976710656)`, oracle-verified, and notably NOT
    /// `Rational(8, 10)`.
    #[test]
    fn a_float_epoch_keeps_its_exact_fraction() {
        let t = cmethod("at")(
            &RubyValue::Class(TIME_CLASS),
            &[RubyValue::Float(10.8)],
            None,
        )
        .unwrap();
        assert_eq!(
            imethod("subsec")(&t, &[], None).unwrap().inspect_string(),
            "(225179981368525/281474976710656)"
        );
        // ...while `nsec` is the truncated VIEW of that same fraction.
        assert!(matches!(
            imethod("nsec")(&t, &[], None).unwrap(),
            RubyValue::Int(800_000_000)
        ));
    }

    #[test]
    fn at_accepts_a_float_and_keeps_the_fraction() {
        let t = cmethod("at")(
            &RubyValue::Class(TIME_CLASS),
            &[RubyValue::Float(1_700_000_000.5)],
            None,
        )
        .unwrap();
        let RubyValue::Float(f) = imethod("to_f")(&t, &[], None).unwrap() else {
            panic!()
        };
        assert_eq!(f, 1_700_000_000.5);
        assert!(matches!(
            imethod("nsec")(&t, &[], None).unwrap(),
            RubyValue::Int(500_000_000)
        ));
        assert!(matches!(
            imethod("usec")(&t, &[], None).unwrap(),
            RubyValue::Int(500_000)
        ));
    }

    /// `inspect` shows sub-second digits (trimmed) where `to_s` doesn't.
    #[test]
    fn inspect_shows_trimmed_subseconds_and_to_s_does_not() {
        let t = time_value(EPOCH, 500_000_000, Some(RTime::UTC));
        assert_eq!(
            imethod("inspect")(&t, &[], None)
                .unwrap()
                .to_display_string(),
            "2023-11-14 22:13:20.5 UTC"
        );
        assert_eq!(
            imethod("to_s")(&t, &[], None).unwrap().to_display_string(),
            "2023-11-14 22:13:20 UTC"
        );
    }

    #[test]
    fn utc_constructor_round_trips_through_to_i() {
        let cls = RubyValue::Class(TIME_CLASS);
        let t = cmethod("utc")(
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
        assert!(matches!(
            imethod("to_i")(&t, &[], None).unwrap(),
            RubyValue::Int(EPOCH)
        ));
        assert_eq!(
            imethod("to_s")(&t, &[], None).unwrap().to_display_string(),
            "2023-11-14 22:13:20 UTC"
        );
    }

    /// `<=>` orders by instant and drives Comparable; `==` ignores the
    /// rendering offset (`t == t.getutc`).
    #[test]
    fn comparison_is_by_instant_not_by_offset() {
        let a = utc_at(EPOCH);
        let b = utc_at(EPOCH + 1);
        assert!(matches!(
            imethod("<=>")(&a, std::slice::from_ref(&b), None).unwrap(),
            RubyValue::Int(-1)
        ));
        assert!(matches!(
            imethod("<=>")(&b, std::slice::from_ref(&a), None).unwrap(),
            RubyValue::Int(1)
        ));
        assert!(matches!(
            imethod("<=>")(&a, std::slice::from_ref(&a), None).unwrap(),
            RubyValue::Int(0)
        ));

        let same_instant_other_offset = time_value(EPOCH, 0, Some(-5 * 3600));
        assert!(matches!(
            imethod("==")(&a, std::slice::from_ref(&same_instant_other_offset), None).unwrap(),
            RubyValue::Bool(true)
        ));
        // Equal Times hash equally.
        assert_eq!(
            imethod("hash")(&a, &[], None).unwrap().inspect_string(),
            imethod("hash")(&same_instant_other_offset, &[], None)
                .unwrap()
                .inspect_string()
        );
    }

    /// `<=>` against a non-Time is nil, which is what makes Comparable raise
    /// its "comparison failed" ArgumentError rather than crash.
    #[test]
    fn comparison_against_a_non_time_is_nil() {
        assert!(matches!(
            imethod("<=>")(&utc_at(EPOCH), &[RubyValue::Int(5)], None).unwrap(),
            RubyValue::Nil
        ));
        assert!(matches!(
            imethod("==")(&utc_at(EPOCH), &[RubyValue::Int(5)], None).unwrap(),
            RubyValue::Bool(false)
        ));
    }

    #[test]
    fn getutc_answers_a_utc_copy_without_mutating() {
        let local = time_value(EPOCH, 0, None);
        let u = imethod("getutc")(&local, &[], None).unwrap();
        assert!(matches!(
            imethod("utc?")(&u, &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
        // Same instant.
        assert!(matches!(
            imethod("to_i")(&u, &[], None).unwrap(),
            RubyValue::Int(EPOCH)
        ));
        // The receiver is untouched.
        assert!(matches!(
            imethod("utc?")(&local, &[], None).unwrap(),
            RubyValue::Bool(false)
        ));
    }

    #[test]
    fn weekday_predicates() {
        let t = utc_at(EPOCH); // a Tuesday
        assert!(matches!(
            imethod("tuesday?")(&t, &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
        assert!(matches!(
            imethod("monday?")(&t, &[], None).unwrap(),
            RubyValue::Bool(false)
        ));
        assert!(matches!(
            imethod("sunday?")(&t, &[], None).unwrap(),
            RubyValue::Bool(false)
        ));
    }

    #[test]
    fn now_is_after_the_fixed_epoch_and_is_local() {
        let n = time_now(&RubyValue::Class(TIME_CLASS), &[], None).unwrap();
        let RubyValue::Int(secs) = imethod("to_i")(&n, &[], None).unwrap() else {
            panic!()
        };
        assert!(secs > EPOCH, "clock is before 2023");
        assert!(matches!(
            imethod("utc?")(&n, &[], None).unwrap(),
            RubyValue::Bool(false)
        ));
    }

    #[test]
    fn lookup_tables_find_their_names() {
        assert!(lookup_class("now").is_some());
        assert!(lookup_class("at").is_some());
        assert!(lookup_class("utc").is_some());
        assert!(lookup_class("nope").is_none());
        assert!(ilookup("strftime").is_some());
        assert!(ilookup("to_i").is_some());
        assert!(ilookup("nope").is_none());
    }
}

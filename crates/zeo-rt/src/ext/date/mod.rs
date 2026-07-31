//! `date` (CRuby's bundled `date` gem) -- `Date`, a proleptic-Gregorian
//! calendar date. `require "date"` activates it (a built-in feature; see
//! `ext/mod.rs`).
//!
//! Backed by an `RObj` holding a **Julian Day Number** (JDN), which makes day
//! arithmetic (`+`/`-`), ordering, and `wday` trivial and exact. Gregorian
//! `(year, month, day)` fields are derived on demand via the standard
//! JDN<->civil formulas. Implemented methods are oracle-verified against ruby
//! 4.0.6.
//!
//! `DateTime` shares this table; its time-of-day fields default to midnight
//! (a documented partial: the calendar half is complete, sub-day fields are
//! not modelled here).

mod parse;

use crate::builtins::{arg_error, arity, type_error};
use crate::dispatch::{RObj, RubyObject};
use crate::{ClassId, RubyValue, Signal, string_new};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zeo_abi::{DATE_CLASS, DATETIME_CLASS};
use zeo_macros::ruby_class;

pub struct RDate {
    jdn: i64,
    class_id: ClassId,
    frozen: AtomicBool,
}

impl RDate {
    fn new(jdn: i64, class_id: ClassId) -> Arc<RDate> {
        Arc::new(RDate {
            jdn,
            class_id,
            frozen: AtomicBool::new(false),
        })
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
        let d = RDate::new(self.jdn, self.class_id);
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
fn civil_to_jdn(y: i64, m: i64, d: i64) -> i64 {
    let a = (14 - m) / 12;
    let y2 = y + 4800 - a;
    let m2 = m + 12 * a - 3;
    d + (153 * m2 + 2) / 5 + 365 * y2 + y2 / 4 - y2 / 100 + y2 / 400 - 32045
}

/// The proleptic-Gregorian `(year, month, day)` of a JDN.
fn jdn_to_civil(jdn: i64) -> (i64, i64, i64) {
    let a = jdn + 32044;
    let b = (4 * a + 3) / 146097;
    let c = a - (146097 * b) / 4;
    let d2 = (4 * c + 3) / 1461;
    let e = c - (1461 * d2) / 4;
    let m2 = (5 * e + 2) / 153;
    let day = e - (153 * m2 + 2) / 5 + 1;
    let month = m2 + 3 - 12 * (m2 / 10);
    let year = 100 * b + d2 - 4800 + m2 / 10;
    (year, month, day)
}

/// `Date#wday`: 0 = Sunday .. 6 = Saturday. JDN 0 is a Monday, so `+1` aligns
/// the modulo to Sunday-origin.
fn jdn_wday(jdn: i64) -> i64 {
    (jdn + 1).rem_euclid(7)
}

/// Day-of-year (1-based) for a JDN.
fn jdn_yday(jdn: i64) -> i64 {
    let (y, _, _) = jdn_to_civil(jdn);
    jdn - civil_to_jdn(y, 1, 1) + 1
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

/// A numeric argument's integer value (`Int`, or a truncated `Float`), for the
/// day-count arguments of `+`/`-`.
fn day_count(v: &RubyValue) -> Option<i64> {
    match v {
        RubyValue::Int(i) => Some(*i),
        RubyValue::Float(f) => Some(*f as i64),
        _ => None,
    }
}

/// `strftime` over a Date's fields (time-of-day directives render as zero).
fn date_strftime(jdn: i64, fmt: &str) -> String {
    let (y, m, d) = jdn_to_civil(jdn);
    let wday = jdn_wday(jdn);
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        let mut dash = false;
        while let Some(&f) = chars.peek() {
            match f {
                '-' | '0' | '_' => {
                    dash = f == '-';
                    chars.next();
                }
                _ => break,
            }
        }
        let Some(dir) = chars.next() else {
            out.push('%');
            break;
        };
        let num = |v: i64, width: usize| {
            if dash {
                v.to_string()
            } else {
                format!("{v:0width$}")
            }
        };
        match dir {
            'Y' => out.push_str(&y.to_string()),
            'y' => out.push_str(&num(y.rem_euclid(100), 2)),
            'C' => out.push_str(&num(y / 100, 2)),
            'm' => out.push_str(&num(m, 2)),
            'd' => out.push_str(&num(d, 2)),
            'e' => out.push_str(&format!("{d:>2}")),
            'j' => out.push_str(&num(jdn_yday(jdn), 3)),
            'A' => out.push_str(DAY_NAMES[wday as usize]),
            'a' => out.push_str(&DAY_NAMES[wday as usize][..3]),
            'B' => out.push_str(MONTH_NAMES[(m - 1) as usize]),
            'b' | 'h' => out.push_str(&MONTH_NAMES[(m - 1) as usize][..3]),
            'w' => out.push_str(&wday.to_string()),
            'u' => out.push_str(&(if wday == 0 { 7 } else { wday }).to_string()),
            'H' | 'M' | 'S' => out.push_str("00"),
            'F' => out.push_str(&format!("{y:04}-{m:02}-{d:02}")),
            '%' => out.push('%'),
            other => {
                out.push('%');
                out.push(other);
            }
        }
    }
    out
}

fn iso_string(jdn: i64) -> String {
    let (y, m, d) = jdn_to_civil(jdn);
    format!("{y:04}-{m:02}-{d:02}")
}

ruby_class! {
    Date = zeo_abi::DATE_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::COMPARABLE_CLASS;

    def "year" (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(jdn_to_civil(date_of(recv).jdn).0))
    }
    def "month" | "mon" (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(jdn_to_civil(date_of(recv).jdn).1))
    }
    def "day" | "mday" (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(jdn_to_civil(date_of(recv).jdn).2))
    }
    def "wday" (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(jdn_wday(date_of(recv).jdn)))
    }
    def "yday" (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(jdn_yday(date_of(recv).jdn)))
    }
    def "jd" (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(date_of(recv).jdn))
    }
    def "leap?" (recv, args, _block) {
        arity!(args, 0);
        let y = jdn_to_civil(date_of(recv).jdn).0;
        Ok(RubyValue::Bool(y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)))
    }
    def "to_s" | "iso8601" (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(string_new(iso_string(date_of(recv).jdn))))
    }
    def "inspect" (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(string_new(format!("#<Date: {}>", iso_string(date_of(recv).jdn)))))
    }
    def "strftime" (recv, args, _block) {
        arity!(args, 1);
        let fmt = &crate::builtins::convert::to_rstr(&args[0])?;
        let fmt = fmt.lock().to_utf8_lossy().into_owned();
        Ok(RubyValue::Str(string_new(date_strftime(date_of(recv).jdn, &fmt))))
    }
    // `date + n` advances by n days; `next_day`/`prev_day` are the named forms.
    def "+" | "next_day" (recv, args, _block) {
        let d = date_of(recv);
        let n = match args.first() {
            None => 1,
            Some(v) => day_count(v).ok_or_else(|| type_error!("expected numeric"))?,
        };
        Ok(RubyValue::Object(RDate::new(d.jdn + n, d.class_id)))
    }
    def "prev_day" (recv, args, _block) {
        arity!(args, 0..=1);
        let d = date_of(recv);
        let n = args.first().and_then(day_count).unwrap_or(1);
        Ok(RubyValue::Object(RDate::new(d.jdn - n, d.class_id)))
    }
    // `date - n` -> a Date n days earlier; `date - other_date` -> the Rational
    // day difference (CRuby's result type).
    def "-" (recv, args, _block) {
        arity!(args, 1);
        let d = date_of(recv);
        if let RubyValue::Object(o) = &args[0] {
            if let Some(other) = o.as_any().downcast_ref::<RDate>() {
                return crate::builtins::rational::rational_new(
                    num_bigint::BigInt::from(d.jdn - other.jdn),
                    num_bigint::BigInt::from(1),
                );
            }
        }
        let n = day_count(&args[0]).ok_or_else(|| type_error!("expected numeric or date"))?;
        Ok(RubyValue::Object(RDate::new(d.jdn - n, d.class_id)))
    }
    def "next" | "succ" (recv, args, _block) {
        arity!(args, 0);
        let d = date_of(recv);
        Ok(RubyValue::Object(RDate::new(d.jdn + 1, d.class_id)))
    }
    def "<=>" (recv, args, _block) {
        arity!(args, 1);
        let a = date_of(recv).jdn;
        let b = match &args[0] {
            RubyValue::Object(o) => match o.as_any().downcast_ref::<RDate>() {
                Some(other) => other.jdn,
                None => return Ok(RubyValue::Nil),
            },
            _ => return Ok(RubyValue::Nil),
        };
        Ok(RubyValue::Int((a.cmp(&b) as i64).signum()))
    }
    def "==" (recv, args, _block) {
        arity!(args, 1);
        let a = date_of(recv).jdn;
        let equal = matches!(&args[0], RubyValue::Object(o)
            if o.as_any().downcast_ref::<RDate>().is_some_and(|d| d.jdn == a));
        Ok(RubyValue::Bool(equal))
    }

    def self."new" | "civil" (recv, args, _block) {
        arity!(args, 0..=3);
        let (y, m, d) = civil_args(args)?;
        Ok(RubyValue::Object(RDate::new(civil_to_jdn(y, m, d), class_of(recv))))
    }
    def self."jd" (recv, args, _block) {
        arity!(args, 0..=1);
        let jdn = match args.first() {
            None => 0,
            Some(RubyValue::Int(n)) => *n,
            Some(RubyValue::Float(f)) => f.trunc() as i64,
            Some(_) => return Err(type_error!("invalid jd (not numeric)")),
        };
        Ok(RubyValue::Object(RDate::new(jdn, class_of(recv))))
    }
    def self."today" (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Object(RDate::new(today_jdn(), class_of(recv))))
    }
    def self."parse" (recv, args, _block) {
        arity!(args, 1..=3);
        let s = &crate::builtins::convert::to_rstr(&args[0])?;
        let text = s.lock().to_utf8_lossy().into_owned();
        let (y, m, d) = parse_date(&text)?;
        Ok(RubyValue::Object(RDate::new(civil_to_jdn(y, m, d), class_of(recv))))
    }
    // The heuristic scanner itself, which `Date.parse` and the `time` gem's
    // `Time.parse` both read their fields out of. See `parse.rs`.
    def self."_parse" (_recv, args, _block) {
        arity!(args, 1..=2);
        let s = &crate::builtins::convert::to_rstr(&args[0])?;
        let text = s.lock().to_utf8_lossy().into_owned();
        let comp = args.get(1).is_none_or(RubyValue::truthy);
        Ok(RubyValue::Hash(crate::collections::hash_new(parse::date_parse(&text, comp))))
    }
    def self."valid_date?" | "valid_civil?" (_recv, args, _block) {
        arity!(args, 3..=4);
        // A non-numeric component answers false rather than raising
        // (oracle: `Date.valid_date?(2020, nil, 1)` is false).
        let Ok((y, m, d)) = civil_args(args) else {
            return Ok(RubyValue::Bool(false));
        };
        // Round-trips only for a real calendar date.
        let jdn = civil_to_jdn(y, m, d);
        Ok(RubyValue::Bool(jdn_to_civil(jdn) == (y, m, d)))
    }
    def self."leap?" (_recv, args, _block) {
        arity!(args, 1);
        let y = &match &args[0] {
            RubyValue::Int(n) => *n,
            RubyValue::Float(f) => f.trunc() as i64,
            _ => return Err(type_error!("invalid year (not numeric)")),
        };
        Ok(RubyValue::Bool(y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)))
    }
}

/// `Date.new`'s civil components, defaulting like CRuby (`Date.new` == the
/// Julian-calendar epoch, but the common call passes all three).
fn civil_args(args: &[RubyValue]) -> Result<(i64, i64, i64), Signal> {
    // NOT the generic implicit-conversion protocol: the date library takes
    // any numeric (a Float truncates) and rejects the rest with its own
    // "invalid year (not numeric)" TypeError shape (oracle-verified; even
    // `to_int` ducks are rejected).
    let int_at = |i: usize, default: i64, name: &str| -> Result<i64, Signal> {
        match args.get(i) {
            None => Ok(default),
            Some(RubyValue::Int(v)) => Ok(*v),
            Some(RubyValue::Float(f)) => Ok(f.trunc() as i64),
            Some(_) => Err(type_error!("invalid {name} (not numeric)")),
        }
    };
    Ok((
        int_at(0, -4712, "year")?,
        int_at(1, 1, "month")?,
        int_at(2, 1, "day")?,
    ))
}

/// The class a `Date` class method should build (`Date` or a `DateTime`).
fn class_of(recv: &RubyValue) -> ClassId {
    match recv {
        RubyValue::Class(DATETIME_CLASS) => DATETIME_CLASS,
        _ => DATE_CLASS,
    }
}

/// The calendar date `Date.parse` builds out of [`parse::date_parse`]'s fields.
///
/// A component ABOVE the most significant one the string gave comes from today,
/// and one below it takes its minimum -- CRuby's completion rule, and the reason
/// `Date.parse("Aug 31")` lands in the current year while `Date.parse("Aug
/// 2000")` lands on the first of the month. A string with no date in it at all,
/// or one whose fields name no real day, is an ArgumentError. (CRuby raises
/// `Date::Error` there, a subclass of it that zeo does not model.)
fn parse_date(text: &str) -> Result<(i64, i64, i64), Signal> {
    let pairs = parse::date_parse(text, true);
    let field = |key: &str| parse::field(&pairs, key);
    // The ISO WEEK date names a different calendar, and an ordinal date counts
    // days rather than naming a month; both convert to a JDN directly.
    if let (Some(cwyear), Some(cweek)) = (field("cwyear"), field("cweek")) {
        let jan4 = civil_to_jdn(cwyear, 1, 4);
        // JDN 0 is a Monday, so a JDN divisible by 7 is one too -- and week 1 is
        // the week holding January 4th.
        let monday = jan4 - jan4.rem_euclid(7);
        return Ok(jdn_to_civil(
            monday + (cweek - 1) * 7 + field("cwday").unwrap_or(1) - 1,
        ));
    }
    if let (Some(y), Some(yday)) = (field("year"), field("yday")) {
        return Ok(jdn_to_civil(civil_to_jdn(y, 1, 1) + yday - 1));
    }
    let (this_year, this_month, _) = jdn_to_civil(today_jdn());
    let (y, m, d) = match (field("year"), field("mon"), field("mday")) {
        (Some(y), mon, mday) => (y, mon.unwrap_or(1), mday.unwrap_or(1)),
        (None, Some(mon), mday) => (this_year, mon, mday.unwrap_or(1)),
        (None, None, Some(mday)) => (this_year, this_month, mday),
        (None, None, None) => return Err(arg_error!("invalid date")),
    };
    if jdn_to_civil(civil_to_jdn(y, m, d)) != (y, m, d) {
        return Err(arg_error!("invalid date"));
    }
    Ok((y, m, d))
}

/// The current UTC calendar day, as a JDN (a documented divergence from CRuby's
/// LOCAL date near midnight in a non-UTC zone); untestable deterministically.
fn today_jdn() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    2_440_588 + secs.div_euclid(86_400)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i64, m: i64, d: i64) -> RubyValue {
        RubyValue::Object(RDate::new(civil_to_jdn(y, m, d), DATE_CLASS))
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

    #[test]
    fn civil_jdn_round_trips() {
        for (y, m, d) in [(2000, 1, 1), (1970, 1, 1), (2024, 2, 29), (1, 1, 1)] {
            assert_eq!(jdn_to_civil(civil_to_jdn(y, m, d)), (y, m, d));
        }
    }

    #[test]
    fn fields_match_ruby() {
        let dt = date(2024, 3, 15); // a Friday
        assert_eq!(int(&im("year")(&dt, &[], None).unwrap()), 2024);
        assert_eq!(int(&im("month")(&dt, &[], None).unwrap()), 3);
        assert_eq!(int(&im("day")(&dt, &[], None).unwrap()), 15);
        assert_eq!(int(&im("wday")(&dt, &[], None).unwrap()), 5);
        assert_eq!(s(&im("to_s")(&dt, &[], None).unwrap()), "2024-03-15");
    }

    #[test]
    fn arithmetic_advances_and_differences() {
        let a = date(2024, 1, 1);
        let plus10 = im("+")(&a, &[RubyValue::Int(10)], None).unwrap();
        assert_eq!(s(&im("to_s")(&plus10, &[], None).unwrap()), "2024-01-11");
        // date - date -> Rational difference.
        let diff = im("-")(&plus10, std::slice::from_ref(&a), None).unwrap();
        assert!(matches!(diff, RubyValue::Rational(_)));
    }

    #[test]
    fn strftime_covers_common_directives() {
        let dt = date(2024, 3, 15);
        assert_eq!(
            s(&im("strftime")(
                &dt,
                &[RubyValue::Str(string_new("%Y-%m-%d (%A)".into()))],
                None
            )
            .unwrap()),
            "2024-03-15 (Friday)"
        );
        assert_eq!(
            s(&im("strftime")(
                &dt,
                &[RubyValue::Str(string_new("%b %-d, %Y".into()))],
                None
            )
            .unwrap()),
            "Mar 15, 2024"
        );
    }
}

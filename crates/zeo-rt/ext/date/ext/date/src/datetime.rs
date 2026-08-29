//! `DateTime` -- a `Date` that carries a time of day and a UTC offset.
//!
//! The backing object is the same [`RDate`](super::RDate); this table is the
//! surface CRuby publishes only on the subclass: the clock accessors (private
//! on `Date`), the offset pair, and the formats that render a time.

use super::{
    NS_PER_SEC, RDate, class_of, date_error, date_of, fmt_arg, jd_wday, jd_yday,
    ns_to_day_rational, numeric_at, offset_string, start_arg, str_val,
};
use crate::builtins::type_error;
use crate::{RubyValue, Signal, string_new};
use num_bigint::BigInt;
use zeo_macros::ruby_class;

/// The `offset` argument `DateTime.new`/`new_offset` take: a `"+09:00"`-style
/// string, or a Rational/Float FRACTION OF A DAY (CRuby's own unit).
fn offset_arg(v: Option<&RubyValue>) -> Result<i32, Signal> {
    match v {
        None | Some(RubyValue::Nil) => Ok(0),
        Some(RubyValue::Str(s)) => {
            let text = s.lock().to_utf8_lossy().into_owned();
            super::parse::numeric_offset(&text)
                .or_else(|| super::parse::named_offset(&text.to_lowercase()))
                .map(|o| o as i32)
                .ok_or_else(|| date_error("invalid date"))
        }
        Some(RubyValue::Int(n)) => Ok((*n * 86_400) as i32),
        Some(RubyValue::Float(f)) => Ok((f * 86_400.0).round() as i32),
        Some(RubyValue::Rational(r)) => {
            let num = i64::try_from(&r.num).unwrap_or(0);
            let den = i64::try_from(&r.den).unwrap_or(1);
            Ok((num * 86_400 / den.max(1)) as i32)
        }
        Some(_) => Err(type_error!("expected numeric")),
    }
}

/// `iso8601`/`rfc3339`/`jisx0301`'s optional digit count: the seconds get a
/// `.nnn` tail of `n` places (`date_core.c:iso8601_timediv`).
fn timediv(n: i64) -> String {
    if n <= 0 {
        "%H:%M:%S".to_string()
    } else {
        format!("%H:%M:%S.%{n}N")
    }
}

fn digits(arg: Option<&RubyValue>) -> Result<i64, Signal> {
    match arg {
        None => Ok(0),
        Some(v) => match crate::builtins::convert::to_int(v)? {
            RubyValue::Int(n) => Ok(n),
            _ => Ok(0),
        },
    }
}

ruby_class! {
    DateTime = zeo_abi::DATETIME_CLASS < zeo_abi::DATE_CLASS;

    allocate super::datetime_allocate;

    def "to_s" (recv) { str_val(date_of(recv), "%Y-%m-%dT%H:%M:%S%:z") }
    def "strftime" cfunc (recv, arg?) {
        let fmt = match arg { Some(v) => fmt_arg(v)?, None => "%Y-%m-%dT%H:%M:%S%:z".to_string() };
        str_val(date_of(recv), &fmt)
    }
    def "iso8601" | "xmlschema" (recv, arg?) {
        str_val(date_of(recv), &format!("%Y-%m-%dT{}%:z", timediv(digits(arg)?)))
    }
    def "rfc3339" (recv, arg?) {
        str_val(date_of(recv), &format!("%Y-%m-%dT{}%:z", timediv(digits(arg)?)))
    }
    def "jisx0301" (recv, arg?) {
        let d = date_of(recv);
        let head = super::jisx0301(d);
        let tail = str_val(d, &format!("T{}%:z", timediv(digits(arg)?)))?;
        Ok(RubyValue::Str(string_new(format!("{head}{}", tail.to_display_string()))))
    }

    def "hour" (recv) { Ok(RubyValue::Int(date_of(recv).hms().0 as i64)) }
    def "min" | "minute" (recv) { Ok(RubyValue::Int(date_of(recv).hms().1 as i64)) }
    def "sec" | "second" (recv) { Ok(RubyValue::Int(date_of(recv).hms().2 as i64)) }
    def "sec_fraction" | "second_fraction" (recv) {
        crate::builtins::rational::rational_new(
            BigInt::from(date_of(recv).sf()), BigInt::from(NS_PER_SEC))
    }
    def "offset" (recv) {
        crate::builtins::rational::rational_new(
            BigInt::from(date_of(recv).of()), BigInt::from(86_400))
    }
    def "zone" (recv) { Ok(RubyValue::Str(string_new(offset_string(date_of(recv).of())))) }
    // The same INSTANT rendered against another offset: `jd`/`df` are UTC, so
    // only `of` moves.
    def "new_offset" (recv, arg?) {
        let d = date_of(recv);
        let of = offset_arg(arg)?;
        Ok(RubyValue::Object(RDate::raw(d.jd(), d.df(), d.sf(), of, d.sg(), true, d.class_id)))
    }
    def "to_time" (recv) {
        let d = date_of(recv);
        let (y, m, day) = d.civil();
        let (h, mi, s) = d.hms();
        crate::dispatch::send_value(
            &RubyValue::Class(zeo_abi::TIME_CLASS),
            crate::Symbol::intern("new"),
            &[
                RubyValue::Int(y), RubyValue::Int(m), RubyValue::Int(day),
                RubyValue::Int(h as i64), RubyValue::Int(mi as i64), RubyValue::Int(s as i64),
                RubyValue::Str(string_new(offset_string(d.of()))),
            ],
            None,
        )
    }
    def "deconstruct_keys" (recv, _keys) {
        let d = date_of(recv);
        let (y, m, day) = d.civil();
        let (h, mi, s) = d.hms();
        let local_jd = d.local_jd();
        let sym = |n: &str| RubyValue::Symbol(crate::Symbol::intern(n));
        Ok(RubyValue::Hash(crate::collections::hash_new(vec![
            (sym("year"), RubyValue::Int(y)),
            (sym("month"), RubyValue::Int(m)),
            (sym("day"), RubyValue::Int(day)),
            (sym("yday"), RubyValue::Int(jd_yday(local_jd, d.sg()))),
            (sym("wday"), RubyValue::Int(jd_wday(local_jd))),
            (sym("hour"), RubyValue::Int(h as i64)),
            (sym("min"), RubyValue::Int(mi as i64)),
            (sym("sec"), RubyValue::Int(s as i64)),
            (sym("sec_fraction"), ns_to_day_rational(d.sf() as i128)?),
            (sym("zone"), RubyValue::Str(string_new(offset_string(d.of())))),
        ])))
    }

    def self."new" | "civil" (recv, _year?, _month?, _mday?, _hour?, _min?, _sec?, _offset?, _start?) {
        let sg = start_arg(__args, 7)?;
        let y = numeric_at(__args, 0, -4712, "year")?;
        let m = numeric_at(__args, 1, 1, "month")?;
        let d = numeric_at(__args, 2, 1, "day")?;
        let h = numeric_at(__args, 3, 0, "hour")?;
        let mi = numeric_at(__args, 4, 0, "min")?;
        let of = offset_arg(__args.get(6))?;
        // A fractional second is exact: `DateTime.new(...,5.5)` keeps 5e8 ns.
        let (s, ns) = match __args.get(5) {
            Some(RubyValue::Float(f)) => (f.trunc() as i64, ((f.fract()) * 1e9).round() as i32),
            other => (numeric_at(std::slice::from_ref(&other.cloned().unwrap_or(RubyValue::Int(0))), 0, 0, "sec")?, 0),
        };
        Ok(RubyValue::Object(datetime_of(recv, y, m, d, h, mi, s, ns, of, sg)?))
    }
    def self."jd" (recv, _jd?, _hour?, _min?, _sec?, _offset?, _start?) {
        let sg = start_arg(__args, 5)?;
        let jd = numeric_at(__args, 0, 0, "jd")?;
        let h = numeric_at(__args, 1, 0, "hour")?;
        let mi = numeric_at(__args, 2, 0, "min")?;
        let s = numeric_at(__args, 3, 0, "sec")?;
        let of = offset_arg(__args.get(4))?;
        Ok(RubyValue::Object(at_jd(recv, jd, h, mi, s, 0, of, sg)?))
    }
    def self."now" cfunc (recv, _start?) {
        let sg = start_arg(__args, 0)?;
        let (secs, nsec) = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => (d.as_secs() as i64, d.subsec_nanos() as i32),
            Err(_) => (0, 0),
        };
        let jd = super::EPOCH_JD + secs.div_euclid(86_400);
        let df = secs.rem_euclid(86_400) as i32;
        Ok(RubyValue::Object(RDate::raw(jd, df, nsec, 0, sg, true, class_of(recv))))
    }
}

/// A complex date from LOCAL civil fields plus an offset.
#[allow(clippy::too_many_arguments)]
fn datetime_of(
    recv: &RubyValue,
    y: i64,
    m: i64,
    d: i64,
    h: i64,
    mi: i64,
    s: i64,
    ns: i32,
    of: i32,
    sg: f64,
) -> Result<std::sync::Arc<RDate>, Signal> {
    let jd = super::civil_checked(y, m, d, sg)?;
    at_jd(recv, jd, h, mi, s, ns, of, sg)
}

#[allow(clippy::too_many_arguments)]
fn at_jd(
    recv: &RubyValue,
    jd: i64,
    h: i64,
    mi: i64,
    s: i64,
    ns: i32,
    of: i32,
    sg: f64,
) -> Result<std::sync::Arc<RDate>, Signal> {
    let h = if h < 0 { 24 + h } else { h };
    let mi = if mi < 0 { 60 + mi } else { mi };
    let s = if s < 0 { 60 + s } else { s };
    if !(0..=24).contains(&h) || !(0..=59).contains(&mi) || !(0..=59).contains(&s) {
        return Err(date_error("invalid date"));
    }
    // Stored as UTC: back the local clock out by the offset.
    let utc = h * 3600 + mi * 60 + s - of as i64;
    Ok(RDate::raw(
        jd + utc.div_euclid(86_400),
        utc.rem_euclid(86_400) as i32,
        ns,
        of,
        sg,
        true,
        class_of(recv),
    ))
}

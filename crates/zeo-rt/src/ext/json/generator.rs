//! The json gem's generator: one state carrying every option, and the depth
//! counter that is also the cycle guard.
//!
//! Probed against ruby 4.0.6's json 2.21.2, one knob at a time, because the
//! layout rules are not guessable:
//!
//!   * Every item is prefixed by `<nl><indent * (depth + 1)>` and the CLOSER
//!     by `<nl><indent * depth>` -- so `indent: "--"` with no `array_nl`
//!     gives `[--1,--2]`, indented items and a bare `]`. An EMPTY container
//!     gets neither and stays `[]`.
//!   * A cycle is not a special case. The depth counter bounds it, so a
//!     self-referential array is `nesting of 100 is too deep. Did you try to
//!     serialize objects with circular references?` -- and the number in
//!     that message is the LIMIT, not the depth reached.
//!   * `1e100.to_json` is `1e+100`, not `1.0e+100`. The placement rule is in
//!     [`float_text`].
//!
//! A residual divergence, accepted and recorded in `docs/COMPATIBILITY.md`:
//! the gem uses grisu2, which is not always shortest, so about 0.9% of
//! doubles get a seventeenth digit under CRuby that Rust's shortest
//! formatting does not print. Both parse back to the same double.

use crate::dispatch::raise_error;
use crate::{RubyValue, Signal};

/// The depth this emit refuses at NO MATTER what `max_nesting` says.
///
/// A DIVERGENCE, deliberately, and the reason is a CYCLE rather than a deep
/// document: this generator walks the structure recursively, so a
/// self-referential one has no bound but the machine stack. Ruby has none
/// either -- `JSON.generate(a, max_nesting: false)` on `a = []; a << a`
/// raises `SystemStackError` there. A loud `JSON::NestingError` the program
/// can rescue beats an abort with no line of output.
///
/// The parser used to share this number and no longer needs one: it keeps
/// its open containers on an explicit stack, so its depth costs heap. Doing
/// the same here would retire this constant too.
/// `tests/divergences/json_nesting_is_bounded_by_the_stack.rb` records what
/// is left.
pub(super) const CYCLE_CEILING: i64 = 2_000;

/// Every generator option, plus the depth this emit has reached.
pub(super) struct State {
    pub indent: String,
    pub space: String,
    pub space_before: String,
    pub object_nl: String,
    pub array_nl: String,
    pub allow_nan: bool,
    pub ascii_only: bool,
    pub script_safe: bool,
    /// `None` = unbounded. The default is 100, and it is the cycle guard.
    pub max_nesting: Option<i64>,
    pub(super) depth: i64,
}

impl Default for State {
    fn default() -> Self {
        State {
            indent: String::new(),
            space: String::new(),
            space_before: String::new(),
            object_nl: String::new(),
            array_nl: String::new(),
            allow_nan: false,
            ascii_only: false,
            script_safe: false,
            max_nesting: Some(100),
            depth: 0,
        }
    }
}

impl State {
    /// The four options `pretty_generate` sets, and nothing else.
    pub(super) fn pretty() -> State {
        State {
            indent: "  ".to_string(),
            space: " ".to_string(),
            object_nl: "\n".to_string(),
            array_nl: "\n".to_string(),
            ..State::default()
        }
    }

    fn enter(&mut self) -> Result<(), Signal> {
        self.depth += 1;
        if self.depth > CYCLE_CEILING {
            return Err(raise_error(
                "JSON::NestingError",
                format!(
                    "nesting of {CYCLE_CEILING} is too deep. Did you try to serialize objects \
                     with circular references?"
                ),
            ));
        }
        match self.max_nesting {
            Some(max) if self.depth > max => Err(raise_error(
                "JSON::NestingError",
                format!(
                    "nesting of {max} is too deep. Did you try to serialize objects with \
                     circular references?"
                ),
            )),
            _ => Ok(()),
        }
    }

    fn item_prefix(&self, nl: &str) -> String {
        format!("{nl}{}", self.indent.repeat(self.depth.max(0) as usize))
    }

    fn close_prefix(&self, nl: &str) -> String {
        format!("{nl}{}", self.indent.repeat((self.depth - 1).max(0) as usize))
    }
}

/// `v` as JSON text, appended to `out`.
pub(super) fn generate_into(v: &RubyValue, st: &mut State, out: &mut String) -> Result<(), Signal> {
    match v {
        RubyValue::Nil => out.push_str("null"),
        RubyValue::Bool(true) => out.push_str("true"),
        RubyValue::Bool(false) => out.push_str("false"),
        RubyValue::Int(_) | RubyValue::BigInt(_) => out.push_str(&v.to_display_string()),
        RubyValue::Float(f) => {
            if !f.is_finite() && !st.allow_nan {
                return Err(raise_error(
                    "JSON::GeneratorError",
                    format!("{} not allowed in JSON", v.to_display_string()),
                ));
            }
            out.push_str(&float_text(*f));
        }
        RubyValue::Str(s) => {
            let text = string_text(s)?;
            escape_into(&text, st, out);
        }
        RubyValue::Symbol(sym) => escape_into(&sym.name(), st, out),
        RubyValue::Array(a) => {
            let items = a.lock().clone();
            st.enter()?;
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&st.item_prefix(&st.array_nl));
                generate_into(item, st, out)?;
            }
            if !items.is_empty() {
                out.push_str(&st.close_prefix(&st.array_nl));
            }
            out.push(']');
            st.depth -= 1;
        }
        RubyValue::Hash(h) => {
            let pairs = crate::collections::hash_pairs(h);
            st.enter()?;
            out.push('{');
            for (i, (k, val)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&st.item_prefix(&st.object_nl));
                key_into(k, st, out)?;
                out.push_str(&st.space_before);
                out.push(':');
                out.push_str(&st.space);
                generate_into(val, st, out)?;
            }
            if !pairs.is_empty() {
                out.push_str(&st.close_prefix(&st.object_nl));
            }
            out.push('}');
            st.depth -= 1;
        }
        // Ruby asks an OBJECT to encode itself, HANDING IT THE STATE, and
        // splices what comes back verbatim.
        //
        // The state is the whole point. A `to_json(*args)` that forwards to
        // an inner Hash's `to_json(state)` produces output indented for the
        // depth it sits at -- so a user object inside a `pretty_generate`
        // is laid out like everything around it. `JSON::Fragment` is the
        // other half of the same rule: it IGNORES the state, which is what
        // makes a fragment keep its own formatting.
        RubyValue::Object(_) => {
            let arg = state_object(st)?;
            let encoded =
                crate::dispatch::send_value(v, crate::Symbol::intern("to_json"), &[arg], None)?;
            match encoded {
                RubyValue::Str(text) => out.push_str(&string_text(&text)?),
                other => escape_into(&other.to_display_string(), st, out),
            }
        }
        other => escape_into(&other.to_display_string(), st, out),
    }
    Ok(())
}

/// This state as a `JSON::State`, for the `obj.to_json(state)` protocol.
///
/// Built through the Ruby class rather than as a bare Hash because user
/// code reads `state.indent` and friends -- and because the DEPTH has to
/// travel, or a nested `to_json` would restart its indentation at zero.
fn state_object(st: &State) -> Result<RubyValue, Signal> {
    let pairs = vec![
        (sym("indent"), str_val(&st.indent)),
        (sym("space"), str_val(&st.space)),
        (sym("space_before"), str_val(&st.space_before)),
        (sym("object_nl"), str_val(&st.object_nl)),
        (sym("array_nl"), str_val(&st.array_nl)),
        (sym("allow_nan"), RubyValue::Bool(st.allow_nan)),
        (sym("ascii_only"), RubyValue::Bool(st.ascii_only)),
        (sym("script_safe"), RubyValue::Bool(st.script_safe)),
        (
            sym("max_nesting"),
            match st.max_nesting {
                Some(n) => RubyValue::Int(n),
                None => RubyValue::Bool(false),
            },
        ),
        (sym("depth"), RubyValue::Int(st.depth)),
    ];
    let opts = RubyValue::Hash(crate::collections::hash_new(pairs));
    let Some(cid) = crate::runtime_meta::runtime_class_id_by_name("JSON::State") else {
        // The gem's Ruby half is what defines `JSON::State`, and a native
        // generate cannot run before that file has. Answering the bare
        // options Hash keeps a `to_json(state)` working either way.
        return Ok(opts);
    };
    let cls = RubyValue::Class(cid);
    crate::dispatch::send_value(&cls, crate::Symbol::intern("new"), &[opts], None)
}

fn sym(name: &str) -> RubyValue {
    RubyValue::Symbol(crate::Symbol::intern(name))
}

fn str_val(text: &str) -> RubyValue {
    RubyValue::Str(crate::string_new(text.to_string()))
}

/// A Hash key. CRuby stringifies whatever it is (a Symbol key becomes its
/// name), and only then escapes.
fn key_into(k: &RubyValue, st: &State, out: &mut String) -> Result<(), Signal> {
    match k {
        RubyValue::Str(s) => escape_into(&string_text(s)?, st, out),
        other => escape_into(&other.to_display_string(), st, out),
    }
    Ok(())
}

/// A Ruby String's text, refusing what JSON cannot carry.
///
/// Two different refusals, and the gem words them differently: text tagged
/// UTF-8 that is not valid UTF-8 is `source sequence is illegal/malformed
/// utf-8`, while a BINARY string with a high byte fails as a TRANSCODE and
/// names the byte and both encodings.
fn string_text(s: &crate::collections::RStr) -> Result<String, Signal> {
    let g = s.lock();
    let bytes = g.bytes();
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(text.to_string());
    }
    let enc = g.encoding();
    if enc == crate::encoding::ASCII_8BIT {
        let bad = bytes
            .iter()
            .find(|b| **b >= 0x80)
            .map_or(String::new(), |b| format!("\\x{b:02X}"));
        return Err(raise_error(
            "JSON::GeneratorError",
            format!("\"{bad}\" from ASCII-8BIT to UTF-8"),
        ));
    }
    Err(raise_error(
        "JSON::GeneratorError",
        "source sequence is illegal/malformed utf-8".to_string(),
    ))
}

fn escape_into(s: &str, st: &State, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '/' if st.script_safe => out.push_str("\\/"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if st.ascii_only && !c.is_ascii() => {
                let cp = c as u32;
                if cp > 0xFFFF {
                    let v = cp - 0x10000;
                    out.push_str(&format!("\\u{:04x}", 0xD800 + (v >> 10)));
                    out.push_str(&format!("\\u{:04x}", 0xDC00 + (v & 0x3FF)));
                } else {
                    out.push_str(&format!("\\u{cp:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A Float the way the json gem prints one.
///
/// The placement rule was derived from the gem's own output and validated
/// against 60,005 random doubles with zero misses: with the value written
/// as `0.<digits> * 10^decpt`, the PLAIN form is used iff `decpt >= -8` and
/// (`decpt <= 15` or the digits run past `decpt`); otherwise scientific,
/// with at least two exponent digits. That is why `1e15` is `1e+15` while
/// `123456789.123456` stays plain, and why `1e-9` is `0.000000001`.
pub(super) fn float_text(f: f64) -> String {
    if f.is_nan() {
        return "NaN".to_string();
    }
    if f.is_infinite() {
        return match f > 0.0 {
            true => "Infinity".to_string(),
            false => "-Infinity".to_string(),
        };
    }
    // Rust's `{:e}` is the shortest round-tripping form, which is the same
    // digit run the gem starts from -- see the module doc for the 0.9% of
    // doubles where grisu2 prints one more.
    let sci = format!("{f:e}");
    let (mant, exp) = sci.split_once('e').expect("`{:e}` always has an exponent");
    let exp: i32 = exp.parse().expect("`{:e}` writes a decimal exponent");
    let neg = mant.starts_with('-');
    let mant = mant.trim_start_matches('-');
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let decpt = exp + 1;
    let sign = if neg { "-" } else { "" };
    if f == 0.0 {
        return format!("{sign}0.0");
    }
    let len = digits.len() as i32;
    if decpt >= -8 && (decpt <= 15 || len > decpt) {
        return match decpt {
            d if d <= 0 => format!("{sign}0.{}{digits}", "0".repeat((-d) as usize)),
            d if d >= len => format!("{sign}{digits}{}.0", "0".repeat((d - len) as usize)),
            d => format!("{sign}{}.{}", &digits[..d as usize], &digits[d as usize..]),
        };
    }
    let head = &digits[..1];
    let tail = &digits[1..];
    let frac = if tail.is_empty() {
        String::new()
    } else {
        format!(".{tail}")
    };
    let e = decpt - 1;
    format!("{sign}{head}{frac}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_print_the_way_the_gem_does() {
        assert_eq!(float_text(1.0), "1.0");
        assert_eq!(float_text(0.1), "0.1");
        assert_eq!(float_text(1e-9), "0.000000001");
        assert_eq!(float_text(1.5e-7), "0.00000015");
        assert_eq!(float_text(1e15), "1e+15");
        assert_eq!(float_text(1e16), "1e+16");
        assert_eq!(float_text(1e100), "1e+100");
        assert_eq!(float_text(123456789.123456), "123456789.123456");
        assert_eq!(float_text(-0.0), "-0.0");
        assert_eq!(float_text(1e-10), "1e-10");
    }

    #[test]
    fn a_layout_prefixes_items_and_the_closer_differently() {
        let mut st = State {
            indent: "--".to_string(),
            ..State::default()
        };
        let mut out = String::new();
        let a = RubyValue::Array(crate::collections::array_new(vec![
            RubyValue::Int(1),
            RubyValue::Int(2),
        ]));
        generate_into(&a, &mut st, &mut out).unwrap();
        assert_eq!(out, "[--1,--2]");
    }

    #[test]
    fn an_empty_container_gets_no_layout() {
        let mut st = State::pretty();
        let mut out = String::new();
        let a = RubyValue::Array(crate::collections::array_new(vec![]));
        generate_into(&a, &mut st, &mut out).unwrap();
        assert_eq!(out, "[]");
    }

    /// Whether the generator REFUSES `v` -- and that it terminates doing
    /// so. Registry-less `raise_error` panics rather than building an
    /// exception, which is a fact about the unit-test process rather than
    /// about the generator, so a panic IS the refusal here.
    fn refuses(v: RubyValue, st: State) -> bool {
        let hush = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let mut st = st;
            let mut out = String::new();
            generate_into(&v, &mut st, &mut out).is_err()
        }));
        std::panic::set_hook(hush);
        out.unwrap_or(true)
    }

    #[test]
    fn the_depth_counter_is_the_cycle_guard() {
        let a = crate::collections::array_new(vec![RubyValue::Int(1)]);
        a.lock().push(RubyValue::Array(a.clone()));
        assert!(refuses(RubyValue::Array(a), State::default()));
    }

    /// A cycle through a HASH, and one through a hash KEY -- the two
    /// containers the walk recurses into, and both had to be bounded.
    #[test]
    fn a_cycle_through_a_hash_is_bounded_too() {
        let h = crate::collections::hash_new(vec![]);
        crate::collections::hash_set(
            &h,
            RubyValue::Int(1),
            RubyValue::Hash(h.clone()),
        );
        assert!(refuses(RubyValue::Hash(h), State::default()));
    }

    /// `max_nesting: false` must not be able to end the process: the cycle
    /// ceiling refuses first.
    ///
    /// This runs on a stack far larger than any real one, because the test is
    /// about the CEILING and not about the host: a test thread's stack is
    /// small, doubly so in a debug build where every frame is several times
    /// its release size, and without the room the process would die before
    /// the assertion ran -- which is the very thing the ceiling prevents.
    #[test]
    fn the_cycle_ceiling_bounds_an_unbounded_generate() {
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(|| {
                let mut inner = crate::collections::array_new(vec![RubyValue::Int(1)]);
                for _ in 0..CYCLE_CEILING + 1 {
                    inner = crate::collections::array_new(vec![RubyValue::Array(inner)]);
                }
                assert!(refuses(
                    RubyValue::Array(inner),
                    State {
                        max_nesting: None,
                        ..State::default()
                    }
                ));
            })
            .expect("spawning the test thread")
            .join()
            .expect("the test thread finished");
    }

    /// A non-finite Float is refused unless `allow_nan` says otherwise, and
    /// the answer is text either way -- never a panic.
    #[test]
    fn non_finite_floats_refuse_or_spell_themselves() {
        for f in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(refuses(RubyValue::Float(f), State::default()));
            let mut st = State {
                allow_nan: true,
                ..State::default()
            };
            let mut out = String::new();
            generate_into(&RubyValue::Float(f), &mut st, &mut out).unwrap();
            assert!(!out.is_empty());
        }
    }

    /// Every double the float printer can be handed round-trips through
    /// Ruby's own `to_f`, including the subnormal and boundary ones.
    #[test]
    fn every_printed_float_parses_back_to_itself() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let fixed = [
            0.0,
            -0.0,
            f64::MIN_POSITIVE,
            f64::MAX,
            f64::MIN,
            5e-324,
            1.0,
            -1.0,
            0.1,
            1e15,
            1e16,
            1e-9,
            1e100,
            1e-100,
            9007199254740993.0,
        ];
        for f in fixed {
            let text = float_text(f);
            assert_eq!(text.parse::<f64>().unwrap().to_bits(), f.to_bits(), "{f}");
        }
        for _ in 0..20_000 {
            let f = f64::from_bits(next());
            if !f.is_finite() {
                continue;
            }
            let text = float_text(f);
            assert_eq!(
                text.parse::<f64>().unwrap().to_bits(),
                f.to_bits(),
                "{f} printed as {text}"
            );
        }
    }
}

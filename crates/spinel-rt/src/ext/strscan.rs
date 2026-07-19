//! `strscan` (CRuby's bundled `strscan` gem, a C extension) -- `StringScanner`,
//! a position-tracking lexer over a String. `require "strscan"` activates it.
//!
//! Backed by an `RObj` over a `Mutex<State>`. `scan`/`skip`/`match?`/`check`
//! anchor a pattern at the current position (via `regexp::regexp_anchored_len`);
//! `scan_until` searches forward (via `regexp::regexp_find`). Patterns may be a
//! `Regexp` or a `String` (matched literally). The full scan/peek/position
//! surface -- including `exist?`/`check_until`/`get_byte`/`unscan` -- is
//! oracle-verified against ruby 4.0.5.

use crate::builtins::{arity, builtin_methods};
use crate::dispatch::{raise_error, RObj, RubyObject};
use crate::{string_new, RubyValue, Signal};
use parking_lot::Mutex;
use spinel_abi::STRING_SCANNER_CLASS;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct State {
    string: String,
    pos: usize,
    /// Byte span of the most recent successful match (for
    /// `matched`/`pre_match`/`post_match`); `None` after a miss.
    last: Option<(usize, usize)>,
    /// The position before the most recent advancing scan, for `unscan`
    /// (CRuby remembers exactly one).
    prev_pos: Option<usize>,
}

pub struct RStringScanner {
    state: Mutex<State>,
    frozen: AtomicBool,
}

impl RStringScanner {
    fn new(string: String) -> RStringScanner {
        RStringScanner {
            state: Mutex::new(State { string, pos: 0, last: None, prev_pos: None }),
            frozen: AtomicBool::new(false),
        }
    }
}

impl RubyObject for RStringScanner {
    fn class_id(&self) -> crate::ClassId {
        STRING_SCANNER_CLASS
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
        let s = self.state.lock();
        let sc = RStringScanner::new(s.string.clone());
        {
            let mut d = sc.state.lock();
            d.pos = s.pos;
            d.last = s.last;
        }
        if copy_frozen {
            sc.set_frozen();
        }
        Arc::new(sc)
    }
}

fn sc_of(recv: &RubyValue) -> &RStringScanner {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RStringScanner>()
            .expect("the StringScanner table only dispatches on StringScanner receivers"),
        _ => unreachable!("the StringScanner table only dispatches on StringScanner receivers"),
    }
}

/// Anchored match length of `pattern` at the start of `tail` (Regexp or literal
/// String), or `None`.
fn anchored_len(pattern: &RubyValue, tail: &str) -> Result<Option<usize>, Signal> {
    match pattern {
        RubyValue::Regexp(re) => Ok(crate::regexp::regexp_anchored_len(re, tail)),
        RubyValue::Str(s) => {
            let p = s.lock().to_utf8_lossy().into_owned();
            Ok(tail.starts_with(&p).then_some(p.len()))
        }
        other => Err(raise_error(
            "TypeError",
            format!("wrong argument type {} (expected Regexp)", crate::builtins::class_name_of(other)),
        )),
    }
}

fn str_val(text: &str) -> RubyValue {
    RubyValue::Str(string_new(text.to_string()))
}

builtin_methods! {
    pub(crate) fn lookup;

    // Anchored scan: on a hit, consume and return the matched text; else nil.
    "scan" => fn scan(recv, args, _block) {
        arity!(args, 1);
        let st = &mut *sc_of(recv).state.lock();
        let tail = &st.string[st.pos..];
        match anchored_len(&args[0], tail)? {
            Some(len) => {
                let (m0, m1) = (st.pos, st.pos + len);
                st.prev_pos = Some(st.pos);
                st.last = Some((m0, m1));
                st.pos = m1;
                Ok(str_val(&st.string[m0..m1]))
            }
            None => { st.last = None; st.prev_pos = None; Ok(RubyValue::Nil) }
        }
    }
    // Like `scan` but returns the matched LENGTH (or nil), still advancing.
    "skip" => fn skip(recv, args, _block) {
        arity!(args, 1);
        let st = &mut *sc_of(recv).state.lock();
        let tail = &st.string[st.pos..];
        match anchored_len(&args[0], tail)? {
            Some(len) => {
                st.prev_pos = Some(st.pos);
                st.last = Some((st.pos, st.pos + len));
                st.pos += len;
                Ok(RubyValue::Int(len as i64))
            }
            None => { st.last = None; st.prev_pos = None; Ok(RubyValue::Nil) }
        }
    }
    // Anchored length probe -- does NOT advance. Returns the length or nil.
    "match?" => fn match_p(recv, args, _block) {
        arity!(args, 1);
        let st = &mut *sc_of(recv).state.lock();
        let tail = &st.string[st.pos..];
        match anchored_len(&args[0], tail)? {
            Some(len) => { st.last = Some((st.pos, st.pos + len)); Ok(RubyValue::Int(len as i64)) }
            None => { st.last = None; Ok(RubyValue::Nil) }
        }
    }
    // Like `scan` but does NOT advance (peek the matched text).
    "check" => fn check(recv, args, _block) {
        arity!(args, 1);
        let st = &mut *sc_of(recv).state.lock();
        let tail = &st.string[st.pos..];
        match anchored_len(&args[0], tail)? {
            Some(len) => {
                st.last = Some((st.pos, st.pos + len));
                Ok(str_val(&st.string[st.pos..st.pos + len]))
            }
            None => { st.last = None; Ok(RubyValue::Nil) }
        }
    }
    // Scan forward to and including the next match; consume and return the
    // text from the old position through the match, or nil.
    "scan_until" => fn scan_until(recv, args, _block) {
        arity!(args, 1);
        let st = &mut *sc_of(recv).state.lock();
        let tail = &st.string[st.pos..];
        let span = match &args[0] {
            RubyValue::Regexp(re) => crate::regexp::regexp_find(re, tail),
            RubyValue::Str(s) => {
                let p = s.lock().to_utf8_lossy().into_owned();
                tail.find(&p).map(|i| (i, i + p.len()))
            }
            other => return Err(raise_error(
                "TypeError",
                format!("wrong argument type {} (expected Regexp)", crate::builtins::class_name_of(other)),
            )),
        };
        match span {
            Some((rel_start, rel_end)) => {
                let (from, to) = (st.pos, st.pos + rel_end);
                // `matched` is just the matched text (from the match start),
                // but scan_until RETURNS everything consumed: pos..match-end.
                st.prev_pos = Some(st.pos);
                st.last = Some((st.pos + rel_start, to));
                st.pos = to;
                Ok(str_val(&st.string[from..to]))
            }
            None => { st.last = None; st.prev_pos = None; Ok(RubyValue::Nil) }
        }
    }
    "getch" => fn getch(recv, args, _block) {
        arity!(args, 0);
        let st = &mut *sc_of(recv).state.lock();
        if st.pos >= st.string.len() {
            st.last = None;
            return Ok(RubyValue::Nil);
        }
        let ch = st.string[st.pos..].chars().next().expect("pos < len");
        let len = ch.len_utf8();
        let (m0, m1) = (st.pos, st.pos + len);
        st.prev_pos = Some(st.pos);
        st.last = Some((m0, m1));
        st.pos = m1;
        Ok(str_val(&st.string[m0..m1]))
    }
    "peek" => fn peek(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Int(n) = &args[0] else {
            return Err(raise_error("TypeError", "no implicit conversion into Integer".to_string()));
        };
        let st = sc_of(recv).state.lock();
        let end = (st.pos + (*n).max(0) as usize).min(st.string.len());
        Ok(str_val(&st.string[st.pos..end]))
    }
    "rest" => fn rest(recv, args, _block) {
        arity!(args, 0);
        let st = sc_of(recv).state.lock();
        Ok(str_val(&st.string[st.pos..]))
    }
    "eos?" => fn eos(recv, args, _block) {
        arity!(args, 0);
        let st = sc_of(recv).state.lock();
        Ok(RubyValue::Bool(st.pos >= st.string.len()))
    }
    "pos" | "charpos" => fn pos(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(sc_of(recv).state.lock().pos as i64))
    }
    "pos=" => fn set_pos(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Int(n) = &args[0] else {
            return Err(raise_error("TypeError", "no implicit conversion into Integer".to_string()));
        };
        sc_of(recv).state.lock().pos = (*n).max(0) as usize;
        Ok(args[0].clone())
    }
    "reset" => fn reset(recv, args, _block) {
        arity!(args, 0);
        let st = &mut *sc_of(recv).state.lock();
        st.pos = 0;
        st.last = None;
        Ok(recv.clone())
    }
    "terminate" => fn terminate(recv, args, _block) {
        arity!(args, 0);
        let st = &mut *sc_of(recv).state.lock();
        st.pos = st.string.len();
        st.last = None;
        Ok(recv.clone())
    }
    "matched" => fn matched(recv, args, _block) {
        arity!(args, 0);
        let st = sc_of(recv).state.lock();
        Ok(st.last.map(|(a, b)| str_val(&st.string[a..b])).unwrap_or(RubyValue::Nil))
    }
    "matched?" => fn matched_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(sc_of(recv).state.lock().last.is_some()))
    }
    "pre_match" => fn pre_match(recv, args, _block) {
        arity!(args, 0);
        let st = sc_of(recv).state.lock();
        Ok(st.last.map(|(a, _)| str_val(&st.string[..a])).unwrap_or(RubyValue::Nil))
    }
    "post_match" => fn post_match(recv, args, _block) {
        arity!(args, 0);
        let st = sc_of(recv).state.lock();
        Ok(st.last.map(|(_, b)| str_val(&st.string[b..])).unwrap_or(RubyValue::Nil))
    }
    "beginning_of_line?" | "bol?" => fn bol(recv, args, _block) {
        arity!(args, 0);
        let st = sc_of(recv).state.lock();
        Ok(RubyValue::Bool(st.pos == 0 || st.string.as_bytes().get(st.pos - 1) == Some(&b'\n')))
    }
    "string" => fn string(recv, args, _block) {
        arity!(args, 0);
        Ok(str_val(&sc_of(recv).state.lock().string))
    }

    // `exist?(pattern)` -- look ahead for the next match WITHOUT advancing;
    // returns the byte count from the current position to the match end, or nil.
    "exist?" => fn exist(recv, args, _block) {
        arity!(args, 1);
        let st = &mut *sc_of(recv).state.lock();
        let tail = &st.string[st.pos..];
        match find_forward(&args[0], tail)? {
            Some((rel_start, rel_end)) => {
                st.last = Some((st.pos + rel_start, st.pos + rel_end));
                Ok(RubyValue::Int(rel_end as i64))
            }
            None => { st.last = None; Ok(RubyValue::Nil) }
        }
    }
    // Like `scan_until` but does NOT advance -- peek the text from the current
    // position through the next match, or nil.
    "check_until" => fn check_until(recv, args, _block) {
        arity!(args, 1);
        let st = &mut *sc_of(recv).state.lock();
        let tail = &st.string[st.pos..];
        match find_forward(&args[0], tail)? {
            Some((rel_start, rel_end)) => {
                st.last = Some((st.pos + rel_start, st.pos + rel_end));
                Ok(str_val(&st.string[st.pos..st.pos + rel_end]))
            }
            None => { st.last = None; Ok(RubyValue::Nil) }
        }
    }
    // `unscan` -- back the pointer up to before the most recent advancing scan
    // (CRuby remembers exactly one); a ScanError if there is none.
    "unscan" => fn unscan(recv, args, _block) {
        arity!(args, 0);
        let st = &mut *sc_of(recv).state.lock();
        match st.prev_pos.take() {
            Some(p) => {
                st.pos = p;
                st.last = None;
                Ok(recv.clone())
            }
            // `StringScanner::Error` is defined by this gem's RUBY half
            // (`gems/strscan/lib/strscan.rb`), which `require "strscan"` always
            // loads before reaching the native half. A nested user exception
            // class registers under its fully qualified name with a real
            // constructor, so raising it by name here works -- which is why
            // this no longer has to degrade to `RuntimeError`.
            None => Err(raise_error(
                "StringScanner::Error",
                "unscan failed: previous match record not exist".to_string(),
            )),
        }
    }
    // `get_byte` -- one BYTE (not char), advancing by one; nil at end.
    "get_byte" => fn get_byte(recv, args, _block) {
        arity!(args, 0);
        let st = &mut *sc_of(recv).state.lock();
        if st.pos >= st.string.len() {
            st.last = None;
            return Ok(RubyValue::Nil);
        }
        let (m0, m1) = (st.pos, st.pos + 1);
        st.prev_pos = Some(st.pos);
        st.last = Some((m0, m1));
        st.pos = m1;
        // A single raw byte -- lossily UTF-8 for a continuation byte, matching
        // this runtime's Str model (documented divergence, like the rest here).
        Ok(str_val(&String::from_utf8_lossy(&st.string.as_bytes()[m0..m1])))
    }
}

/// Search `tail` forward for `pattern` (Regexp or literal String), returning
/// its byte span relative to `tail`'s start, or `None`.
fn find_forward(pattern: &RubyValue, tail: &str) -> Result<Option<(usize, usize)>, Signal> {
    match pattern {
        RubyValue::Regexp(re) => Ok(crate::regexp::regexp_find(re, tail)),
        RubyValue::Str(s) => {
            let p = s.lock().to_utf8_lossy().into_owned();
            Ok(tail.find(&p).map(|i| (i, i + p.len())))
        }
        other => Err(raise_error(
            "TypeError",
            format!("wrong argument type {} (expected Regexp)", crate::builtins::class_name_of(other)),
        )),
    }
}

builtin_methods! {
    pub(crate) fn lookup_class;

    "new" => fn new_m(_recv, args, _block) {
        arity!(args, 1..=2); // (string[, opts]) -- opts ignored
        let RubyValue::Str(s) = &args[0] else {
            return Err(raise_error(
                "TypeError",
                format!("no implicit conversion of {} into String", crate::builtins::class_name_of(&args[0])),
            ));
        };
        let text = s.lock().to_utf8_lossy().into_owned();
        Ok(RubyValue::Object(Arc::new(RStringScanner::new(text))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(string_new(text.to_string()))
    }
    fn re(src: &str) -> RubyValue {
        RubyValue::Regexp(crate::regexp::regexp_new(src, false, false, false).expect("valid regex"))
    }
    fn text(v: &RubyValue) -> String {
        match v {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn scan_advances_and_anchors() {
        let sc = new_m(&RubyValue::Nil, &[s("foo123")], None).unwrap();
        assert_eq!(text(&scan(&sc, &[re("[a-z]+")], None).unwrap()), "foo");
        // Anchored: a digit pattern won't match starting mid-"123"? it does now.
        assert_eq!(text(&scan(&sc, &[re("\\d+")], None).unwrap()), "123");
        assert!(matches!(eos(&sc, &[], None).unwrap(), RubyValue::Bool(true)));
    }

    #[test]
    fn scan_miss_returns_nil_without_advancing() {
        let sc = new_m(&RubyValue::Nil, &[s("foo")], None).unwrap();
        assert!(matches!(scan(&sc, &[re("\\d+")], None).unwrap(), RubyValue::Nil));
        assert_eq!(sc_of(&sc).state.lock().pos, 0);
    }

    #[test]
    fn scan_until_consumes_through_match() {
        let sc = new_m(&RubyValue::Nil, &[s("a=1;b=2")], None).unwrap();
        assert_eq!(text(&scan_until(&sc, &[re(";")], None).unwrap()), "a=1;");
        assert_eq!(text(&rest(&sc, &[], None).unwrap()), "b=2");
    }
}

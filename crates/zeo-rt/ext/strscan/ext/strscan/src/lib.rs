//! `strscan` (CRuby's bundled `strscan` gem, a C extension) -- `StringScanner`,
//! a position-tracking lexer over a String. `require "strscan"` activates it.
//!
//! Backed by an `RObj` over a `Mutex<State>`. Every method that takes a pattern
//! goes through one door, `regexp::scanner_match`, which anchors at the scan
//! position or searches forward from it and hands back the match's group spans
//! in the string's own coordinates. Patterns may be a `Regexp` or a `String`
//! (matched literally). The whole surface -- scan/peek/position, the capture
//! reads (`[]`/`captures`/`named_captures`/`values_at`), and `inspect` -- is
//! oracle-verified against ruby 4.0.6.

use crate::builtins::index_error;
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::regexp::ScannerMatch;
use crate::{RubyValue, Signal, string_new};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zeo_abi::STRING_SCANNER_CLASS;
use zeo_macros::ruby_class;

/// How many bytes of context each side of the scan position `#inspect` shows.
const INSPECT_CONTEXT: usize = 5;

struct State {
    string: String,
    pos: usize,
    /// The most recent successful match, in the string's own coordinates --
    /// `None` after a miss, exactly as CRuby clears its match registers. Group 0
    /// is the whole match, so `matched`/`pre_match`/`post_match`/`matched_size`
    /// read it too.
    last: Option<ScannerMatch>,
    /// The position before the most recent advancing scan, for `unscan`
    /// (CRuby remembers exactly one).
    prev_pos: Option<usize>,
    /// `StringScanner.new(str, fixed_anchor: true)` -- reported by
    /// `#fixed_anchor?`. Stored rather than acted on: this scanner always hands
    /// the engine the tail slice, which is the `false` behaviour (see
    /// `regexp::scanner_match`'s documented divergence).
    fixed_anchor: bool,
    /// False for the blank `StringScanner.allocate` answers. Ruby keeps an
    /// unseeded scanner distinct from one over `""`: every row raises
    /// `ArgumentError: uninitialized StringScanner object` there, and
    /// `#inspect` names the state.
    initialized: bool,
}

impl State {
    /// The whole match's byte span, or `None` when the last attempt missed.
    fn matched_span(&self) -> Option<(usize, usize)> {
        self.last.as_ref()?.groups.first().copied().flatten()
    }

    /// Records a hit and its group spans.
    fn hit(&mut self, m: ScannerMatch) {
        self.last = Some(m);
    }

    /// Records a miss: CRuby drops both the match registers and the `unscan`
    /// record, so a miss can't be un-scanned.
    fn miss(&mut self) {
        self.last = None;
        self.prev_pos = None;
    }

    /// A group-less advance (`getch`, `get_byte`, `scan_byte`): CRuby still
    /// sets the registers to the consumed span, so `matched` and `[0]` answer.
    fn consumed(&mut self, from: usize, to: usize) {
        self.prev_pos = Some(from);
        self.last = Some(ScannerMatch {
            groups: vec![Some((from, to))],
            names: Vec::new(),
        });
        self.pos = to;
    }
}

pub struct RStringScanner {
    state: Mutex<State>,
    frozen: AtomicBool,
}

impl RStringScanner {
    fn new(string: String, fixed_anchor: bool) -> RStringScanner {
        RStringScanner {
            state: Mutex::new(State {
                string,
                pos: 0,
                last: None,
                prev_pos: None,
                fixed_anchor,
                initialized: true,
            }),
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
        let sc = RStringScanner::new(s.string.clone(), s.fixed_anchor);
        sc.state.lock().pos = s.pos;
        if copy_frozen {
            sc.set_frozen();
        }
        Arc::new(sc)
    }
}

/// A blank `StringScanner` -- no string to scan. Every row but `#inspect`
/// goes through [`live_sc`], which refuses it the way ruby does.
fn scanner_allocate() -> RubyValue {
    let sc = RStringScanner::new(String::new(), false);
    sc.state.lock().initialized = false;
    RubyValue::Object(std::sync::Arc::new(sc))
}

/// [`sc_of`] with ruby's uninitialized guard.
fn live_sc(recv: &RubyValue) -> Result<&RStringScanner, crate::Signal> {
    let sc = sc_of(recv);
    let ok = sc.state.lock().initialized;
    match ok {
        true => Ok(sc),
        false => Err(crate::builtins::arg_error!(
            "uninitialized StringScanner object"
        )),
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

fn str_val(text: &str) -> RubyValue {
    RubyValue::Str(string_new(text.to_string()))
}

/// The text of group `i` of the last match, or `nil` -- `nil` also for an
/// out-of-range index, which is `StringScanner#[]`'s answer where
/// `MatchData#[]` would raise.
fn group_text(st: &State, i: usize) -> RubyValue {
    match st
        .last
        .as_ref()
        .and_then(|m| m.groups.get(i).copied())
        .flatten()
    {
        Some((a, b)) => str_val(&st.string[a..b]),
        None => RubyValue::Nil,
    }
}

/// Resolves `StringScanner#[]`/`#values_at`'s key to a group index. An Integer
/// counts from the end when negative and may be out of range (the caller
/// answers `nil`); a String/Symbol names a group and MUST exist.
fn group_index(st: &State, key: &RubyValue) -> Result<Option<usize>, Signal> {
    let named = |name: &str| match st.last.as_ref() {
        Some(m) => m
            .names
            .iter()
            .find(|(n, _)| n == name)
            .map(|&(_, i)| Some(i))
            .ok_or_else(|| index_error!("undefined group name reference: {name}")),
        // No match on record: CRuby reports the name as undefined too.
        None => Err(index_error!("undefined group name reference: {name}")),
    };
    match key {
        RubyValue::Symbol(s) => named(&s.name()),
        RubyValue::Str(s) => named(&s.lock().to_utf8_lossy()),
        other => {
            let n = crate::builtins::convert::to_index(other)?;
            let count = st.last.as_ref().map_or(0, |m| m.groups.len()) as i64;
            let i = if n < 0 { n + count } else { n };
            Ok((0..count).contains(&i).then_some(i as usize))
        }
    }
}

/// `#inspect`'s rendering of one side's context bytes: `dump`ed as a binary
/// string (so a multi-byte character shows as `\xC3\xA9`, matching CRuby),
/// with the ellipsis CRuby puts INSIDE the quotes when there is more to see.
fn inspect_context(bytes: &[u8], truncated: bool, leading: bool) -> String {
    let mut out = String::from("\"");
    if truncated && leading {
        out.push_str("...");
    }
    for &b in bytes {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\t' => out.push_str("\\t"),
            b'\r' => out.push_str("\\r"),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\x{b:02X}")),
        }
    }
    if truncated && !leading {
        out.push_str("...");
    }
    out.push('"');
    out
}

/// `scan_full`/`search_full`'s shared body: `args` is
/// `(pattern, advance_pointer, return_string)`. Between them these two are the
/// primitive the four anchored methods are each a fixed corner of -- `scan` is
/// `(true, true)`, `skip` `(true, false)`, `check` `(false, true)`, `match?`
/// `(false, false)` -- over the whole span consumed rather than the match alone.
fn full_scan(st: &mut State, args: &[RubyValue], anchored: bool) -> Result<RubyValue, Signal> {
    let Some(m) = crate::regexp::scanner_match(&args[0], &st.string, st.pos, anchored)? else {
        st.miss();
        return Ok(RubyValue::Nil);
    };
    let (_, to) = m.groups[0].expect("group 0 of a hit always participates");
    let from = st.pos;
    st.prev_pos = Some(from);
    st.hit(m);
    if args[1].truthy() {
        st.pos = to;
    }
    Ok(match args[2].truthy() {
        true => str_val(&st.string[from..to]),
        false => RubyValue::Int((to - from) as i64),
    })
}

/// `base:` from the trailing options Hash (the kwargs convention) -- CRuby's
/// `scan_integer` accepts 10 and 16 and rejects everything else.
fn integer_base(args: &[RubyValue]) -> Result<u32, Signal> {
    let Some(RubyValue::Hash(h)) = args.last() else {
        return Ok(10);
    };
    let v = crate::collections::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern("base")));
    match v {
        RubyValue::Nil => Ok(10),
        other => match crate::builtins::convert::to_index(&other)? {
            10 => Ok(10),
            16 => Ok(16),
            n => Err(crate::builtins::arg_error!("Unsupported integer base: {n}")),
        },
    }
}

ruby_class! {
    StringScanner = zeo_abi::STRING_SCANNER_CLASS < zeo_abi::OBJECT_CLASS;

    allocate scanner_allocate;

    // Anchored scan: on a hit, consume and return the matched text; else nil.
    def "scan" (recv, arg) {
        let st = &mut *live_sc(recv)?.state.lock();
        match crate::regexp::scanner_match(arg, &st.string, st.pos, true)? {
            Some(m) => {
                let (a, b) = m.groups[0].expect("group 0 of a hit always participates");
                st.prev_pos = Some(st.pos);
                st.hit(m);
                st.pos = b;
                Ok(str_val(&st.string[a..b]))
            }
            None => { st.miss(); Ok(RubyValue::Nil) }
        }
    }
    // Like `scan` but returns the matched LENGTH (or nil), still advancing.
    def "skip" (recv, arg) {
        let st = &mut *live_sc(recv)?.state.lock();
        match crate::regexp::scanner_match(arg, &st.string, st.pos, true)? {
            Some(m) => {
                let (a, b) = m.groups[0].expect("group 0 of a hit always participates");
                st.prev_pos = Some(st.pos);
                st.hit(m);
                st.pos = b;
                Ok(RubyValue::Int((b - a) as i64))
            }
            None => { st.miss(); Ok(RubyValue::Nil) }
        }
    }
    // Anchored length probe -- does NOT advance. Returns the length or nil.
    def "match?" (recv, arg) {
        let st = &mut *live_sc(recv)?.state.lock();
        match crate::regexp::scanner_match(arg, &st.string, st.pos, true)? {
            Some(m) => {
                let (a, b) = m.groups[0].expect("group 0 of a hit always participates");
                st.hit(m);
                Ok(RubyValue::Int((b - a) as i64))
            }
            None => { st.last = None; Ok(RubyValue::Nil) }
        }
    }
    // Like `scan` but does NOT advance (peek the matched text).
    def "check" (recv, arg) {
        let st = &mut *live_sc(recv)?.state.lock();
        match crate::regexp::scanner_match(arg, &st.string, st.pos, true)? {
            Some(m) => {
                let (a, b) = m.groups[0].expect("group 0 of a hit always participates");
                st.hit(m);
                Ok(str_val(&st.string[a..b]))
            }
            None => { st.last = None; Ok(RubyValue::Nil) }
        }
    }
    // Scan forward to and including the next match; consume and return the
    // text from the old position through the match, or nil.
    def "scan_until" (recv, arg) {
        let st = &mut *live_sc(recv)?.state.lock();
        match crate::regexp::scanner_match(arg, &st.string, st.pos, false)? {
            Some(m) => {
                let (_, to) = m.groups[0].expect("group 0 of a hit always participates");
                // `matched` is just the matched text, but scan_until RETURNS
                // everything consumed: the old position through the match end.
                let from = st.pos;
                st.prev_pos = Some(from);
                st.hit(m);
                st.pos = to;
                Ok(str_val(&st.string[from..to]))
            }
            None => { st.miss(); Ok(RubyValue::Nil) }
        }
    }
    // `skip_until` -- `scan_until`'s length-returning form.
    def "skip_until" (recv, arg) {
        let st = &mut *live_sc(recv)?.state.lock();
        match crate::regexp::scanner_match(arg, &st.string, st.pos, false)? {
            Some(m) => {
                let (_, to) = m.groups[0].expect("group 0 of a hit always participates");
                let from = st.pos;
                st.prev_pos = Some(from);
                st.hit(m);
                st.pos = to;
                Ok(RubyValue::Int((to - from) as i64))
            }
            None => { st.miss(); Ok(RubyValue::Nil) }
        }
    }
    // `scan_full(pattern, advance, return_string)` -- the primitive the four
    // anchored methods above are each a fixed corner of: `scan` is
    // `(true, true)`, `skip` `(true, false)`, `check` `(false, true)`,
    // `match?` `(false, false)`. `search_full` is the same for the forward
    // search, over the whole span consumed rather than the match alone.
    def "scan_full" (recv, _pattern, _advance_pointer, _return_string) {
        full_scan(&mut live_sc(recv)?.state.lock(), __args, true)
    }
    def "search_full" (recv, _pattern, _advance_pointer, _return_string) {
        full_scan(&mut live_sc(recv)?.state.lock(), __args, false)
    }
    def "getch" (recv) {
        let st = &mut *live_sc(recv)?.state.lock();
        if st.pos >= st.string.len() {
            st.last = None;
            return Ok(RubyValue::Nil);
        }
        let ch = st.string[st.pos..].chars().next().expect("pos < len");
        let (from, to) = (st.pos, st.pos + ch.len_utf8());
        st.consumed(from, to);
        Ok(str_val(&st.string[from..to]))
    }
    def "peek" (recv, arg) {
        let n = &crate::builtins::convert::to_index(arg)?;
        let st = live_sc(recv)?.state.lock();
        let end = (st.pos + (*n).max(0) as usize).min(st.string.len());
        Ok(str_val(&st.string[st.pos..end]))
    }
    def "rest" (recv) {
        let st = live_sc(recv)?.state.lock();
        Ok(str_val(&st.string[st.pos..]))
    }
    // Bytes, not characters -- the counterpart of `pos` (`charpos` is the one
    // that counts characters).
    def "rest_size" (recv) {
        let st = live_sc(recv)?.state.lock();
        Ok(RubyValue::Int((st.string.len() - st.pos.min(st.string.len())) as i64))
    }
    def "rest?" (recv) {
        let st = live_sc(recv)?.state.lock();
        Ok(RubyValue::Bool(st.pos < st.string.len()))
    }
    def "eos?" (recv) {
        let st = live_sc(recv)?.state.lock();
        Ok(RubyValue::Bool(st.pos >= st.string.len()))
    }
    // BYTE offset -- Ruby's scanner positions are byte-based throughout.
    def "pos" | "pointer" (recv) {
        Ok(RubyValue::Int(live_sc(recv)?.state.lock().pos as i64))
    }
    // ...and its CHARACTER-counting sibling, which differs the moment the
    // string holds anything multi-byte.
    def "charpos" (recv) {
        let st = live_sc(recv)?.state.lock();
        Ok(RubyValue::Int(st.string[..st.pos].chars().count() as i64))
    }
    def "pos=" | "pointer=" (recv, arg) {
        let n = crate::builtins::convert::to_index(arg)?;
        let mut st = live_sc(recv)?.state.lock();
        let len = st.string.len() as i64;
        // A NEGATIVE position counts from the end, and anything outside the
        // subject RAISES. Clamping silently put the scanner somewhere the
        // program did not ask for.
        let at = if n < 0 { n + len } else { n };
        if at < 0 || at > len {
            return Err(crate::builtins::range_error!("index out of range"));
        }
        st.pos = at as usize;
        Ok((*arg).clone())
    }
    def "reset" (recv) {
        let st = &mut *live_sc(recv)?.state.lock();
        st.pos = 0;
        st.last = None;
        Ok(recv.clone())
    }
    def "terminate" (recv) {
        let st = &mut *live_sc(recv)?.state.lock();
        st.pos = st.string.len();
        st.last = None;
        Ok(recv.clone())
    }
    def "matched" (recv) {
        let st = live_sc(recv)?.state.lock();
        Ok(group_text(&st, 0))
    }
    def "matched?" (recv) {
        Ok(RubyValue::Bool(live_sc(recv)?.state.lock().last.is_some()))
    }
    // The matched text's BYTE length, or nil when the last attempt missed.
    def "matched_size" (recv) {
        let st = live_sc(recv)?.state.lock();
        Ok(match st.matched_span() {
            Some((a, b)) => RubyValue::Int((b - a) as i64),
            None => RubyValue::Nil,
        })
    }
    def "pre_match" (recv) {
        let st = live_sc(recv)?.state.lock();
        Ok(match st.matched_span() {
            Some((a, _)) => str_val(&st.string[..a]),
            None => RubyValue::Nil,
        })
    }
    def "post_match" (recv) {
        let st = live_sc(recv)?.state.lock();
        Ok(match st.matched_span() {
            Some((_, b)) => str_val(&st.string[b..]),
            None => RubyValue::Nil,
        })
    }
    // `scanner[n]` / `scanner[:name]` -- a group of the LAST match. Unlike
    // `MatchData#[]`, an out-of-range INDEX answers nil rather than raising;
    // an unknown NAME still raises, matching CRuby.
    def "[]" (recv, arg) {
        let st = live_sc(recv)?.state.lock();
        Ok(match group_index(&st, arg)? {
            Some(i) => group_text(&st, i),
            None => RubyValue::Nil,
        })
    }
    // Every group EXCEPT the whole match, `nil` for one that didn't
    // participate; nil overall when the last attempt missed.
    def "captures" (recv) {
        let st = live_sc(recv)?.state.lock();
        let Some(m) = st.last.as_ref() else { return Ok(RubyValue::Nil) };
        let out = (1..m.groups.len()).map(|i| group_text(&st, i)).collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    def "values_at" (recv, *args, &_block) {
        let st = live_sc(recv)?.state.lock();
        if st.last.is_none() {
            return Ok(RubyValue::Nil);
        }
        let mut out = Vec::with_capacity(args.len());
        for key in args {
            out.push(match group_index(&st, key)? {
                Some(i) => group_text(&st, i),
                None => RubyValue::Nil,
            });
        }
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    // `{ "name" => text }` for each named group -- `{}` (not nil) when the
    // pattern had none, and `{}` when the last attempt missed.
    def "named_captures" (recv) {
        let st = live_sc(recv)?.state.lock();
        let pairs = st.last.as_ref().map_or_else(Vec::new, |m| {
            m.names
                .iter()
                .map(|(name, i)| (str_val(name), group_text(&st, *i)))
                .collect()
        });
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }
    // The group COUNT of the last match, whole match included -- nil on a miss.
    def "size" (recv) {
        let st = live_sc(recv)?.state.lock();
        Ok(match st.last.as_ref() {
            Some(m) => RubyValue::Int(m.groups.len() as i64),
            None => RubyValue::Nil,
        })
    }
    def "beginning_of_line?" | "bol?" (recv) {
        let st = live_sc(recv)?.state.lock();
        Ok(RubyValue::Bool(st.pos == 0 || st.string.as_bytes().get(st.pos - 1) == Some(&b'\n')))
    }
    def "string" (recv) {
        Ok(str_val(&live_sc(recv)?.state.lock().string))
    }
    // Replacing the subject restarts the scan; APPENDING to it doesn't, which
    // is the whole point of `<<` (feeding a scanner incrementally).
    def "string=" (recv, arg) {
        let s = &crate::builtins::convert::to_rstr(arg)?;
        let st = &mut *live_sc(recv)?.state.lock();
        st.string = s.lock().to_utf8_lossy().into_owned();
        st.pos = 0;
        st.last = None;
        st.prev_pos = None;
        Ok((*arg).clone())
    }
    def "concat" | "<<" (recv, other) {
        let s = &crate::builtins::convert::to_rstr(other)?;
        let text = s.lock().to_utf8_lossy().into_owned();
        live_sc(recv)?.state.lock().string.push_str(&text);
        Ok(recv.clone())
    }
    // Whether `^`/`\A` anchor to the string start rather than the scan
    // position. Always false unless asked for at construction -- and see
    // `regexp::scanner_match` for why the true form isn't honoured yet.
    def "fixed_anchor?" (recv) {
        Ok(RubyValue::Bool(live_sc(recv)?.state.lock().fixed_anchor))
    }
    // `#<StringScanner 5/30 "aaaaa" @ "aaaaa...">` -- position over length,
    // then up to five bytes each side of it; a spent scanner is just
    // `#<StringScanner fin>`.
    def "inspect" (recv) {
        let st = sc_of(recv).state.lock();
        // The one row a blank answers, and it names the state.
        if !st.initialized {
            return Ok(str_val("#<StringScanner (uninitialized)>"));
        }
        if st.pos >= st.string.len() {
            return Ok(str_val("#<StringScanner fin>"));
        }
        let bytes = st.string.as_bytes();
        let after_len = (bytes.len() - st.pos).min(INSPECT_CONTEXT);
        let after = inspect_context(
            &bytes[st.pos..st.pos + after_len],
            bytes.len() - st.pos > INSPECT_CONTEXT,
            false,
        );
        let head = format!("#<StringScanner {}/{}", st.pos, bytes.len());
        if st.pos == 0 {
            return Ok(str_val(&format!("{head} @ {after}>")));
        }
        let before_len = st.pos.min(INSPECT_CONTEXT);
        let before = inspect_context(
            &bytes[st.pos - before_len..st.pos],
            st.pos > INSPECT_CONTEXT,
            true,
        );
        Ok(str_val(&format!("{head} {before} @ {after}>")))
    }

    // `exist?(pattern)` -- look ahead for the next match WITHOUT advancing;
    // returns the byte count from the current position to the match end, or nil.
    def "exist?" (recv, arg) {
        let st = &mut *live_sc(recv)?.state.lock();
        match crate::regexp::scanner_match(arg, &st.string, st.pos, false)? {
            Some(m) => {
                let (_, to) = m.groups[0].expect("group 0 of a hit always participates");
                let len = to - st.pos;
                st.hit(m);
                Ok(RubyValue::Int(len as i64))
            }
            None => { st.last = None; Ok(RubyValue::Nil) }
        }
    }
    // Like `scan_until` but does NOT advance -- peek the text from the current
    // position through the next match, or nil.
    def "check_until" (recv, arg) {
        let st = &mut *live_sc(recv)?.state.lock();
        match crate::regexp::scanner_match(arg, &st.string, st.pos, false)? {
            Some(m) => {
                let (_, to) = m.groups[0].expect("group 0 of a hit always participates");
                let from = st.pos;
                st.hit(m);
                Ok(str_val(&st.string[from..to]))
            }
            None => { st.last = None; Ok(RubyValue::Nil) }
        }
    }
    // `unscan` -- back the pointer up to before the most recent advancing scan
    // (CRuby remembers exactly one); a ScanError if there is none.
    def "unscan" (recv) {
        let st = &mut *live_sc(recv)?.state.lock();
        match st.prev_pos.take() {
            Some(p) => {
                st.pos = p;
                st.last = None;
                Ok(recv.clone())
            }
            // `StringScanner::Error` is defined by this gem's RUBY half
            // (`ext/strscan/lib/strscan.rb`), which `require "strscan"` always
            // loads before reaching the native half. A nested user exception
            // class registers under its fully qualified name with a real
            // constructor, so raising it by name here works.
            None => Err(raise_error(
                "StringScanner::Error",
                "unscan failed: previous match record not exist".to_string(),
            )),
        }
    }
    // `get_byte` -- one BYTE (not char), advancing by one; nil at end.
    def "get_byte" (recv) {
        let st = &mut *live_sc(recv)?.state.lock();
        if st.pos >= st.string.len() {
            st.last = None;
            return Ok(RubyValue::Nil);
        }
        let (from, to) = (st.pos, st.pos + 1);
        st.consumed(from, to);
        // ONE byte, tagged with the subject's own encoding. Through
        // `from_utf8_lossy` a continuation byte became U+FFFD -- three bytes
        // where ruby answers one, and never the byte that is actually there.
        let byte = st.string.as_bytes()[from];
        Ok(RubyValue::Str(crate::string_from_bytes(
            vec![byte],
            crate::encoding::UTF_8,
        )))
    }
    // `scan_byte`/`peek_byte` -- `get_byte`/`peek(1)` as an INTEGER, which is
    // what a byte-level lexer actually wants.
    def "scan_byte" (recv) {
        let st = &mut *live_sc(recv)?.state.lock();
        if st.pos >= st.string.len() {
            st.last = None;
            return Ok(RubyValue::Nil);
        }
        let byte = st.string.as_bytes()[st.pos];
        let (from, to) = (st.pos, st.pos + 1);
        st.consumed(from, to);
        Ok(RubyValue::Int(byte as i64))
    }
    def "peek_byte" (recv) {
        let st = live_sc(recv)?.state.lock();
        Ok(match st.string.as_bytes().get(st.pos) {
            Some(&b) => RubyValue::Int(b as i64),
            None => RubyValue::Nil,
        })
    }
    // `scan_integer(base: 10)` -- an optional sign then digits in base 10 or
    // 16 (where a `0x` prefix is allowed), consumed and returned as an
    // Integer. nil, consuming nothing, when the text isn't one.
    def "scan_integer" (recv, *args, &_block) {
        let base = integer_base(args)?;
        let st = &mut *live_sc(recv)?.state.lock();
        let tail = &st.string[st.pos..];
        let mut end = 0;
        if tail.starts_with(['+', '-']) {
            end += 1;
        }
        if base == 16 && tail[end..].starts_with("0x") {
            end += 2;
        }
        let digits = tail[end..]
            .bytes()
            .take_while(|b| (b as &u8).is_ascii_digit() || (base == 16 && b.is_ascii_hexdigit()))
            .count();
        if digits == 0 {
            st.miss();
            return Ok(RubyValue::Nil);
        }
        end += digits;
        // The `0x` prefix has to come back off for `from_str_radix`, which
        // takes bare digits with an optional sign.
        let literal = &tail[..end];
        let cleaned = literal.replacen("0x", "", 1);
        let Ok(n) = i64::from_str_radix(&cleaned, base) else {
            st.miss();
            return Ok(RubyValue::Nil);
        };
        let (from, to) = (st.pos, st.pos + end);
        st.consumed(from, to);
        Ok(RubyValue::Int(n))
    }

    def self."new" cfunc allocs (_recv, string, opts?) {
        let s = &crate::builtins::convert::to_rstr(string)?;
        let text = s.lock().to_utf8_lossy().into_owned();
        let fixed_anchor = match opts {
            Some(RubyValue::Hash(h)) => crate::collections::hash_get(
                h,
                &RubyValue::Symbol(crate::Symbol::intern("fixed_anchor")),
            )
            .truthy(),
            _ => false,
        };
        Ok(RubyValue::Object(Arc::new(RStringScanner::new(text, fixed_anchor))))
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
    /// `StringScanner`'s `ruby_class!`-generated methods have mangled Rust
    /// idents, so the tests reach them through the registered tables.
    fn tbl() -> &'static crate::builtins::BuiltinClassTable {
        crate::builtins::registered_table(zeo_abi::STRING_SCANNER_CLASS)
            .expect("StringScanner is a registered builtin table")
    }
    fn im(name: &str) -> crate::builtins::BuiltinMethodFn {
        let t = tbl()
            .instance
            .as_ref()
            .expect("StringScanner has instance methods");
        (t.lookup)(name).unwrap_or_else(|| panic!("StringScanner#{name} is defined"))
    }
    fn cm(name: &str) -> crate::builtins::BuiltinMethodFn {
        let t = tbl()
            .class
            .as_ref()
            .expect("StringScanner has class methods");
        (t.lookup)(name).unwrap_or_else(|| panic!("StringScanner.{name} is defined"))
    }
    fn scanner(subject: &str) -> RubyValue {
        cm("new")(&RubyValue::Nil, &[s(subject)], None).expect("StringScanner.new")
    }
    fn call(sc: &RubyValue, name: &str, args: &[RubyValue]) -> RubyValue {
        im(name)(sc, args, None).unwrap_or_else(|_| panic!("StringScanner#{name} raised"))
    }

    #[test]
    fn scan_advances_and_anchors() {
        let sc = scanner("foo123");
        assert_eq!(text(&call(&sc, "scan", &[re("[a-z]+")])), "foo");
        assert_eq!(text(&call(&sc, "scan", &[re("\\d+")])), "123");
        assert!(matches!(call(&sc, "eos?", &[]), RubyValue::Bool(true)));
    }

    #[test]
    fn scan_miss_returns_nil_without_advancing() {
        let sc = scanner("foo");
        assert!(matches!(call(&sc, "scan", &[re("\\d+")]), RubyValue::Nil));
        assert_eq!(sc_of(&sc).state.lock().pos, 0);
    }

    #[test]
    fn scan_until_consumes_through_match() {
        let sc = scanner("a=1;b=2");
        assert_eq!(text(&call(&sc, "scan_until", &[re(";")])), "a=1;");
        assert_eq!(text(&call(&sc, "rest", &[])), "b=2");
    }

    #[test]
    fn captures_read_the_last_match() {
        let sc = scanner("key=value");
        call(&sc, "scan", &[re("(\\w+)=(\\w+)")]);
        assert_eq!(text(&call(&sc, "[]", &[RubyValue::Int(1)])), "key");
        assert_eq!(text(&call(&sc, "[]", &[RubyValue::Int(2)])), "value");
        // An out-of-range index is nil here, where `MatchData#[]` would raise.
        assert!(matches!(
            call(&sc, "[]", &[RubyValue::Int(9)]),
            RubyValue::Nil
        ));
        // ...and a negative index counts back from the last group.
        assert_eq!(text(&call(&sc, "[]", &[RubyValue::Int(-1)])), "value");
        assert!(matches!(call(&sc, "size", &[]), RubyValue::Int(3)));
    }

    #[test]
    fn named_groups_are_reachable_by_name() {
        let sc = scanner("2026-07-28");
        call(&sc, "scan", &[re("(?<y>\\d+)-(?<m>\\d+)")]);
        assert_eq!(text(&call(&sc, "[]", &[s("y")])), "2026");
        assert_eq!(
            text(&call(
                &sc,
                "[]",
                &[RubyValue::Symbol(crate::Symbol::intern("m"))]
            )),
            "07"
        );
        // The unknown-NAME IndexError needs the exception registry, which only
        // a generated program installs -- `strscan_capture_surface.rb` covers it.
    }

    #[test]
    fn a_miss_clears_the_match_registers() {
        let sc = scanner("abc");
        call(&sc, "scan", &[re("(a)")]);
        assert!(matches!(call(&sc, "scan", &[re("\\d")]), RubyValue::Nil));
        assert!(matches!(call(&sc, "captures", &[]), RubyValue::Nil));
        assert!(matches!(call(&sc, "size", &[]), RubyValue::Nil));
        assert!(matches!(call(&sc, "matched", &[]), RubyValue::Nil));
    }

    #[test]
    fn pos_counts_bytes_and_charpos_counts_characters() {
        let sc = scanner("héllo");
        call(&sc, "scan", &[re("h.")]);
        assert!(matches!(call(&sc, "pos", &[]), RubyValue::Int(3)));
        assert!(matches!(call(&sc, "charpos", &[]), RubyValue::Int(2)));
        assert!(matches!(call(&sc, "rest_size", &[]), RubyValue::Int(3)));
    }

    #[test]
    fn inspect_shows_the_position_and_its_context() {
        let sc = scanner("abc");
        assert_eq!(
            text(&call(&sc, "inspect", &[])),
            "#<StringScanner 0/3 @ \"abc\">"
        );
        let long = scanner(&"a".repeat(30));
        call(&long, "scan", &[re("a{5}")]);
        assert_eq!(
            text(&call(&long, "inspect", &[])),
            "#<StringScanner 5/30 \"aaaaa\" @ \"aaaaa...\">"
        );
        call(&long, "terminate", &[]);
        assert_eq!(text(&call(&long, "inspect", &[])), "#<StringScanner fin>");
    }

    #[test]
    fn scan_integer_reads_a_signed_literal() {
        let sc = scanner("+12ab");
        assert!(matches!(call(&sc, "scan_integer", &[]), RubyValue::Int(12)));
        assert_eq!(text(&call(&sc, "rest", &[])), "ab");
        assert!(matches!(
            call(&scanner("ab"), "scan_integer", &[]),
            RubyValue::Nil
        ));
    }

    #[test]
    fn appending_keeps_the_position_but_replacing_resets_it() {
        let sc = scanner("ab");
        call(&sc, "scan", &[re("a")]);
        call(&sc, "<<", &[s("cd")]);
        assert!(matches!(call(&sc, "pos", &[]), RubyValue::Int(1)));
        assert_eq!(text(&call(&sc, "string", &[])), "abcd");
        call(&sc, "string=", &[s("zz")]);
        assert!(matches!(call(&sc, "pos", &[]), RubyValue::Int(0)));
    }
}

//! `stringio` (CRuby's bundled `stringio` gem, a C extension) -- an in-memory
//! bytes buffer with the `IO` read/write surface, no operating-system stream
//! behind it. `require "stringio"` activates it (a built-in feature; see
//! `ext/mod.rs`).
//!
//! Backed by an `RObj` over a `Mutex<State>` (a `StringIO` is mutable and
//! shared by reference). Positions are byte offsets; writes overwrite from the
//! current position and extend the buffer, exactly like a file. The read/write
//! surface -- including `seek`/`getc`/`readline`/`readlines`/`truncate` -- is
//! oracle-verified against ruby 4.0.5.
//!
//! Documented divergence: strings are handed back as UTF-8 (lossy for non-UTF-8
//! bytes), matching this runtime's default `Str` -- CRuby's StringIO preserves
//! arbitrary bytes with an encoding.

use crate::builtins::{arity, builtin_methods};
use crate::dispatch::{raise_error, RObj, RubyObject};
use crate::{string_new, RubyValue};
use parking_lot::Mutex;
use zeo_abi::STRINGIO_CLASS;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct State {
    bytes: Vec<u8>,
    pos: usize,
    closed: bool,
}

pub struct RStringIO {
    state: Mutex<State>,
    frozen: AtomicBool,
}

impl RStringIO {
    fn with_bytes(bytes: Vec<u8>) -> RStringIO {
        RStringIO {
            state: Mutex::new(State { bytes, pos: 0, closed: false }),
            frozen: AtomicBool::new(false),
        }
    }
}

impl RubyObject for RStringIO {
    fn class_id(&self) -> crate::ClassId {
        STRINGIO_CLASS
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
        let io = RStringIO::with_bytes(s.bytes.clone());
        io.state.lock().pos = s.pos;
        if copy_frozen {
            io.set_frozen();
        }
        Arc::new(io)
    }
}

/// The `RStringIO` behind a receiver -- the table only dispatches on one.
fn io_of(recv: &RubyValue) -> &RStringIO {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RStringIO>()
            .expect("the StringIO table only dispatches on StringIO receivers"),
        _ => unreachable!("the StringIO table only dispatches on StringIO receivers"),
    }
}

/// The bytes of a `String` argument, or `to_s` of anything else (what
/// `IO#write`/`#print`/`#puts` do to non-strings).
fn arg_bytes(v: &RubyValue) -> Vec<u8> {
    match v {
        RubyValue::Str(s) => s.lock().bytes().to_vec(),
        other => other.to_display_string().into_bytes(),
    }
}

fn bytes_to_str(bytes: &[u8]) -> RubyValue {
    RubyValue::Str(string_new(String::from_utf8_lossy(bytes).into_owned()))
}

/// Overwrite-from-`pos` write, extending the buffer as a file would.
fn write_at(state: &mut State, data: &[u8]) {
    let end = state.pos + data.len();
    if state.bytes.len() < end {
        state.bytes.resize(end, 0);
    }
    state.bytes[state.pos..end].copy_from_slice(data);
    state.pos = end;
}

builtin_methods! {
    pub(crate) fn lookup;

    // The whole buffer as a String, independent of position.
    "string" => fn string(recv, args, _block) {
        arity!(args, 0);
        Ok(bytes_to_str(&io_of(recv).state.lock().bytes))
    }
    "read" => fn read(recv, args, _block) {
        arity!(args, 0..=1);
        let mut s = io_of(recv).state.lock();
        match args.first() {
            None | Some(RubyValue::Nil) => {
                let out = s.bytes[s.pos.min(s.bytes.len())..].to_vec();
                s.pos = s.bytes.len();
                Ok(bytes_to_str(&out))
            }
            Some(RubyValue::Int(n)) if *n >= 0 => {
                let n = *n as usize;
                if s.pos >= s.bytes.len() && n > 0 {
                    return Ok(RubyValue::Nil); // EOF with a nonzero length is nil
                }
                let end = (s.pos + n).min(s.bytes.len());
                let out = s.bytes[s.pos..end].to_vec();
                s.pos = end;
                Ok(bytes_to_str(&out))
            }
            Some(other) => Err(raise_error(
                "TypeError",
                format!("no implicit conversion of {} into Integer", crate::builtins::convert_name_of(other)),
            )),
        }
    }
    "write" => fn write(recv, args, _block) {
        let mut written = 0usize;
        let mut s = io_of(recv).state.lock();
        for a in args {
            let b = arg_bytes(a);
            written += b.len();
            write_at(&mut s, &b);
        }
        Ok(RubyValue::Int(written as i64))
    }
    "<<" => fn push(recv, args, _block) {
        arity!(args, 1);
        let mut s = io_of(recv).state.lock();
        write_at(&mut s, &arg_bytes(&args[0]));
        Ok(recv.clone())
    }
    "print" => fn print(recv, args, _block) {
        let mut s = io_of(recv).state.lock();
        for a in args {
            write_at(&mut s, &arg_bytes(a));
        }
        Ok(RubyValue::Nil)
    }
    "puts" => fn puts(recv, args, _block) {
        let mut s = io_of(recv).state.lock();
        if args.is_empty() {
            write_at(&mut s, b"\n");
        }
        for a in args {
            puts_one(&mut s, a);
        }
        Ok(RubyValue::Nil)
    }
    "gets" => fn gets(recv, args, _block) {
        arity!(args, 0..=1);
        let sep = match args.first() {
            None | Some(RubyValue::Nil) => "\n".to_string(),
            Some(RubyValue::Str(s)) => s.lock().to_utf8_lossy().into_owned(),
            Some(other) => other.to_display_string(),
        };
        let mut s = io_of(recv).state.lock();
        if s.pos >= s.bytes.len() {
            return Ok(RubyValue::Nil);
        }
        let rest = &s.bytes[s.pos..];
        let end = find_sub(rest, sep.as_bytes())
            .map(|i| s.pos + i + sep.len())
            .unwrap_or(s.bytes.len());
        let line = s.bytes[s.pos..end].to_vec();
        s.pos = end;
        Ok(bytes_to_str(&line))
    }
    "each_line" => fn each_line(recv, args, block) {
        arity!(args, 0);
        let Some(RubyValue::Proc(p)) = block else {
            return Err(crate::dispatch::raise_no_block_yield());
        };
        loop {
            let line = gets(recv, &[], None)?;
            if matches!(line, RubyValue::Nil) {
                break;
            }
            p.call(&[line])?;
        }
        Ok(recv.clone())
    }
    "eof?" | "eof" => fn eof(recv, args, _block) {
        arity!(args, 0);
        let s = io_of(recv).state.lock();
        Ok(RubyValue::Bool(s.pos >= s.bytes.len()))
    }
    "rewind" => fn rewind(recv, args, _block) {
        arity!(args, 0);
        io_of(recv).state.lock().pos = 0;
        Ok(RubyValue::Int(0))
    }
    "pos" | "tell" => fn pos(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(io_of(recv).state.lock().pos as i64))
    }
    "pos=" => fn set_pos(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Int(n) = &args[0] else {
            return Err(raise_error("TypeError", "no implicit conversion into Integer".to_string()));
        };
        io_of(recv).state.lock().pos = (*n).max(0) as usize;
        Ok(args[0].clone())
    }
    "size" | "length" => fn size(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(io_of(recv).state.lock().bytes.len() as i64))
    }
    "close" => fn close(recv, args, _block) {
        arity!(args, 0);
        io_of(recv).state.lock().closed = true;
        Ok(RubyValue::Nil)
    }
    "closed?" => fn closed(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(io_of(recv).state.lock().closed))
    }

    // `seek(offset, whence = SEEK_SET)` -- reposition; whence 0/1/2 =
    // absolute/relative/from-end. Returns 0, like CRuby's IO#seek.
    "seek" => fn seek(recv, args, _block) {
        arity!(args, 1..=2);
        let RubyValue::Int(off) = &args[0] else {
            return Err(raise_error("TypeError", "no implicit conversion into Integer".to_string()));
        };
        let whence = match args.get(1) {
            None => 0,
            Some(RubyValue::Int(w)) => *w,
            Some(_) => return Err(raise_error("TypeError", "no implicit conversion into Integer".to_string())),
        };
        let mut s = io_of(recv).state.lock();
        let base = match whence {
            0 => 0i64,
            1 => s.pos as i64,
            2 => s.bytes.len() as i64,
            _ => return Err(raise_error("Errno::EINVAL", "Invalid argument".to_string())),
        };
        let target = base + off;
        if target < 0 {
            return Err(raise_error("Errno::EINVAL", "Invalid argument - invalid seek".to_string()));
        }
        s.pos = target as usize;
        Ok(RubyValue::Int(0))
    }
    // `getc` -- one character (the next whole UTF-8 char), or nil at EOF.
    "getc" => fn getc(recv, args, _block) {
        arity!(args, 0);
        let mut s = io_of(recv).state.lock();
        if s.pos >= s.bytes.len() {
            return Ok(RubyValue::Nil);
        }
        let len = utf8_char_len(s.bytes[s.pos]).min(s.bytes.len() - s.pos);
        let ch = s.bytes[s.pos..s.pos + len].to_vec();
        s.pos += len;
        Ok(bytes_to_str(&ch))
    }
    // `readline(sep = "\n")` -- like `gets`, but raises `EOFError` at end.
    "readline" => fn readline(recv, args, _block) {
        arity!(args, 0..=1);
        match gets(recv, args, None)? {
            RubyValue::Nil => Err(raise_error("EOFError", "end of file reached".to_string())),
            line => Ok(line),
        }
    }
    // `readlines(sep = "\n")` -- every remaining line as an Array.
    "readlines" => fn readlines(recv, args, _block) {
        arity!(args, 0..=1);
        let mut lines = Vec::new();
        loop {
            match gets(recv, args, None)? {
                RubyValue::Nil => break,
                line => lines.push(line),
            }
        }
        Ok(RubyValue::Array(crate::array_new(lines)))
    }
    // `truncate(len)` -- resize the buffer, zero-padding when it grows.
    // Returns 0 (CRuby's IO#truncate result).
    "truncate" => fn truncate(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Int(len) = &args[0] else {
            return Err(raise_error("TypeError", "no implicit conversion into Integer".to_string()));
        };
        if *len < 0 {
            return Err(raise_error("Errno::EINVAL", "Invalid argument".to_string()));
        }
        io_of(recv).state.lock().bytes.resize(*len as usize, 0);
        Ok(RubyValue::Int(0))
    }
}

/// The byte length of a UTF-8 sequence given its leading byte (1 for ASCII or
/// an invalid lead byte, so `getc` always makes progress).
fn utf8_char_len(lead: u8) -> usize {
    match lead {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    }
}

/// `puts` for a single argument: arrays flatten (each element on its own
/// line); scalars get a trailing newline unless they already end in one.
fn puts_one(state: &mut State, v: &RubyValue) {
    if let RubyValue::Array(a) = v {
        for el in a.lock().iter() {
            puts_one(state, el);
        }
        return;
    }
    let mut b = arg_bytes(v);
    if !b.ends_with(b"\n") {
        b.push(b'\n');
    }
    write_at(state, &b);
}

/// First index of `needle` in `haystack` (empty needle never matches -- `gets`
/// treats an empty separator specially in CRuby; unsupported here, documented).
fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

builtin_methods! {
    pub(crate) fn lookup_class;

    "new" | "open" => fn new_m(_recv, args, _block) {
        arity!(args, 0..=2); // (string=""[, mode]) -- mode ignored for now
        let bytes = match args.first() {
            None | Some(RubyValue::Nil) => Vec::new(),
            Some(RubyValue::Str(s)) => s.lock().bytes().to_vec(),
            Some(other) => return Err(raise_error(
                "TypeError",
                format!("no implicit conversion of {} into String", crate::builtins::convert_name_of(other)),
            )),
        };
        Ok(RubyValue::Object(Arc::new(RStringIO::with_bytes(bytes))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(string_new(text.to_string()))
    }
    fn text(v: &RubyValue) -> String {
        match v {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn puts_then_string_matches_ruby() {
        let io = new_m(&RubyValue::Nil, &[], None).unwrap();
        puts(&io, &[s("a")], None).unwrap();
        push(&io, &[s("b")], None).unwrap();
        assert_eq!(text(&string(&io, &[], None).unwrap()), "a\nb");
    }

    #[test]
    fn read_and_rewind_track_position() {
        let io = new_m(&RubyValue::Nil, &[s("hello")], None).unwrap();
        assert_eq!(text(&read(&io, &[RubyValue::Int(3)], None).unwrap()), "hel");
        assert_eq!(text(&read(&io, &[], None).unwrap()), "lo");
        assert!(matches!(eof(&io, &[], None).unwrap(), RubyValue::Bool(true)));
        rewind(&io, &[], None).unwrap();
        assert_eq!(text(&read(&io, &[], None).unwrap()), "hello");
    }

    #[test]
    fn gets_returns_lines_then_nil() {
        let io = new_m(&RubyValue::Nil, &[s("a\nb")], None).unwrap();
        assert_eq!(text(&gets(&io, &[], None).unwrap()), "a\n");
        assert_eq!(text(&gets(&io, &[], None).unwrap()), "b");
        assert!(matches!(gets(&io, &[], None).unwrap(), RubyValue::Nil));
    }

    #[test]
    fn getc_reads_one_char_then_nil() {
        let io = new_m(&RubyValue::Nil, &[s("hé")], None).unwrap();
        assert_eq!(text(&getc(&io, &[], None).unwrap()), "h");
        assert_eq!(text(&getc(&io, &[], None).unwrap()), "é");
        assert!(matches!(getc(&io, &[], None).unwrap(), RubyValue::Nil));
    }

    #[test]
    fn seek_repositions_by_whence() {
        let io = new_m(&RubyValue::Nil, &[s("abcdef")], None).unwrap();
        seek(&io, &[RubyValue::Int(2)], None).unwrap();
        assert_eq!(text(&read(&io, &[RubyValue::Int(2)], None).unwrap()), "cd");
        seek(&io, &[RubyValue::Int(-1), RubyValue::Int(2)], None).unwrap();
        assert_eq!(text(&read(&io, &[], None).unwrap()), "f");
    }

    #[test]
    fn readlines_collects_every_line() {
        let io = new_m(&RubyValue::Nil, &[s("a\nb\nc")], None).unwrap();
        let RubyValue::Array(a) = readlines(&io, &[], None).unwrap() else {
            panic!("expected an Array")
        };
        let lines: Vec<String> = a.lock().iter().map(text).collect();
        assert_eq!(lines, vec!["a\n", "b\n", "c"]);
    }

    #[test]
    fn truncate_resizes_the_buffer() {
        let io = new_m(&RubyValue::Nil, &[s("hello world")], None).unwrap();
        truncate(&io, &[RubyValue::Int(5)], None).unwrap();
        assert_eq!(text(&string(&io, &[], None).unwrap()), "hello");
    }
}

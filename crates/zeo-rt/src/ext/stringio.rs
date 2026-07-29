//! `stringio` (CRuby's bundled `stringio` gem, a C extension) -- an in-memory
//! bytes buffer with the `IO` read/write surface, no operating-system stream
//! behind it. `require "stringio"` activates it (a built-in feature; see
//! `ext/mod.rs`).
//!
//! Backed by an `RObj` over a `Mutex<State>` (a `StringIO` is mutable and
//! shared by reference). Positions are byte offsets; writes overwrite from the
//! current position and extend the buffer, exactly like a file. The read/write
//! surface -- including `seek`/`getc`/`readline`/`readlines`/`truncate` -- is
//! oracle-verified against ruby 4.0.6.
//!
//! Documented divergence: strings are handed back as UTF-8 (lossy for non-UTF-8
//! bytes), matching this runtime's default `Str` -- CRuby's StringIO preserves
//! arbitrary bytes with an encoding.

use crate::builtins::{arity, eof_error};
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::RubyValue;
use zeo_macros::ruby_class;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zeo_abi::STRINGIO_CLASS;

struct State {
    bytes: Vec<u8>,
    /// The buffer's encoding, which every read but `read(len)` is tagged with.
    ///
    /// A `StringIO` is a byte buffer that REMEMBERS what its bytes mean: the
    /// string it was built from carries an encoding, and CRuby hands that same
    /// encoding back out of `string`/`read`/`gets`/`getc`. Without it, binary
    /// content cannot survive a round-trip -- decoding the bytes as UTF-8 to
    /// answer a read replaces every non-UTF-8 byte with U+FFFD, which is
    /// silent corruption of exactly the data a StringIO is most often used to
    /// carry (a compressed stream, an image, a socket capture).
    enc: crate::encoding::EncodingId,
    pos: usize,
    closed: bool,
}

pub struct RStringIO {
    state: Mutex<State>,
    frozen: AtomicBool,
}

impl RStringIO {
    fn with_bytes(bytes: Vec<u8>, enc: crate::encoding::EncodingId) -> RStringIO {
        RStringIO {
            state: Mutex::new(State {
                bytes,
                enc,
                pos: 0,
                closed: false,
            }),
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
        let io = RStringIO::with_bytes(s.bytes.clone(), s.enc);
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

/// Bytes out of the buffer, tagged with the buffer's own encoding -- what
/// `string`/`read`/`gets`/`getc`/`readlines` all answer.
fn bytes_to_str(bytes: &[u8], enc: crate::encoding::EncodingId) -> RubyValue {
    RubyValue::Str(crate::string_from_bytes(bytes.to_vec(), enc))
}

/// Bytes out of the buffer as ASCII-8BIT. `read(len)` is the one read that
/// ignores the buffer's encoding -- CRuby documents it as reading in binary
/// mode, because a length in BYTES can land mid-character.
fn bytes_to_binary(bytes: &[u8]) -> RubyValue {
    RubyValue::Str(crate::string_from_bytes(
        bytes.to_vec(),
        crate::encoding::ASCII_8BIT,
    ))
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

ruby_class! {
    StringIO = zeo_abi::STRINGIO_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::ENUMERABLE_CLASS;

    // The whole buffer as a String, independent of position.
    def "string" (recv, args, _block) {
        arity!(args, 0);
        let s = io_of(recv).state.lock();
        Ok(bytes_to_str(&s.bytes, s.enc))
    }
    // The IO encoding pair, as `IO` answers it: the buffer's own encoding
    // outward, and no transcoding on the way in. csv's writer reads both to
    // decide whether it must convert what it is about to emit.
    def "external_encoding" (recv, args, _block) {
        arity!(args, 0);
        crate::dispatch::send_value(
            &{ let s = io_of(recv).state.lock(); bytes_to_str(&s.bytes, s.enc) },
            crate::Symbol::intern("encoding"),
            &[],
            None,
        )
    }
    def "internal_encoding" (_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Nil)
    }
    def "read" (recv, args, _block) {
        arity!(args, 0..=1);
        let mut s = io_of(recv).state.lock();
        match args.first() {
            None | Some(RubyValue::Nil) => {
                let out = s.bytes[s.pos.min(s.bytes.len())..].to_vec();
                s.pos = s.bytes.len();
                Ok(bytes_to_str(&out, s.enc))
            }
            Some(v) => {
                let n = crate::builtins::convert::to_index(v)?;
                if n < 0 {
                    return Err(crate::builtins::arg_error!("negative length {n} given"));
                }
                let n = n as usize;
                if s.pos >= s.bytes.len() && n > 0 {
                    return Ok(RubyValue::Nil); // EOF with a nonzero length is nil
                }
                let end = (s.pos + n).min(s.bytes.len());
                let out = s.bytes[s.pos..end].to_vec();
                s.pos = end;
                Ok(bytes_to_binary(&out))
            }
        }
    }
    def "write" (recv, args, _block) {
        let mut written = 0usize;
        let mut s = io_of(recv).state.lock();
        for a in args {
            let b = arg_bytes(a);
            written += b.len();
            write_at(&mut s, &b);
        }
        Ok(RubyValue::Int(written as i64))
    }
    def "<<" (recv, args, _block) {
        arity!(args, 1);
        let mut s = io_of(recv).state.lock();
        write_at(&mut s, &arg_bytes(&args[0]));
        Ok(recv.clone())
    }
    def "print" (recv, args, _block) {
        let mut s = io_of(recv).state.lock();
        for a in args {
            write_at(&mut s, &arg_bytes(a));
        }
        Ok(RubyValue::Nil)
    }
    def "puts" (recv, args, _block) {
        let mut s = io_of(recv).state.lock();
        if args.is_empty() {
            write_at(&mut s, b"\n");
        }
        for a in args {
            puts_one(&mut s, a);
        }
        Ok(RubyValue::Nil)
    }
    def "gets" as gets (recv, args, _block) {
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
        Ok(bytes_to_str(&line, s.enc))
    }
    def "each_line" | "each" (recv, args, block) {
        arity!(args, 0);
        // Blockless, this is an Enumerator over the same lines -- Ruby's rule
        // for every `each_*`, and what `each_line.to_a` (csv's reader) needs.
        let Some(RubyValue::Proc(p)) = block else {
            return Ok(crate::builtins::enumerator::enumerator_for(recv, "each_line", &[]));
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
    def "eof?" | "eof" (recv, args, _block) {
        arity!(args, 0);
        let s = io_of(recv).state.lock();
        Ok(RubyValue::Bool(s.pos >= s.bytes.len()))
    }
    def "rewind" (recv, args, _block) {
        arity!(args, 0);
        io_of(recv).state.lock().pos = 0;
        Ok(RubyValue::Int(0))
    }
    def "pos" | "tell" (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(io_of(recv).state.lock().pos as i64))
    }
    def "pos=" (recv, args, _block) {
        arity!(args, 1);
        let n = &crate::builtins::convert::to_index(&args[0])?;
        io_of(recv).state.lock().pos = (*n).max(0) as usize;
        Ok(args[0].clone())
    }
    def "size" | "length" (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(io_of(recv).state.lock().bytes.len() as i64))
    }
    def "close" (recv, args, _block) {
        arity!(args, 0);
        io_of(recv).state.lock().closed = true;
        Ok(RubyValue::Nil)
    }
    def "closed?" (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(io_of(recv).state.lock().closed))
    }

    // `seek(offset, whence = SEEK_SET)` -- reposition; whence 0/1/2 =
    // absolute/relative/from-end. Returns 0, like CRuby's IO#seek.
    def "seek" (recv, args, _block) {
        arity!(args, 1..=2);
        let off = &crate::builtins::convert::to_index(&args[0])?;
        let whence = match args.get(1) {
            None => 0,
            Some(w) => crate::builtins::convert::to_index(w)?,
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
    def "getc" (recv, args, _block) {
        arity!(args, 0);
        let mut s = io_of(recv).state.lock();
        if s.pos >= s.bytes.len() {
            return Ok(RubyValue::Nil);
        }
        let len = char_len(s.enc, s.bytes[s.pos]).min(s.bytes.len() - s.pos);
        let ch = s.bytes[s.pos..s.pos + len].to_vec();
        s.pos += len;
        Ok(bytes_to_str(&ch, s.enc))
    }
    // `readline(sep = "\n")` -- like `gets`, but raises `EOFError` at end.
    def "readline" (recv, args, _block) {
        arity!(args, 0..=1);
        match gets(recv, args, None)? {
            RubyValue::Nil => Err(eof_error!("end of file reached")),
            line => Ok(line),
        }
    }
    // `readlines(sep = "\n")` -- every remaining line as an Array.
    def "readlines" (recv, args, _block) {
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
    def "truncate" (recv, args, _block) {
        arity!(args, 1);
        let len = &crate::builtins::convert::to_index(&args[0])?;
        if *len < 0 {
            return Err(raise_error("Errno::EINVAL", "Invalid argument".to_string()));
        }
        io_of(recv).state.lock().bytes.resize(*len as usize, 0);
        Ok(RubyValue::Int(0))
    }

    def self."new" | "open" (_recv, args, _block) {
        arity!(args, 0..=2); // (string=""[, mode]) -- mode ignored for now
        // An empty `StringIO.new` is UTF-8, as the `""` it stands in for is.
        let (bytes, enc) = match args.first() {
            None | Some(RubyValue::Nil) => (Vec::new(), crate::encoding::UTF_8),
            Some(v) => {
                let s = crate::builtins::convert::to_rstr(v)?;
                let s = s.lock();
                (s.bytes().to_vec(), s.encoding())
            }
        };
        Ok(RubyValue::Object(Arc::new(RStringIO::with_bytes(bytes, enc))))
    }
}

/// How many bytes the character starting with `lead` occupies in `enc`. Only
/// UTF-8 is multi-byte here; in a binary buffer every byte is its own
/// character, which is why `getc` on one answers a single byte.
fn char_len(enc: crate::encoding::EncodingId, lead: u8) -> usize {
    if enc == crate::encoding::UTF_8 {
        utf8_char_len(lead)
    } else {
        1
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::string_new;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(string_new(text.to_string()))
    }
    fn text(v: &RubyValue) -> String {
        match v {
            RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
            other => panic!("expected Str, got {other:?}"),
        }
    }
    /// `StringIO`'s `ruby_class!`-generated methods have mangled Rust idents, so
    /// the tests reach them through the registered instance/class tables.
    fn tbl() -> &'static crate::builtins::BuiltinClassTable {
        crate::builtins::registered_table(zeo_abi::STRINGIO_CLASS)
            .expect("StringIO is a registered builtin table")
    }
    fn im(name: &str) -> crate::builtins::BuiltinMethodFn {
        let t = tbl().instance.as_ref().expect("StringIO has instance methods");
        (t.lookup)(name).unwrap_or_else(|| panic!("StringIO#{name} is defined"))
    }
    fn cm(name: &str) -> crate::builtins::BuiltinMethodFn {
        let t = tbl().class.as_ref().expect("StringIO has class methods");
        (t.lookup)(name).unwrap_or_else(|| panic!("StringIO.{name} is defined"))
    }

    #[test]
    fn puts_then_string_matches_ruby() {
        let io = cm("new")(&RubyValue::Nil, &[], None).unwrap();
        im("puts")(&io, &[s("a")], None).unwrap();
        im("<<")(&io, &[s("b")], None).unwrap();
        assert_eq!(text(&im("string")(&io, &[], None).unwrap()), "a\nb");
    }

    #[test]
    fn read_and_rewind_track_position() {
        let io = cm("new")(&RubyValue::Nil, &[s("hello")], None).unwrap();
        assert_eq!(text(&im("read")(&io, &[RubyValue::Int(3)], None).unwrap()), "hel");
        assert_eq!(text(&im("read")(&io, &[], None).unwrap()), "lo");
        assert!(matches!(
            im("eof?")(&io, &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
        im("rewind")(&io, &[], None).unwrap();
        assert_eq!(text(&im("read")(&io, &[], None).unwrap()), "hello");
    }

    #[test]
    fn gets_returns_lines_then_nil() {
        let io = cm("new")(&RubyValue::Nil, &[s("a\nb")], None).unwrap();
        assert_eq!(text(&im("gets")(&io, &[], None).unwrap()), "a\n");
        assert_eq!(text(&im("gets")(&io, &[], None).unwrap()), "b");
        assert!(matches!(im("gets")(&io, &[], None).unwrap(), RubyValue::Nil));
    }

    /// A StringIO over BINARY content must hand the same bytes back. Decoding
    /// them as UTF-8 to answer a read replaced every non-UTF-8 byte with
    /// U+FFFD -- silent corruption, and the reason a gzip member could not
    /// survive a `StringIO` round-trip. Byte 0x8b is a gzip header byte and
    /// is not valid UTF-8 on its own, which is what makes it the case to pin.
    #[test]
    fn binary_content_survives_a_round_trip() {
        let raw: Vec<u8> = vec![0x1f, 0x8b, 0x08, 0x00, 0xc8];
        let binary = RubyValue::Str(crate::string_from_bytes(
            raw.clone(),
            crate::encoding::ASCII_8BIT,
        ));
        let io = cm("new")(&RubyValue::Nil, &[binary], None).unwrap();

        for method in ["string", "read"] {
            let got = im(method)(&io, &[], None).unwrap();
            let RubyValue::Str(s) = &got else {
                panic!("{method} should answer a String, got {got:?}");
            };
            assert_eq!(s.lock().bytes(), &raw[..], "{method} corrupted the bytes");
            assert_eq!(s.lock().encoding(), crate::encoding::ASCII_8BIT);
            im("rewind")(&io, &[], None).unwrap();
        }

        // `read(len)` is the one read CRuby answers in binary regardless, and
        // a byte count may land mid-character -- so it must not re-encode.
        let head = im("read")(&io, &[RubyValue::Int(2)], None).unwrap();
        let RubyValue::Str(head) = head else { panic!("read(2) should answer a String") };
        assert_eq!(head.lock().bytes(), &raw[..2]);

        // In a binary buffer every byte is its own character.
        im("rewind")(&io, &[], None).unwrap();
        let ch = im("getc")(&io, &[], None).unwrap();
        let RubyValue::Str(ch) = ch else { panic!("getc should answer a String") };
        assert_eq!(ch.lock().bytes(), &[0x1f]);
    }

    /// The other half of the same rule: a UTF-8 buffer keeps ITS encoding, and
    /// `getc` there walks whole characters rather than bytes.
    #[test]
    fn utf8_content_keeps_its_encoding_and_char_boundaries() {
        let io = cm("new")(
            &RubyValue::Nil,
            &[RubyValue::Str(string_new("\u{e9}a".to_string()))],
            None,
        )
        .unwrap();
        let ch = im("getc")(&io, &[], None).unwrap();
        let RubyValue::Str(ch) = ch else { panic!("getc should answer a String") };
        assert_eq!(ch.lock().bytes(), "\u{e9}".as_bytes());
        assert_eq!(ch.lock().encoding(), crate::encoding::UTF_8);
    }

    #[test]
    fn getc_reads_one_char_then_nil() {
        let io = cm("new")(&RubyValue::Nil, &[s("hé")], None).unwrap();
        assert_eq!(text(&im("getc")(&io, &[], None).unwrap()), "h");
        assert_eq!(text(&im("getc")(&io, &[], None).unwrap()), "é");
        assert!(matches!(im("getc")(&io, &[], None).unwrap(), RubyValue::Nil));
    }

    #[test]
    fn seek_repositions_by_whence() {
        let io = cm("new")(&RubyValue::Nil, &[s("abcdef")], None).unwrap();
        im("seek")(&io, &[RubyValue::Int(2)], None).unwrap();
        assert_eq!(text(&im("read")(&io, &[RubyValue::Int(2)], None).unwrap()), "cd");
        im("seek")(&io, &[RubyValue::Int(-1), RubyValue::Int(2)], None).unwrap();
        assert_eq!(text(&im("read")(&io, &[], None).unwrap()), "f");
    }

    #[test]
    fn readlines_collects_every_line() {
        let io = cm("new")(&RubyValue::Nil, &[s("a\nb\nc")], None).unwrap();
        let RubyValue::Array(a) = im("readlines")(&io, &[], None).unwrap() else {
            panic!("expected an Array")
        };
        let lines: Vec<String> = a.lock().iter().map(text).collect();
        assert_eq!(lines, vec!["a\n", "b\n", "c"]);
    }

    #[test]
    fn truncate_resizes_the_buffer() {
        let io = cm("new")(&RubyValue::Nil, &[s("hello world")], None).unwrap();
        im("truncate")(&io, &[RubyValue::Int(5)], None).unwrap();
        assert_eq!(text(&im("string")(&io, &[], None).unwrap()), "hello");
    }
}

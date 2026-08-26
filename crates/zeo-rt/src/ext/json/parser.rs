//! The json gem's parser, by recursive descent, straight into `RubyValue`.
//!
//! Not serde_json. The json gem parses a DIALECT, not RFC 8259, and the
//! differences are not options serde exposes at any level: comments, a
//! trailing comma, `NaN`/`Infinity`, `-0` as an Integer, an integer past
//! `u64` kept exact, `max_nesting`, and `object_class:`/`array_class:`/
//! `decimal_class:` construction. There is no event layer low enough to fix
//! those from above, so this owns the whole parse -- and owning it means
//! owning CRuby's own error texts, which were the "decided substitution"
//! the module doc used to record.
//!
//! Every rule here was probed against ruby 4.0.6's json 2.21.2 rather than
//! read from a spec, and `tests/json_parser_edge_cases.rb` is the probe made
//! permanent. The ones worth naming:
//!
//!   * A STRING IS BYTES. Invalid UTF-8 inside a string passes THROUGH --
//!     `JSON.parse("[\"\\xFF\"]")` answers a String holding that byte. An
//!     earlier draft built a Rust `String` and replaced it with U+FFFD,
//!     which is silent corruption of exactly the shape this project forbids.
//!   * A lone LOW surrogate is legal and encodes as its three bytes; a lone
//!     HIGH surrogate is `incomplete surrogate pair`. The asymmetry is the
//!     gem's, and it is what `"\udc00"` answering `"\xED\xB0\x80"` means.
//!   * NESTING is checked when a container's CONTENT is about to be parsed,
//!     so an empty innermost container is free: `max_nesting: 2` accepts
//!     `[[[]]]` and refuses `[[[1]]]` with `nesting of 3 is too deep`. The
//!     limit is SIGNED, because `max_nesting: -1` refuses at depth 1.
//!   * A number classifies by its LITERAL SPAN, not by its value. The
//!     integer form goes to `Int` -- or to a real BigInt past `i64`, so
//!     `123456789012345678901234567890` stays exact -- and `-0` is
//!     `Int(0)` while `-0.0` is a Float.
//!   * `decimal_class:` follows CRuby's protocol exactly: `try_convert` if
//!     the class answers to it, else `new`, else -- for a class whose name
//!     has no `::` -- the bare KERNEL METHOD of the same name. That last
//!     arm is how `BigDecimal`, which answers to neither, lands on
//!     `Kernel#BigDecimal("1.5")`.

use crate::collections::{array_new, hash_new};
use crate::dispatch::raise_error;
use crate::{RubyValue, Signal};

/// How much of the input a message quotes back. The gem's own cap, measured.
const QUOTE: usize = 32;

/// The depth this descent refuses at NO MATTER what `max_nesting` says.
///
/// A DIVERGENCE, deliberately: ruby's parser keeps its own stack and reads a
/// million-deep document with `max_nesting: false`, where this one recurses
/// and would end the process. A loud `JSON::NestingError` is the safe
/// answer.
///
/// The number is MEASURED, not chosen, and it is measured on the SMALLEST
/// stack a parse can run on rather than the main thread's. A `Fiber` was
/// the case that mattered: 4,000 levels ran and 8,000 ended the process
/// without a word. This sits below the smaller of those with room for
/// whatever the program had on the stack already, and it is still twenty
/// times the gem's OWN default of 100 -- far past any document a program
/// means to write. `tests/json_nesting_is_bounded_by_the_stack.rb` records
/// the divergence.
pub(super) const STACK_CEILING: i64 = 2_000;

/// What a parse was asked for. Built once from the options Hash.
pub(super) struct Opts {
    pub symbolize: bool,
    pub freeze: bool,
    pub allow_nan: bool,
    pub allow_trailing_comma: bool,
    /// `None` = unbounded (`max_nesting: false` or `0`). Signed: a negative
    /// limit refuses at the first container, which is what ruby does.
    pub max_nesting: Option<i64>,
    pub object_class: Option<RubyValue>,
    pub array_class: Option<RubyValue>,
    pub decimal_class: Option<RubyValue>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            symbolize: false,
            freeze: false,
            allow_nan: false,
            allow_trailing_comma: false,
            max_nesting: Some(100),
            object_class: None,
            array_class: None,
            decimal_class: None,
        }
    }
}

pub(super) struct Parser<'a> {
    src: &'a [u8],
    at: usize,
    depth: i64,
    opts: &'a Opts,
}

/// The `(line, column)` a message reports.
///
/// Counted the way the gem's C parser counts: the line is one plus the
/// newlines up to AND INCLUDING `at`, and the column is `at + 1` less the
/// start of that line. So a raw newline inside a string reports `line 2
/// column 0` -- the byte IS the line break, and it belongs to neither side.
fn line_col(src: &[u8], at: usize) -> (usize, usize) {
    let upto = (at + 1).min(src.len());
    let head = &src[..upto];
    let line = 1 + head.iter().filter(|&&b| b == b'\n').count();
    let bol = head.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    (line, (at + 1).saturating_sub(bol))
}

/// Up to [`QUOTE`] bytes from `at`, as lossy text -- what the gem quotes
/// back. It quotes the REST of the input from the offending byte, capped,
/// not just the one byte.
fn snippet(src: &[u8], at: usize) -> String {
    let start = at.min(src.len());
    let end = (start + QUOTE).min(src.len());
    let run = &src[start..end];
    // Stops at a NUL: the gem hands a C string to its formatter, so a NUL
    // ends the quoted text -- which is why a stray NUL after a document
    // reports an EMPTY one.
    let run = match run.iter().position(|&b| b == 0) {
        Some(i) => &run[..i],
        None => run,
    };
    String::from_utf8_lossy(run).into_owned()
}

/// [`snippet`] with the gem's quoting: what is there in single quotes, and
/// nothing at all when the run is empty.
fn quoted(src: &[u8], at: usize) -> String {
    match snippet(src, at) {
        s if s.is_empty() => String::new(),
        s => format!("'{s}'"),
    }
}

impl<'a> Parser<'a> {
    pub(super) fn new(src: &'a [u8], opts: &'a Opts) -> Parser<'a> {
        Parser {
            src,
            at: 0,
            depth: 0,
            opts,
        }
    }

    fn err(&self, msg: impl std::fmt::Display, at: usize) -> Signal {
        let (line, col) = line_col(self.src, at);
        raise_error(
            "JSON::ParserError",
            format!("{msg} at line {line} column {col}"),
        )
    }

    /// The truncation message every unterminated STRING reports.
    fn unclosed_string(&self) -> Signal {
        self.err(
            "unexpected end of input, expected closing \"",
            self.src.len(),
        )
    }

    fn incomplete_escape(&self, esc: usize) -> Signal {
        self.err(
            format!(
                "incomplete unicode character escape sequence at '{}'",
                snippet(self.src, esc)
            ),
            esc,
        )
    }

    fn nesting_err(&self) -> Signal {
        raise_error(
            "JSON::NestingError",
            format!("nesting of {} is too deep", self.depth),
        )
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.at).copied()
    }

    /// Whitespace AND comments. The gem accepts `//` to end of line and
    /// `/* */` anywhere whitespace is allowed -- before the document,
    /// between tokens, and after it. An UNTERMINATED block comment is an
    /// error, not an early end.
    fn skip_space(&mut self) -> Result<(), Signal> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.at += 1,
                Some(b'/') if self.src.get(self.at + 1) == Some(&b'/') => {
                    self.at += 2;
                    while let Some(b) = self.peek() {
                        self.at += 1;
                        if b == b'\n' {
                            break;
                        }
                    }
                }
                Some(b'/') if self.src.get(self.at + 1) == Some(&b'*') => {
                    let open = self.at;
                    self.at += 2;
                    loop {
                        if self.at >= self.src.len() {
                            return Err(
                                self.err("unterminated comment, expected closing '*/'", open)
                            );
                        }
                        if self.src[self.at] == b'*' && self.src.get(self.at + 1) == Some(&b'/') {
                            self.at += 2;
                            break;
                        }
                        self.at += 1;
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    /// The whole document: one value, then nothing but space and comments.
    pub(super) fn parse_document(&mut self) -> Result<RubyValue, Signal> {
        self.skip_space()?;
        if self.at >= self.src.len() {
            // AFTER the skip: a document of nothing but comments reports
            // where the comments ended, not byte zero.
            return Err(self.err("unexpected end of input", self.at));
        }
        let v = self.parse_value()?;
        self.skip_space()?;
        if self.at < self.src.len() {
            return Err(self.err(
                format!(
                    "unexpected token at end of stream {}",
                    quoted(self.src, self.at)
                ),
                self.at,
            ));
        }
        if self.opts.freeze {
            super::freeze_tree(&v);
        }
        Ok(v)
    }

    fn parse_value(&mut self) -> Result<RubyValue, Signal> {
        self.skip_space()?;
        match self.peek() {
            None => Err(self.err("unexpected end of input", self.src.len())),
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => {
                let bytes = self.parse_string()?;
                Ok(RubyValue::Str(crate::string_from_bytes(
                    bytes,
                    crate::encoding::UTF_8,
                )))
            }
            Some(b't') => self.keyword(b"true", RubyValue::Bool(true)),
            Some(b'f') => self.keyword(b"false", RubyValue::Bool(false)),
            Some(b'n') => self.keyword(b"null", RubyValue::Nil),
            Some(b'N') if self.opts.allow_nan => self.keyword(b"NaN", RubyValue::Float(f64::NAN)),
            Some(b'I') if self.opts.allow_nan => {
                self.keyword(b"Infinity", RubyValue::Float(f64::INFINITY))
            }
            // `-Infinity` opens like a NUMBER, so it reports as an invalid
            // number rather than as a stray token when refused.
            Some(b'-') if self.src.get(self.at + 1) == Some(&b'I') => {
                if !self.opts.allow_nan {
                    return Err(self.err(
                        format!("invalid number: '{}'", snippet(self.src, self.at)),
                        self.at,
                    ));
                }
                self.at += 1;
                self.keyword(b"Infinity", RubyValue::Float(f64::NEG_INFINITY))
            }
            Some(b'-' | b'0'..=b'9') => self.parse_number(),
            // A word-shaped run is a TOKEN, anything else a CHARACTER.
            Some(b) if b.is_ascii_alphabetic() => Err(self.err(
                format!("unexpected token '{}'", snippet(self.src, self.at)),
                self.at,
            )),
            Some(_) => Err(self.err(
                format!("unexpected character: '{}'", snippet(self.src, self.at)),
                self.at,
            )),
        }
    }

    fn keyword(&mut self, word: &[u8], v: RubyValue) -> Result<RubyValue, Signal> {
        if self.src[self.at..].starts_with(word) {
            self.at += word.len();
            return Ok(v);
        }
        Err(self.err(
            format!("unexpected token '{}'", snippet(self.src, self.at)),
            self.at,
        ))
    }

    /// A container's content is what counts against `max_nesting` -- see the
    /// module doc.
    fn enter(&mut self) -> Result<(), Signal> {
        self.depth += 1;
        if self.depth > STACK_CEILING {
            return Err(self.nesting_err());
        }
        match self.opts.max_nesting {
            Some(max) if self.depth > max => Err(self.nesting_err()),
            _ => Ok(()),
        }
    }

    fn parse_array(&mut self) -> Result<RubyValue, Signal> {
        self.at += 1; // `[`
        let mut items: Vec<RubyValue> = Vec::new();
        self.skip_space()?;
        if self.peek() == Some(b']') {
            self.at += 1;
            return self.finish_array(items);
        }
        self.enter()?;
        loop {
            self.skip_space()?;
            // A trailing comma leaves the closer here.
            if self.peek() == Some(b']') && self.opts.allow_trailing_comma && !items.is_empty() {
                self.at += 1;
                break;
            }
            items.push(self.parse_value()?);
            self.skip_space()?;
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    break;
                }
                _ => return Err(self.err("expected ',' or ']' after array value", self.at)),
            }
        }
        self.depth -= 1;
        self.finish_array(items)
    }

    /// `array_class:` builds by `new` then `<<`, which is what lets an Array
    /// subclass see every element the way CRuby's parser hands them over.
    fn finish_array(&mut self, items: Vec<RubyValue>) -> Result<RubyValue, Signal> {
        let Some(cls) = &self.opts.array_class else {
            return Ok(RubyValue::Array(array_new(items)));
        };
        let out = crate::dispatch::send_value(cls, crate::Symbol::intern("new"), &[], None)?;
        for item in items {
            crate::dispatch::send_value(&out, crate::Symbol::intern("<<"), &[item], None)?;
        }
        Ok(out)
    }

    fn parse_object(&mut self) -> Result<RubyValue, Signal> {
        self.at += 1; // `{`
        let mut pairs: Vec<(RubyValue, RubyValue)> = Vec::new();
        self.skip_space()?;
        if self.peek() == Some(b'}') {
            self.at += 1;
            return self.finish_object(pairs);
        }
        self.enter()?;
        loop {
            self.skip_space()?;
            if self.peek() == Some(b'}') && self.opts.allow_trailing_comma && !pairs.is_empty() {
                self.at += 1;
                break;
            }
            if self.peek() != Some(b'"') {
                let what = match self.at >= self.src.len() {
                    true => "EOF".to_string(),
                    false => format!("'{}'", snippet(self.src, self.at)),
                };
                return Err(self.err(format!("expected object key, got {what}"), self.at));
            }
            let key = self.parse_string()?;
            let key = match self.opts.symbolize {
                true => RubyValue::Symbol(crate::Symbol::intern(&String::from_utf8_lossy(&key))),
                false => RubyValue::Str(crate::string_from_bytes(key, crate::encoding::UTF_8)),
            };
            self.skip_space()?;
            if self.peek() != Some(b':') {
                return Err(self.err("expected ':' after object key", self.at));
            }
            self.at += 1;
            pairs.push((key, self.parse_value()?));
            self.skip_space()?;
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    break;
                }
                None => {
                    return Err(
                        self.err("expected ',' or '}' after object value, got: EOF", self.at)
                    );
                }
                Some(_) => {
                    return Err(self.err(
                        format!(
                            "expected ',' or '}}' after object value, got: '{}'",
                            snippet(self.src, self.at)
                        ),
                        self.at,
                    ));
                }
            }
        }
        self.depth -= 1;
        self.finish_object(pairs)
    }

    /// `object_class:` builds by `new` then `[]=`, so an OpenStruct or a
    /// Hash subclass sees each pair as an assignment.
    fn finish_object(&mut self, pairs: Vec<(RubyValue, RubyValue)>) -> Result<RubyValue, Signal> {
        let Some(cls) = &self.opts.object_class else {
            return Ok(RubyValue::Hash(hash_new(pairs)));
        };
        let out = crate::dispatch::send_value(cls, crate::Symbol::intern("new"), &[], None)?;
        for (k, v) in pairs {
            crate::dispatch::send_value(&out, crate::Symbol::intern("[]="), &[k, v], None)?;
        }
        Ok(out)
    }

    /// A string, AS BYTES -- see the module doc for why this is not a Rust
    /// `String`.
    fn parse_string(&mut self) -> Result<Vec<u8>, Signal> {
        self.at += 1; // `"`
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(b) = self.peek() else {
                return Err(self.unclosed_string());
            };
            match b {
                b'"' => {
                    self.at += 1;
                    return Ok(out);
                }
                // A raw control byte has to be escaped. DEL (0x7F) is NOT a
                // control byte for this rule.
                b if b < 0x20 => {
                    return Err(self.err(
                        format!(
                            "invalid ASCII control character in string: {}",
                            quoted(self.src, self.at)
                        ),
                        self.at,
                    ));
                }
                b'\\' => {
                    let esc = self.at;
                    self.at += 1;
                    let Some(e) = self.peek() else {
                        return Err(self.unclosed_string());
                    };
                    self.at += 1;
                    match e {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0c),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => self.parse_escape_u(esc, &mut out)?,
                        _ => {
                            return Err(self.err(
                                format!(
                                    "invalid escape character in string: '{}'",
                                    snippet(self.src, esc)
                                ),
                                esc,
                            ));
                        }
                    }
                }
                _ => {
                    // Raw bytes pass through EXACTLY, valid UTF-8 or not.
                    let start = self.at;
                    while let Some(c) = self.peek() {
                        if c == b'"' || c == b'\\' || c < 0x20 {
                            break;
                        }
                        self.at += 1;
                    }
                    out.extend_from_slice(&self.src[start..self.at]);
                }
            }
        }
    }

    /// `\uXXXX`, including the surrogate PAIR form.
    ///
    /// A lone LOW surrogate is legal and encodes as its three bytes; a lone
    /// HIGH surrogate is an error naming the rest of the string. That
    /// asymmetry is the gem's own -- see the module doc.
    fn parse_escape_u(&mut self, esc: usize, out: &mut Vec<u8>) -> Result<(), Signal> {
        let hi = self.hex4(esc)?;
        if !(0xD800..0xDC00).contains(&hi) {
            push_cp(out, hi);
            return Ok(());
        }
        let paired = self.peek() == Some(b'\\') && self.src.get(self.at + 1) == Some(&b'u');
        if !paired {
            return Err(self.err(
                format!("incomplete surrogate pair at '{}'", snippet(self.src, esc)),
                esc,
            ));
        }
        let second = self.at;
        self.at += 2;
        let lo = self.hex4(second)?;
        if !(0xDC00..0xE000).contains(&lo) {
            return Err(self.err(
                format!(
                    "incomplete surrogate pair at '{}'",
                    snippet(self.src, second)
                ),
                second,
            ));
        }
        let cp = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
        push_cp(out, cp);
        Ok(())
    }

    fn hex4(&mut self, esc: usize) -> Result<u32, Signal> {
        if self.at + 4 > self.src.len() {
            return Err(self.incomplete_escape(esc));
        }
        let text = &self.src[self.at..self.at + 4];
        let v = std::str::from_utf8(text)
            .ok()
            .and_then(|s| u32::from_str_radix(s, 16).ok())
            .ok_or_else(|| self.incomplete_escape(esc))?;
        self.at += 4;
        Ok(v)
    }

    /// A number, classified by its LITERAL SPAN -- see the module doc.
    fn parse_number(&mut self) -> Result<RubyValue, Signal> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        let int_start = self.at;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        if self.at == int_start {
            return Err(self.bad_number(start));
        }
        // JSON forbids a leading zero on a multi-digit integer.
        if self.src[int_start] == b'0' && self.at - int_start > 1 {
            return Err(self.bad_number(start));
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.at += 1;
            let frac = self.at;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
            if self.at == frac {
                return Err(self.bad_number(start));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_float = true;
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            let exp = self.at;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
            if self.at == exp {
                return Err(self.bad_number(start));
            }
        }
        let text = String::from_utf8_lossy(&self.src[start..self.at]).into_owned();
        if !is_float {
            // Exact, however long: `int_value` keeps the canonical
            // Int-vs-BigInt invariant, so `-0` lands on `Int(0)`.
            let n: num_bigint::BigInt = text.parse().map_err(|_| self.bad_number(start))?;
            return Ok(crate::builtins::integer::int_value(n));
        }
        match &self.opts.decimal_class {
            // `parse` saturates to an infinity past f64's range, which is
            // what ruby answers for `1e400`, and to zero under it.
            None => Ok(RubyValue::Float(text.parse().unwrap_or(f64::NAN))),
            Some(cls) => decimal(cls, &text),
        }
    }

    fn bad_number(&self, start: usize) -> Signal {
        self.err(
            format!("invalid number: '{}'", snippet(self.src, start)),
            start,
        )
    }
}

/// A code point as UTF-8 bytes -- including a lone SURROGATE, which
/// `char::from_u32` refuses and the gem encodes anyway (three bytes, the
/// CESU-8 form). That is how `"\udc00"` answers `"\xED\xB0\x80"`.
fn push_cp(out: &mut Vec<u8>, cp: u32) {
    match char::from_u32(cp) {
        Some(c) => {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
        None => {
            out.push(0xE0 | ((cp >> 12) & 0x0F) as u8);
            out.push(0x80 | ((cp >> 6) & 0x3F) as u8);
            out.push(0x80 | (cp & 0x3F) as u8);
        }
    }
}

/// CRuby's `decimal_class:` protocol -- see the module doc for why the
/// third arm exists.
fn decimal(cls: &RubyValue, text: &str) -> Result<RubyValue, Signal> {
    let arg = RubyValue::Str(crate::string_new(text.to_string()));
    // Asked of the CLASS OBJECT, not of `cls.class_id()` -- that is `Class`,
    // and every class answers to `new` through it. `BigDecimal` is exactly
    // the case this has to get right: it answers to neither name, which is
    // what sends it to the Kernel method below.
    let answers = |name: &str| {
        matches!(
            crate::dispatch::send_value(
                cls,
                crate::Symbol::intern("respond_to?"),
                &[RubyValue::Symbol(crate::Symbol::intern(name))],
                None,
            ),
            Ok(RubyValue::Bool(true))
        )
    };
    if answers("try_convert") {
        return crate::dispatch::send_value(
            cls,
            crate::Symbol::intern("try_convert"),
            &[arg],
            None,
        );
    }
    if answers("new") {
        return crate::dispatch::send_value(cls, crate::Symbol::intern("new"), &[arg], None);
    }
    let name = cls.to_display_string();
    if name.contains("::") {
        return Err(raise_error(
            "JSON::ParserError",
            format!("cannot build {name} from a number"),
        ));
    }
    let main = crate::dispatch::main_object();
    crate::dispatch::send_value(&main, crate::Symbol::intern(&name), &[arg], None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A parse that SUCCEEDS. Every refusal goes through [`refuses`]
    /// instead: registry-less, `raise_error` panics rather than building an
    /// exception, which is a fact about the unit-test process and not about
    /// the parser.
    fn parse(text: &str) -> RubyValue {
        let opts = Opts::default();
        Parser::new(text.as_bytes(), &opts)
            .parse_document()
            .unwrap_or_else(|_| panic!("{text:?} parses"))
    }

    /// Whether the parser REFUSES `bytes` -- and, just as much, that it
    /// TERMINATES and reads nothing it does not own doing so. Registry-less
    /// a refusal arrives as a panic, so this catches one.
    fn refuses(bytes: &[u8]) -> bool {
        refuses_with(bytes, Opts::default())
    }

    fn refuses_with(bytes: &[u8], opts: Opts) -> bool {
        let owned = bytes.to_vec();
        let hush = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        // `Opts` holds `RubyValue`s, which are not `UnwindSafe` -- and the
        // whole point here is that a panic is caught rather than trusted, so
        // asserting it is exactly right.
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            Parser::new(&owned, &opts).parse_document().is_err()
        }));
        std::panic::set_hook(hush);
        // A panic IS the refusal here; `Ok(false)` means it parsed.
        out.unwrap_or(true)
    }

    fn first(v: &RubyValue) -> RubyValue {
        let RubyValue::Array(a) = v else {
            panic!("array")
        };
        let e = a.lock()[0].clone();
        e
    }

    #[test]
    fn a_number_classifies_by_its_literal_span() {
        assert!(matches!(first(&parse("[-0]")), RubyValue::Int(0)));
        assert!(matches!(first(&parse("[-0.0]")), RubyValue::Float(_)));
        assert!(matches!(first(&parse("[0]")), RubyValue::Int(0)));
        assert!(matches!(first(&parse("[1e2]")), RubyValue::Float(_)));
        assert!(matches!(first(&parse("[1E2]")), RubyValue::Float(_)));
        assert!(matches!(first(&parse("[1.5]")), RubyValue::Float(_)));
    }

    #[test]
    fn an_integer_past_i64_stays_exact() {
        assert_eq!(
            first(&parse("[123456789012345678901234567890]")).to_display_string(),
            "123456789012345678901234567890"
        );
        assert_eq!(
            first(&parse("[9007199254740993]")).to_display_string(),
            "9007199254740993"
        );
        assert_eq!(
            first(&parse("[-9223372036854775809]")).to_display_string(),
            "-9223372036854775809"
        );
        // 400 digits: exact, and no overflow anywhere on the way.
        let long = format!("[{}]", "9".repeat(400));
        assert_eq!(first(&parse(&long)).to_display_string().len(), 400);
    }

    #[test]
    fn an_exponent_past_f64_saturates_rather_than_failing() {
        let RubyValue::Float(f) = first(&parse("[1e400]")) else {
            panic!("float")
        };
        assert!(f.is_infinite());
        let RubyValue::Float(f) = first(&parse("[1e-400]")) else {
            panic!("float")
        };
        assert_eq!(f, 0.0);
        // An exponent that does not fit an i32, let alone an f64.
        let RubyValue::Float(f) = first(&parse("[1e99999999999999999999]")) else {
            panic!("float")
        };
        assert!(f.is_infinite());
    }

    #[test]
    fn nesting_counts_content_not_brackets() {
        let bounded = || Opts {
            max_nesting: Some(2),
            ..Opts::default()
        };
        assert!(Parser::new(b"[[[]]]", &bounded()).parse_document().is_ok());
        assert!(refuses_with(b"[[[1]]]", bounded()));
        // A NEGATIVE limit refuses at the first container.
        assert!(refuses_with(
            b"[1]",
            Opts {
                max_nesting: Some(-1),
                ..Opts::default()
            }
        ));
    }

    /// Run `f` with a stack far larger than any this could need.
    ///
    /// The two ceiling tests are about the CEILING, not about the host's
    /// stack -- and a test thread's is small, doubly so in a debug build
    /// where every frame is several times its release size. Without this
    /// the assertion never runs: the stack ends the whole test process
    /// first, which is the very thing the ceiling exists to prevent.
    /// `tests/json_nesting_is_bounded_by_the_stack.rb` is what proves the
    /// number is right for a REAL zeo program, fibers included.
    fn on_a_big_stack(f: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(f)
            .expect("spawning the test thread")
            .join()
            .expect("the test thread finished");
    }

    #[test]
    fn the_stack_ceiling_refuses_rather_than_overflowing() {
        on_a_big_stack(|| {
        let unbounded = || Opts {
            max_nesting: None,
            ..Opts::default()
        };
        let over = (STACK_CEILING + 1) as usize;
        let deep = format!("{}1{}", "[".repeat(over), "]".repeat(over));
        assert!(refuses_with(deep.as_bytes(), unbounded()));
        // ...and exactly AT the ceiling still parses.
        let at = STACK_CEILING as usize;
        let ok = format!("{}1{}", "[".repeat(at), "]".repeat(at));
        assert!(!refuses_with(ok.as_bytes(), unbounded()));
        });
    }

    #[test]
    fn comments_are_whitespace_everywhere() {
        parse("// c\n[1] /* x */");
        parse("[1 /* x */, 2]");
        parse("/* a */ [1]");
        // ...but an unterminated one is an error, not an early end.
        assert!(refuses(b"[1] /* x"));
        assert!(refuses(b"/* x"));
    }

    #[test]
    fn a_string_is_bytes_and_invalid_utf8_survives() {
        let opts = Opts::default();
        let v = Parser::new(b"[\"\xFF\"]", &opts).parse_document().unwrap();
        let RubyValue::Str(s) = first(&v) else {
            panic!("string")
        };
        assert_eq!(s.lock().bytes(), b"\xFF");
    }

    #[test]
    fn a_lone_low_surrogate_encodes_and_a_lone_high_one_refuses() {
        let RubyValue::Str(s) = parse(r#""\udc00""#) else {
            panic!("string")
        };
        assert_eq!(s.lock().bytes(), b"\xED\xB0\x80");
        assert!(refuses(br#""\ud800""#));
        assert!(refuses(br#""\ud800x""#));
        assert!(refuses(br#""\udc00\ud800""#));
        let RubyValue::Str(s) = parse(r#""\ud83d\ude00""#) else {
            panic!("string")
        };
        assert_eq!(s.lock().to_utf8_lossy(), "😀");
    }

    #[test]
    fn a_raw_control_byte_in_a_string_is_refused() {
        assert!(refuses(b"\"a\tb\""));
        assert!(refuses(b"\"a\nb\""));
        assert!(refuses(b"\"a\x00b\""));
        // DEL is not a control byte for this rule.
        assert!(!refuses(b"\"a\x7Fb\""));
    }

    #[test]
    fn a_leading_zero_is_an_invalid_number() {
        assert!(refuses(b"[01]"));
        assert!(refuses(b"[01.5]"));
        assert!(!refuses(b"[0]"));
        assert!(!refuses(b"[0.5]"));
        assert!(!refuses(b"[0e1]"));
    }

    /// Every prefix of a valid document is either parsed or refused --
    /// never an out-of-bounds read, never a hang.
    #[test]
    fn every_truncation_of_a_document_terminates() {
        let doc = br#"{"a":[1,-2.5e3,null,true,"x\u00e9\ud83d\ude00"],"b":{"c":[]}}"#;
        for i in 0..=doc.len() {
            let _ = refuses(&doc[..i]);
        }
    }

    /// Every single byte, as a whole document and in each position a
    /// document has: the parser answers or refuses, and reads nothing it
    /// does not own.
    #[test]
    fn every_stray_byte_terminates() {
        for b in 0u8..=255 {
            let _ = refuses(&[b]);
            let _ = refuses(&[b'[', b, b']']);
            let _ = refuses(&[b'"', b, b'"']);
            let _ = refuses(&[b'{', b'"', b'a', b'"', b':', b, b'}']);
            let _ = refuses(&[b'\\', b]);
            let _ = refuses(&[b'"', b'\\', b, b'"']);
            let _ = refuses(&[b'/', b]);
        }
    }

    /// A `\u` escape truncated at every position, and every byte in the
    /// hex slots.
    #[test]
    fn every_broken_unicode_escape_terminates() {
        for pre in [r#""\u"#, r#""\ud8"#, r#""\ud800\u"#] {
            for i in 0..=pre.len() {
                let _ = refuses(pre[..i].as_bytes());
            }
        }
        for b in 0u8..=255 {
            let mut s = br#""\u00"#.to_vec();
            s.extend_from_slice(&[b, b'0', b'"']);
            let _ = refuses(&s);
        }
    }

    #[test]
    fn line_and_column_count_the_way_the_gem_does() {
        assert_eq!(line_col(b"[1,]", 3), (1, 4));
        // The byte IS the line break, so it belongs to neither side.
        assert_eq!(line_col(b"\"a\nb\"", 2), (2, 0));
        assert_eq!(line_col(b"a\nbc", 3), (2, 2));
        // Past the end (every truncation message) stays in range.
        assert_eq!(line_col(b"ab", 99), (1, 100));
        assert_eq!(line_col(b"", 0), (1, 1));
    }

    #[test]
    fn a_message_quotes_at_most_thirty_two_bytes() {
        let long = "9".repeat(200);
        assert_eq!(snippet(long.as_bytes(), 0).len(), QUOTE);
        assert_eq!(snippet(b"ab", 5), "");
        assert_eq!(snippet(b"ab", 1), "b");
    }
}

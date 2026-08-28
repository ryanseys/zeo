//! The dump half: Psych's block style, hand-rolled.
//!
//! yaml-rust2's emitter cannot produce what psych produces -- it has no
//! `!ruby/range`, no `! ''` for a nil key, no `%YAML 1.1` header, no
//! identity anchors, and it indents a sequence under a mapping key one
//! level deeper than psych does. So the layout is ours, and every rule
//! below was probed against ruby 4.0.6 rather than assumed.
//!
//! The ones that are not guessable:
//!
//!   * A SEQUENCE under a mapping key stays at the KEY's indent, while a
//!     nested MAPPING indents one level in. `indentation:` moves the
//!     second and leaves the first alone.
//!   * A multi-line string is a LITERAL block (`|`, or `|-` with no
//!     trailing newline) -- unless it holds a tab, a `\r`, or a space
//!     before a newline, which libyaml cannot write as a block and
//!     downgrades to double quotes.
//!   * ANCHORS come from an identity pass, and only containers and objects
//!     get one. Two references to one String dump as two strings; two
//!     references to one Array dump as `&1` and `*1`. That pass is also
//!     the cycle fix -- a self-referential array is `--- &1\n- *1\n`
//!     rather than a recursion.

use crate::collections::hash_pairs;
use crate::{RubyValue, Signal};
use std::collections::HashMap;

/// What a dump was asked for.
pub(super) struct DumpOpts {
    /// Spaces per level for a nested MAPPING. Psych's default is 2.
    pub indentation: usize,
    /// Where a long plain scalar wraps. Psych's default is 80.
    pub line_width: usize,
    /// `%YAML 1.1` ahead of the document.
    pub header: bool,
}

impl Default for DumpOpts {
    fn default() -> Self {
        DumpOpts {
            indentation: 2,
            line_width: 80,
            header: false,
        }
    }
}

pub(super) struct Emitter<'a> {
    opts: &'a DumpOpts,
    /// Identity -> anchor number, for the values that appear more than
    /// once. Filled by [`Emitter::plan_anchors`] before anything is
    /// written.
    anchors: HashMap<usize, usize>,
    /// Which anchors have already been WRITTEN, so the second reference
    /// emits an alias.
    written: HashMap<usize, usize>,
    out: String,
}

/// The identity an anchor is keyed by: the heap address behind a
/// container, which is what `equal?` compares.
fn identity(v: &RubyValue) -> Option<usize> {
    match v {
        RubyValue::Array(a) => Some(std::sync::Arc::as_ptr(a).cast::<()>() as usize),
        RubyValue::Hash(h) => Some(std::sync::Arc::as_ptr(h).cast::<()>() as usize),
        RubyValue::Object(o) => Some(std::sync::Arc::as_ptr(o).cast::<()>() as usize),
        // A STRING is never anchored, however many times it appears --
        // psych's rule, probed.
        _ => None,
    }
}

impl<'a> Emitter<'a> {
    pub(super) fn new(opts: &'a DumpOpts) -> Emitter<'a> {
        Emitter {
            opts,
            anchors: HashMap::new(),
            written: HashMap::new(),
            out: String::new(),
        }
    }

    pub(super) fn dump(mut self, v: &RubyValue) -> Result<String, Signal> {
        let mut seen: HashMap<usize, usize> = HashMap::new();
        self.plan_anchors(v, &mut seen);
        // Numbered in FIRST-VISIT order, which is the order they are
        // written, so `&1` really is the first anchor in the text.
        let mut ordered: Vec<usize> = self
            .anchors
            .keys()
            .copied()
            .filter(|k| seen.contains_key(k))
            .collect();
        ordered.sort_by_key(|k| self.anchors[k]);
        for (n, key) in ordered.into_iter().enumerate() {
            self.anchors.insert(key, n + 1);
        }
        if self.opts.header {
            self.out.push_str("%YAML 1.1\n");
        }
        self.emit_document(v)?;
        Ok(self.out)
    }

    /// Count every container by identity, in visit order, and record the
    /// ones seen twice. Also the cycle guard: a container already being
    /// visited is not descended into again.
    fn plan_anchors(&mut self, v: &RubyValue, seen: &mut HashMap<usize, usize>) {
        let Some(id) = identity(v) else {
            return;
        };
        let count = seen.entry(id).or_insert(0);
        *count += 1;
        if *count > 1 {
            let next = self.anchors.len() + 1;
            self.anchors.entry(id).or_insert(next);
            return;
        }
        match v {
            RubyValue::Array(a) => {
                let items = a.lock().iter().cloned().collect::<Vec<_>>();
                for item in &items {
                    self.plan_anchors(item, seen);
                }
            }
            RubyValue::Hash(h) => {
                for (k, val) in hash_pairs(h) {
                    self.plan_anchors(&k, seen);
                    self.plan_anchors(&val, seen);
                }
            }
            _ => {}
        }
    }

    /// The anchor or alias marker for `v`, if it has one. `Ok(true)` from
    /// the alias arm means the value is fully written.
    fn marker(&mut self, v: &RubyValue) -> (Option<usize>, bool) {
        let Some(id) = identity(v) else {
            return (None, false);
        };
        let Some(&n) = self.anchors.get(&id) else {
            return (None, false);
        };
        match self.written.insert(id, n) {
            Some(_) => (Some(n), true),
            None => (Some(n), false),
        }
    }

    fn emit_document(&mut self, v: &RubyValue) -> Result<(), Signal> {
        let (anchor, alias) = self.marker(v);
        let head = match (anchor, alias) {
            (Some(n), true) => format!("--- *{n}\n"),
            (Some(n), false) => format!("--- &{n}\n"),
            (None, false) => "---".to_string(),
            (None, true) => unreachable!("an alias needs an anchor"),
        };
        if alias {
            self.out.push_str(&head);
            return Ok(());
        }
        if anchor.is_some() {
            self.out.push_str(&head);
            return self.emit_body(v, 0);
        }
        match v {
            RubyValue::Array(a) if !a.lock().is_empty() => {
                self.out.push_str("---\n");
                self.emit_seq(v, 0)
            }
            RubyValue::Hash(h) if crate::collections::hash_len(h) > 0 => {
                self.out.push_str("---\n");
                self.emit_map(v, 0)
            }
            RubyValue::Nil => {
                self.out.push_str("---\n");
                Ok(())
            }
            other => {
                // Indent 1: a block scalar under `--- ` writes its body one
                // level in, exactly as it would under a key.
                let (text, block) = self.scalar(other, 1)?;
                // A block scalar already ends in its own newline.
                let tail = if block { "" } else { "\n" };
                self.out.push_str(&format!("--- {text}{tail}"));
                Ok(())
            }
        }
    }

    /// A container's body, once its `---`/anchor line is written.
    fn emit_body(&mut self, v: &RubyValue, indent: usize) -> Result<(), Signal> {
        match v {
            RubyValue::Array(_) => self.emit_seq(v, indent),
            RubyValue::Hash(_) => self.emit_map(v, indent),
            other => {
                let (text, block) = self.scalar(other, indent + 1)?;
                self.out.push_str(&text);
                if !block {
                    self.out.push('\n');
                }
                Ok(())
            }
        }
    }

    fn pad(&mut self, indent: usize) {
        let width = indent * self.opts.indentation;
        self.out.push_str(&" ".repeat(width));
    }

    fn emit_seq(&mut self, v: &RubyValue, indent: usize) -> Result<(), Signal> {
        let RubyValue::Array(a) = v else {
            return Ok(());
        };
        let items: Vec<RubyValue> = a.lock().iter().cloned().collect();
        for item in &items {
            self.pad(indent);
            self.out.push('-');
            self.emit_after_dash(item, indent)?;
        }
        Ok(())
    }

    fn emit_after_dash(&mut self, v: &RubyValue, indent: usize) -> Result<(), Signal> {
        let (anchor, alias) = self.marker(v);
        if alias {
            self.out
                .push_str(&format!(" *{}\n", anchor.expect("an alias has an anchor")));
            return Ok(());
        }
        if let Some(n) = anchor {
            self.out.push_str(&format!(" &{n}\n"));
            return self.emit_body(v, indent + 1);
        }
        match v {
            _ if !is_nonempty_collection(v) => {
                let (text, block) = self.scalar(v, indent + 1)?;
                // NIL writes NOTHING, not a space and nothing -- `-\n` and
                // `a:\n` are what psych emits, with no trailing blank.
                if !text.is_empty() {
                    self.out.push(' ');
                    self.out.push_str(&text);
                }
                if !block {
                    self.out.push('\n');
                }
                Ok(())
            }
            RubyValue::Array(_) => {
                self.out.push('\n');
                self.emit_seq(v, indent + 1)
            }
            RubyValue::Hash(h) => {
                // The first pair rides the dash line; the rest indent in.
                self.out.push(' ');
                self.emit_map_from(&hash_pairs(h), indent + 1)
            }
            _ => Ok(()),
        }
    }

    fn emit_map(&mut self, v: &RubyValue, indent: usize) -> Result<(), Signal> {
        let RubyValue::Hash(h) = v else {
            return Ok(());
        };
        for pair in hash_pairs(h) {
            self.pad(indent);
            self.emit_pair(&pair, indent)?;
        }
        Ok(())
    }

    /// Like [`Emitter::emit_map`] but the FIRST pair is written where the
    /// cursor already is -- used right after a `- ` marker.
    fn emit_map_from(
        &mut self,
        pairs: &[(RubyValue, RubyValue)],
        indent: usize,
    ) -> Result<(), Signal> {
        for (i, pair) in pairs.iter().enumerate() {
            if i > 0 {
                self.pad(indent);
            }
            self.emit_pair(pair, indent)?;
        }
        Ok(())
    }

    fn emit_pair(&mut self, (k, v): &(RubyValue, RubyValue), indent: usize) -> Result<(), Signal> {
        let (key, _) = self.key_text(k)?;
        self.out.push_str(&key);
        self.out.push(':');
        let (anchor, alias) = self.marker(v);
        if alias {
            self.out
                .push_str(&format!(" *{}\n", anchor.expect("an alias has an anchor")));
            return Ok(());
        }
        if let Some(n) = anchor {
            self.out.push_str(&format!(" &{n}\n"));
            return match v {
                // A sequence stays at the KEY's indent -- psych's own
                // quirk, and it applies under an anchor too.
                RubyValue::Array(_) => self.emit_seq(v, indent),
                _ => self.emit_body(v, indent + 1),
            };
        }
        match v {
            _ if !is_nonempty_collection(v) => {
                let (text, block) = self.scalar(v, indent + 1)?;
                // NIL writes NOTHING, not a space and nothing -- `-\n` and
                // `a:\n` are what psych emits, with no trailing blank.
                if !text.is_empty() {
                    self.out.push(' ');
                    self.out.push_str(&text);
                }
                if !block {
                    self.out.push('\n');
                }
                Ok(())
            }
            RubyValue::Array(_) => {
                self.out.push('\n');
                self.emit_seq(v, indent)
            }
            RubyValue::Hash(_) => {
                self.out.push('\n');
                self.emit_map(v, indent + 1)
            }
            _ => Ok(()),
        }
    }

    /// A KEY's text. A nil key is `! ''` -- psych's spelling for "the empty
    /// scalar, and I mean it".
    fn key_text(&mut self, k: &RubyValue) -> Result<(String, bool), Signal> {
        if matches!(k, RubyValue::Nil) {
            return Ok(("! ''".to_string(), false));
        }
        self.scalar(k, 0)
    }

    /// One scalar's text. The bool says the text ALREADY ends in a newline
    /// (a block scalar does).
    fn scalar(&mut self, v: &RubyValue, indent: usize) -> Result<(String, bool), Signal> {
        // A block scalar's body sits one LEVEL in from the line that opens
        // it -- `indent` is already that level, so it is not incremented
        // again here.
        let pad = " ".repeat(indent * self.opts.indentation);
        match v {
            RubyValue::Nil => Ok((String::new(), false)),
            RubyValue::Bool(b) => Ok((b.to_string(), false)),
            RubyValue::Int(_) | RubyValue::BigInt(_) => Ok((v.to_display_string(), false)),
            RubyValue::Float(f) => Ok((float_text(*f), false)),
            RubyValue::Symbol(s) => Ok((format!(":{}", s.name()), false)),
            RubyValue::Str(s) => {
                let g = s.lock();
                let bytes = g.bytes().to_vec();
                drop(g);
                Ok(string_scalar(&bytes, &pad, self.opts.line_width))
            }
            RubyValue::Array(_) => Ok(("[]".to_string(), false)),
            RubyValue::Hash(_) => Ok(("{}".to_string(), false)),
            other => self.object_scalar(other, indent),
        }
    }

    /// The values psych writes with a tag or a native spelling of their
    /// own: Time, Date, DateTime and Range.
    fn object_scalar(&mut self, v: &RubyValue, indent: usize) -> Result<(String, bool), Signal> {
        let class = crate::dispatch::class_name(v.class_id()).unwrap_or_default();
        match class.as_str() {
            "Time" => Ok((time_text(v)?, false)),
            "Date" => Ok((send_to_s(v)?, false)),
            "DateTime" => Ok((
                format!("!ruby/object:DateTime {}", datetime_text(v)?),
                false,
            )),
            "Range" => {
                let begin = self.send_scalar(v, "begin", indent)?;
                let end = self.send_scalar(v, "end", indent)?;
                let excl = crate::dispatch::send_value(
                    v,
                    crate::Symbol::intern("exclude_end?"),
                    &[],
                    None,
                )?;
                // A `!ruby/range`'s fields sit at the tag's OWN level, not
                // one in -- `--- !ruby/range` puts `begin:` at column 0.
                let pad = " ".repeat(indent.saturating_sub(1) * self.opts.indentation);
                Ok((
                    format!(
                        "!ruby/range\n{pad}begin: {begin}\n{pad}end: {end}\n{pad}excl: {}\n",
                        excl.truthy()
                    ),
                    true,
                ))
            }
            _ => Ok((single_quoted(&v.to_display_string()), false)),
        }
    }

    fn send_scalar(
        &mut self,
        v: &RubyValue,
        name: &str,
        indent: usize,
    ) -> Result<String, Signal> {
        let got = crate::dispatch::send_value(v, crate::Symbol::intern(name), &[], None)?;
        Ok(self.scalar(&got, indent)?.0)
    }
}

fn send_to_s(v: &RubyValue) -> Result<String, Signal> {
    let s = crate::dispatch::send_value(v, crate::Symbol::intern("to_s"), &[], None)?;
    Ok(s.to_display_string())
}

/// `2001-02-03 04:05:06.000000000 Z` -- psych's timestamp spelling, always
/// in UTC with nanosecond precision.
fn time_text(v: &RubyValue) -> Result<String, Signal> {
    let utc = crate::dispatch::send_value(v, crate::Symbol::intern("utc"), &[], None)?;
    let fmt = RubyValue::Str(crate::string_new(
        "%Y-%m-%d %H:%M:%S.%9N Z".to_string(),
    ));
    let s = crate::dispatch::send_value(&utc, crate::Symbol::intern("strftime"), &[fmt], None)?;
    Ok(s.to_display_string())
}

fn datetime_text(v: &RubyValue) -> Result<String, Signal> {
    let fmt = RubyValue::Str(crate::string_new(
        "%Y-%m-%d %H:%M:%S.%9N Z".to_string(),
    ));
    let s = crate::dispatch::send_value(v, crate::Symbol::intern("strftime"), &[fmt], None)?;
    Ok(s.to_display_string())
}

fn is_nonempty_collection(v: &RubyValue) -> bool {
    match v {
        RubyValue::Array(a) => !a.lock().is_empty(),
        RubyValue::Hash(h) => crate::collections::hash_len(h) > 0,
        _ => false,
    }
}

/// A Float the way psych writes one: `.inf`, `.nan`, and otherwise ruby's
/// own `to_s`.
fn float_text(f: f64) -> String {
    if f.is_nan() {
        return ".nan".to_string();
    }
    if f.is_infinite() {
        return match f > 0.0 {
            true => ".inf".to_string(),
            false => "-.inf".to_string(),
        };
    }
    RubyValue::Float(f).to_display_string()
}

/// A String as a YAML scalar, choosing among psych's five styles.
///
/// `pad` is the indent a block scalar's lines carry, and `width` is where
/// a long plain scalar wraps. The bool says the text already ends in a
/// newline.
fn string_scalar(bytes: &[u8], pad: &str, width: usize) -> (String, bool) {
    // Not text at all: psych writes the bytes as base64 under `!binary`.
    let Ok(text) = std::str::from_utf8(bytes) else {
        return (binary_block(bytes, pad), true);
    };
    if text.is_empty() {
        return ("''".to_string(), false);
    }
    if text.contains('\n') {
        // A literal block cannot carry a tab, a `\r`, or a space before a
        // line break -- libyaml downgrades all three to double quotes.
        let downgrade = text.contains('\t')
            || text.contains('\r')
            || text.split('\n').any(|line| line.ends_with(' '))
            || text.starts_with(' ');
        if downgrade {
            return (double_quoted(text), false);
        }
        let chomp = if text.ends_with('\n') { "|" } else { "|-" };
        let body: String = text
            .trim_end_matches('\n')
            .split('\n')
            .map(|line| format!("{pad}{line}\n"))
            .collect();
        return (format!("{chomp}\n{body}"), true);
    }
    if needs_double_quotes(text) {
        return (double_quoted(text), false);
    }
    if is_plain_safe(text) {
        return (wrap_plain(text, pad, width), false);
    }
    (single_quoted(text), false)
}

/// `!binary |-` plus base64, wrapped the way psych wraps it.
fn binary_block(bytes: &[u8], pad: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut b64 = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let chars = [
            TABLE[(n >> 18) as usize & 63],
            TABLE[(n >> 12) as usize & 63],
            TABLE[(n >> 6) as usize & 63],
            TABLE[n as usize & 63],
        ];
        for (i, c) in chars.iter().enumerate() {
            match i <= chunk.len() {
                true => b64.push(*c as char),
                false => b64.push('='),
            }
        }
    }
    let body: String = b64
        .as_bytes()
        .chunks(60)
        .map(|c| format!("{pad}{}\n", String::from_utf8_lossy(c)))
        .collect();
    format!("!binary |-\n{body}")
}

/// A long PLAIN scalar folds at a space, so no line runs past `width`.
fn wrap_plain(text: &str, pad: &str, width: usize) -> String {
    if text.len() <= width || !text.contains(' ') {
        return text.to_string();
    }
    let mut out = String::new();
    let mut line = 0usize;
    for word in text.split(' ') {
        if line > 0 && line + 1 + word.len() > width {
            out.push('\n');
            out.push_str(pad);
            line = pad.len();
        } else if line > 0 {
            out.push(' ');
            line += 1;
        }
        out.push_str(word);
        line += word.len();
    }
    out
}

/// Whether the text has to be DOUBLE quoted -- the only style that can
/// carry an escape.
fn needs_double_quotes(text: &str) -> bool {
    text.chars()
        .any(|c| c.is_control() || c == '\u{85}' || c == '\u{2028}')
        || text.starts_with(' ')
        || text.ends_with(' ')
        || text.starts_with('-')
        || text.starts_with('@')
        || text.starts_with('`')
}

fn double_quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn single_quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// Whether `text` can be a plain (unquoted) scalar without being misread
/// as a number, a boolean, a date, or block structure.
fn is_plain_safe(text: &str) -> bool {
    if matches!(super::scanner::resolve(text), super::scanner::Scalar::Plain(RubyValue::Str(_))) {
        // Not a number, a boolean or a null -- but the STRUCTURE rules
        // still apply below.
    } else {
        return false;
    }
    let first_ok = text
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '/');
    let body_ok = text
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '_' | ' ' | '/' | '.' | '-' | '+' | '(' | ')'));
    let edge_ok = !text.starts_with(' ') && !text.ends_with(' ');
    let no_colon = !text.contains(": ") && !text.ends_with(':');
    first_ok && body_ok && edge_ok && no_colon
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_encoder_it_has_to_round_trip_with() {
        assert_eq!(binary_block(b"hi", ""), "!binary |-\naGk=\n");
        assert_eq!(binary_block(b"\xFF\x00", ""), "!binary |-\n/wA=\n");
        assert_eq!(binary_block(b"\xFF", ""), "!binary |-\n/w==\n");
        assert_eq!(binary_block(b"abc", ""), "!binary |-\nYWJj\n");
        // Nothing it is handed can panic it.
        for len in 0..8 {
            let bytes: Vec<u8> = (0..len).map(|i| i as u8).collect();
            let _ = binary_block(&bytes, "  ");
        }
    }

    #[test]
    fn a_multiline_string_is_a_block_unless_it_cannot_be() {
        assert_eq!(string_scalar(b"a\nb\n", "  ", 80).0, "|\n  a\n  b\n");
        assert_eq!(string_scalar(b"a\nb", "  ", 80).0, "|-\n  a\n  b\n");
        // A tab, a `\r`, or a space before the break: double quotes.
        assert!(string_scalar(b"a\tb\nc", "  ", 80).0.starts_with('"'));
        assert!(string_scalar(b"a\r\nb", "  ", 80).0.starts_with('"'));
        assert!(string_scalar(b"a \nb", "  ", 80).0.starts_with('"'));
    }

    #[test]
    fn a_string_that_reads_as_something_else_is_quoted() {
        for s in ["true", "017", "1.5", "null", "~", "yes", "2001-12-14", "1:02"] {
            let out = string_scalar(s.as_bytes(), "", 80).0;
            assert!(out.starts_with('\'') || out.starts_with('"'), "{s} -> {out}");
        }
        // ...and one that reads as itself is plain, non-ASCII included.
        for s in ["hello", "héllo", "a b", "x/y", "v1.2"] {
            assert_eq!(string_scalar(s.as_bytes(), "", 80).0, s);
        }
    }

    #[test]
    fn a_long_plain_scalar_folds_at_a_space() {
        let text = "word ".repeat(20);
        let text = text.trim_end();
        let out = wrap_plain(text, "  ", 20);
        assert!(out.lines().count() > 2);
        assert!(out.lines().all(|l| l.len() <= 20));
        // Nothing to fold at: left alone rather than broken mid-word.
        assert_eq!(wrap_plain(&"x".repeat(100), "  ", 20), "x".repeat(100));
    }

    #[test]
    fn floats_take_yamls_own_spellings() {
        assert_eq!(float_text(f64::INFINITY), ".inf");
        assert_eq!(float_text(f64::NEG_INFINITY), "-.inf");
        assert_eq!(float_text(f64::NAN), ".nan");
        assert_eq!(float_text(1.0), "1.0");
        assert_eq!(float_text(-0.0), "-0.0");
    }
}

//! `String`'s search-and-replace half: the indexing engines behind
//! `[]`/`slice!`/`[]=`, `sub`/`gsub` and their replacement expansion, and
//! padding. The `ruby_class!` rows stay in `mod.rs` and call these by bare
//! name.

use super::*;

/// `String#[]`/`slice` with a Regexp: the whole match or a named/numbered
/// capture group. `None` group arg means the whole match.
pub(super) fn regexp_index(
    re: &crate::RRegexp,
    text: &str,
    enc: crate::encoding::EncodingId,
    group: Option<&RubyValue>,
) -> Result<RubyValue, Signal> {
    let RubyValue::MatchData(m) = crate::regexp_match(re, text, enc)? else {
        return Ok(RubyValue::Nil);
    };
    match group {
        None => Ok(crate::matchdata_group(&m, 0)),
        Some(RubyValue::Int(n)) => Ok(crate::matchdata_group(&m, *n)),
        Some(RubyValue::Str(name)) => {
            crate::matchdata_group_by_name(&m, &name.lock().to_utf8_lossy())
        }
        Some(RubyValue::Symbol(s)) => crate::matchdata_group_by_name(&m, &s.name()),
        _ => Ok(RubyValue::Nil),
    }
}

/// `String#slice!`: removes the matched span from `recv` in place and returns
/// it. Supports the `(index[, len])` / `(range)` / `(substring)` forms
/// (Regexp/`slice!` is a documented gap).
pub(super) fn slice_bang_impl(
    recv: &RubyValue,
    index: &RubyValue,
    len: Option<&RubyValue>,
) -> Result<RubyValue, Signal> {
    let husk = crate::regexp::husk_payload(index);
    let index = husk.as_ref().unwrap_or(index);
    let handle = recv_str!(recv);
    let mut chars: Vec<char> = handle.lock().char_vec();
    let n = chars.len() as i64;
    let norm = |i: i64| if i < 0 { i + n } else { i };
    // Resolve the [start, end) character span to remove.
    let (start, end) = match (index, len) {
        (RubyValue::Int(i), Some(RubyValue::Int(len))) => {
            let start = norm(*i);
            if start < 0 || start > n || *len < 0 {
                return Ok(RubyValue::Nil);
            }
            (start as usize, (start + *len).min(n) as usize)
        }
        (RubyValue::Int(i), None) => {
            let start = norm(*i);
            if start < 0 || start >= n {
                return Ok(RubyValue::Nil);
            }
            (start as usize, (start + 1) as usize)
        }
        (RubyValue::Range(__rg), None) => {
            let (s, e, exclusive) = __rg.parts();
            // `rb_range_beg_len` converts both endpoints before it measures
            // the string, so an endpoint that is not an index RAISES rather
            // than answering nil (a bignum RangeError, a String TypeError).
            let bound = |v: Option<&RubyValue>| match v {
                None | Some(RubyValue::Nil) => Ok(None),
                Some(v) => crate::builtins::convert::to_index(v).map(Some),
            };
            let start = match bound(s)? {
                Some(v) => norm(v),
                None => 0,
            };
            let end = match bound(e)? {
                Some(v) => {
                    let v = norm(v);
                    if exclusive { v } else { v + 1 }
                }
                None => n,
            };
            if start < 0 || start > n {
                return Ok(RubyValue::Nil);
            }
            (start as usize, end.clamp(start, n) as usize)
        }
        (RubyValue::Str(sub), None) => {
            let needle: Vec<char> = sub.lock().to_utf8_lossy().chars().collect();
            match find_subslice(&chars, &needle) {
                Some(pos) => (pos, pos + needle.len()),
                None => return Ok(RubyValue::Nil),
            }
        }
        // `slice!(regexp)` / `slice!(regexp, capture)`: remove and return the
        // whole match (or the named/numbered capture group).
        (RubyValue::Regexp(re), cap) => {
            let text: String = chars.iter().collect();
            let enc = handle.lock().encoding();
            let RubyValue::MatchData(m) = crate::regexp_match(re, &text, enc)? else {
                return Ok(RubyValue::Nil);
            };
            let key = cap.cloned().unwrap_or(RubyValue::Int(0));
            let RubyValue::Array(off) = crate::matchdata_offset(&m, &key, false)? else {
                return Ok(RubyValue::Nil);
            };
            let off = off.lock();
            match (&off[0], &off[1]) {
                (RubyValue::Int(lo), RubyValue::Int(hi)) => (*lo as usize, *hi as usize),
                // A non-participating capture group removes nothing.
                _ => return Ok(RubyValue::Nil),
            }
        }
        _ => return Ok(RubyValue::Nil),
    };
    let removed: String = chars[start..end].iter().collect();
    chars.drain(start..end);
    write_back(handle, chars);
    Ok(str_value(removed))
}

/// The `String#[]=` engine (CRuby `rb_str_aset_m`): resolves the target
/// character span for every index shape, then splices in the replacement.
/// Returns the assigned value, matching Ruby's index-assignment expression.
/// Write a character vector back into `handle` under its OWN encoding.
///
/// `replace_utf8` re-encodes, which is silent corruption for a BINARY
/// string: every byte above 0x7F becomes a two-byte UTF-8 sequence, so a
/// 21-byte gzip member comes back 28 bytes long and does not parse. The
/// characters of a byte-encoded string ARE its bytes, so writing them back
/// as bytes is both faithful and cheap.
fn write_back(handle: &crate::collections::RStr, chars: Vec<char>) {
    let enc = handle.lock().encoding();
    if enc == crate::encoding::UTF_8 {
        handle.lock().replace_utf8(chars.into_iter().collect());
        return;
    }
    // A character that does not fit one byte cannot have come from this
    // string, so it came from the REPLACEMENT -- encode it, and let the
    // ordinary compatibility rules apply to what results.
    let mut bytes = Vec::with_capacity(chars.len());
    for c in chars {
        match u8::try_from(c as u32) {
            Ok(b) => bytes.push(b),
            Err(_) => {
                let mut buf = [0u8; 4];
                bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    handle.lock().replace_bytes(bytes, enc);
}

pub(super) fn index_set_impl(
    recv: &RubyValue,
    index: &RubyValue,
    second: &RubyValue,
    third: Option<&RubyValue>,
) -> Result<RubyValue, Signal> {
    let husk = crate::regexp::husk_payload(index);
    let index = husk.as_ref().unwrap_or(index);
    let handle = recv_str!(recv);
    if handle.is_frozen() {
        return Err(crate::dispatch::raise_error_details(
            "FrozenError",
            format!("can't modify frozen String: {}", recv.inspect_string()),
            &[("receiver", recv.clone())],
        ));
    }
    let val = third.unwrap_or(second);
    // The replacement's `to_str` conversion happens at CRuby's exact point in
    // each index shape: AFTER the index converts but BEFORE its bounds check
    // (Int/start-length/Range forms), yet only after a Regexp/substring index
    // has actually matched. `None` here means "not yet converted".
    let mut repl: Option<crate::collections::RStr> = None;

    let mut chars: Vec<char> = handle.lock().char_vec();
    let n = chars.len() as i64;
    let norm = |i: i64| if i < 0 { i + n } else { i };
    let index_err = |msg: String| crate::builtins::index_error!("{}", msg);

    // Resolve the [start, end) character span to overwrite.
    let (start, end) = if let RubyValue::Regexp(re) = index {
        let (text, enc) = {
            let g = handle.lock();
            (g.to_utf8_lossy().into_owned(), g.encoding())
        };
        let RubyValue::MatchData(m) = crate::regexp_match(re, &text, enc)? else {
            return Err(index_err("regexp not matched".to_string()));
        };
        let span = if third.is_some() {
            match second {
                RubyValue::Int(k) => m.groups.get(*k as usize).copied().flatten(),
                RubyValue::Str(name) => group_span_by_name(&m, &name.lock().to_utf8_lossy()),
                RubyValue::Symbol(s) => group_span_by_name(&m, &s.name()),
                _ => None,
            }
        } else {
            m.groups.first().copied().flatten()
        };
        let Some((bstart, bend)) = span else {
            return Err(index_err("regexp not matched".to_string()));
        };
        (text[..bstart].chars().count(), text[..bend].chars().count())
    } else if third.is_some() {
        let (i, len) = (arg_int!(index), arg_int!(second));
        repl = Some(convert::to_rstr(val)?);
        let start = norm(i);
        if start < 0 || start > n {
            return Err(index_err(format!("index {i} out of string")));
        }
        if len < 0 {
            return Err(index_err(format!("negative length {len}")));
        }
        (start as usize, (start + len).min(n) as usize)
    } else {
        match index {
            RubyValue::Range(__rg) => {
                let (s, e, exclusive) = __rg.parts();
                let start = match s {
                    Some(v) => norm(convert::to_index(v)?),
                    None => 0,
                };
                if start < 0 || start > n {
                    return Err(range_error!("{} out of range", index.to_display_string()));
                }
                let end = match e {
                    Some(v) => {
                        let v = norm(convert::to_index(v)?);
                        if exclusive { v } else { v + 1 }
                    }
                    None => n,
                };
                repl = Some(convert::to_rstr(val)?);
                (start as usize, end.clamp(start, n) as usize)
            }
            RubyValue::Str(sub) => {
                let needle: Vec<char> = sub.lock().to_utf8_lossy().chars().collect();
                match find_subslice(&chars, &needle) {
                    Some(pos) => (pos, pos + needle.len()),
                    None => return Err(index_err("string not matched".to_string())),
                }
            }
            other => {
                let i = convert::to_index(other)?;
                repl = Some(convert::to_rstr(val)?);
                let start = norm(i);
                if start < 0 || start > n {
                    return Err(index_err(format!("index {i} out of string")));
                }
                (start as usize, (start + 1).min(n) as usize)
            }
        }
    };

    let repl = match repl {
        Some(r) => r,
        None => convert::to_rstr(val)?,
    };
    let repl_chars: Vec<char> = repl.lock().to_utf8_lossy().chars().collect();
    chars.splice(start..end, repl_chars);
    write_back(handle, chars);
    Ok(val.clone())
}

/// The byte span of a named capture group, or `None` when the name is absent
/// or the group didn't participate in the match.
pub(super) fn group_span_by_name(m: &crate::RMatchData, name: &str) -> Option<(usize, usize)> {
    let idx = m.names.iter().find(|(nm, _)| nm == name)?.1;
    m.groups.get(idx).copied().flatten()
}

/// The first index of `needle` within `haystack` (both char slices), or None.
pub(super) fn find_subslice(haystack: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// The `[from, len]` BYTE window of a `bytesplice` replacement -- the
/// trailing arguments of its 3-argument range form and its 5-argument
/// index form. Out of range is CRuby's own IndexError, naming the
/// replacement rather than the receiver.
pub(super) fn repl_window(
    repl: &crate::collections::RStr,
    from: i64,
    len: i64,
) -> Result<crate::collections::RStr, Signal> {
    let size = repl.lock().bytesize() as i64;
    let from = if from < 0 { from + size } else { from };
    if from < 0 || from > size {
        return Err(index_error!("index {from} out of string"));
    }
    if len < 0 {
        return Err(index_error!("negative length {len}"));
    }
    let (bytes, enc) = {
        let buf = repl.lock();
        (buf.bytes().to_vec(), buf.encoding())
    };
    let end = ((from + len) as usize).min(bytes.len());
    Ok(crate::collections::string_wrap(
        crate::enc::StrBuf::from_bytes(bytes[from as usize..end].to_vec(), enc),
    ))
}

/// Wraps a `Regexp` or `String` pattern argument as a compiled Regexp --
/// `match`/`match?`'s shared coercion (a String pattern compiles literally).
pub(super) fn to_regexp(v: &RubyValue) -> Result<crate::regexp::RRegexp, Signal> {
    if let Some(re) = crate::regexp::as_regexp(v) {
        return Ok(re);
    }
    match v {
        RubyValue::Regexp(re) => Ok(re.clone()),
        RubyValue::Str(pat) => crate::regexp_new(&pat.lock().to_utf8_lossy(), false, false, false)
            .map_err(|e| regexp_error!("{e}")),
        other => Err(type_error!(
            "wrong argument type {} (expected Regexp)",
            crate::builtins::check_type_name(other)
        )),
    }
}

/// The BYTE offset a `match`/`match?` engine should start at, given an
/// optional start position (char offset, end-relative when negative).
/// `None` means the position lands outside the string -- the caller reports
/// "no match" without running the engine.
///
/// An offset, not a SLICE. A MatchData built from a slice reports its
/// offsets relative to that slice, so `.begin(0)` and `pre_match` were both
/// wrong for every positioned match; the engine takes a start offset
/// directly, which is what CRuby's `rb_reg_search(str, re, pos, 0)` uses.
pub(crate) fn match_haystack(text: &str, pos: Option<&RubyValue>) -> Result<Option<usize>, Signal> {
    let Some(v) = pos else {
        return Ok(Some(0));
    };
    let clen = text.chars().count() as i64;
    let start = match convert::to_index(v)? {
        p if p < 0 => p + clen,
        p => p,
    };
    if start < 0 || start > clen {
        return Ok(None);
    }
    Ok(Some(
        text.char_indices()
            .nth(start as usize)
            .map_or(text.len(), |(b, _)| b),
    ))
}

/// Expands the replacement-string escapes CRuby honors for a String-pattern
/// `sub`/`gsub`: `\\` -> `\`, `\&`/`\0` -> the match, `` \` `` -> the text
/// before it, `\'` -> the text after; `\1`..`\9` insert nothing (a String
/// pattern captures no groups). Any other `\X` stays literal.
pub(super) fn expand_str_replacement(
    template: &str,
    prematch: &str,
    matched: &str,
    postmatch: &str,
) -> String {
    let mut out = String::new();
    let mut chars = template.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('0') | Some('&') => out.push_str(matched),
            Some(d) if d.is_ascii_digit() => {}
            Some('`') => out.push_str(prematch),
            Some('\'') => out.push_str(postmatch),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Every character boundary of `text` as `(byte_offset, Some(char))`, plus a
/// final `(text.len(), None)` for the end-of-string boundary. Drives the
/// empty-pattern `sub`/`gsub` insertion (a match at every boundary).
pub(super) fn char_boundaries(text: &str) -> impl Iterator<Item = (usize, Option<char>)> + '_ {
    text.char_indices()
        .map(|(i, c)| (i, Some(c)))
        .chain(std::iter::once((text.len(), None)))
}

/// `sub`/`gsub`'s shared core: String or Regexp pattern, String
/// replacement or block.
pub(super) fn sub_gsub(
    recv: &RubyValue,
    args: &[RubyValue],
    block: Option<RubyValue>,
    global: bool,
) -> Result<RubyValue, Signal> {
    // The engine works on the decoded text; `enc` carries the receiver's
    // encoding across so every result below is rebuilt in it rather than
    // being retagged UTF-8.
    let (text, enc) = match recv {
        RubyValue::Str(s) => {
            let g = s.lock();
            (g.to_utf8_lossy().into_owned(), g.encoding())
        }
        _ => unreachable!("String table row dispatched on a non-String receiver"),
    };
    let block_proc = match &block {
        Some(RubyValue::Proc(p)) => Some(p.clone()),
        _ => None,
    };
    if block_proc.is_some() {
        crate::builtins::check_arity(args.len(), 1, Some(1))?;
    } else if global && args.len() == 1 {
        // Blockless `gsub(pattern)` is an Enumerator over the matched
        // substrings (iterating it with a block performs the substitution,
        // `rb_enumeratorize`'s re-invoke rule). `sub` has no such form --
        // it keeps the 2-arg ArgumentError below (oracle-verified).
        return Ok(crate::builtins::enumerator::enumerator_for(
            recv, "gsub", args,
        ));
    } else {
        crate::builtins::check_arity(args.len(), 2, Some(2))?;
    }
    // Pattern: a Regexp as-is; anything else through the `to_str` probe (a
    // literal pattern); a non-convertible pattern is CRuby's
    // "wrong argument type X (expected Regexp)" (oracle-verified, no
    // method-name suffix).
    let pattern_arg = match &args[0] {
        re @ RubyValue::Regexp(_) => re.clone(),
        // A `class MyRe < Regexp` husk IS the pattern; `to_str` would not find
        // it, since ruby has no Regexp conversion protocol.
        husk if crate::regexp::husk_payload(husk).is_some() => {
            crate::regexp::husk_payload(husk).expect("just probed")
        }
        other => match convert::check_to_str(other)? {
            Some(s) => s,
            None => {
                return Err(type_error!(
                    "wrong argument type {} (expected Regexp)",
                    crate::builtins::check_type_name(other)
                ));
            }
        },
    };
    // A STRING pattern has to be encoding-compatible with the receiver --
    // CRuby's `rb_enc_check`, the same test `+` makes. Without it the
    // pattern's bytes were reinterpreted under the receiver's encoding and
    // could MATCH, so a substitution happened where ruby raises.
    if let RubyValue::Str(pat) = &pattern_arg {
        let recv_str = recv_str!(recv);
        let (a, b) = (recv_str.lock(), pat.lock());
        if crate::encoding::compat_concat_enc(&a, &b).is_none() {
            let (left, right) = (a.encoding(), b.encoding());
            drop((a, b));
            return Err(crate::dispatch::raise_error(
                "Encoding::CompatibilityError",
                format!(
                    "incompatible character encodings: {} and {}",
                    left.inspect_name(),
                    right.inspect_name()
                ),
            ));
        }
    }
    // Re-encoded at the single exit rather than per arm: the Regexp arms hand
    // back whatever the engine built, and it works in decoded UTF-8. The
    // String arms already build in `enc`, and re-encoding those is a no-op.
    let result = match (&pattern_arg, block_proc) {
        (RubyValue::Regexp(re), None) => match &args[1] {
            // A Hash replacement maps each matched substring to `hash[match]`,
            // exactly a block that looks the match up -- so it rides the
            // existing block-substitution path. Through `[]`, not a raw table
            // read: a miss takes the hash's DEFAULT (or its default_proc), and
            // only a hash with neither yields the empty string.
            RubyValue::Hash(h) => {
                let table = RubyValue::Hash(h.clone());
                let p = crate::RProc::new(move |a: &[RubyValue]| {
                    crate::dispatch::send_value(
                        &table,
                        crate::Symbol::intern("[]"),
                        std::slice::from_ref(&a[0]),
                        None,
                    )
                });
                if global {
                    crate::regexp_gsub_block(re, &text, enc, &p)
                } else {
                    crate::regexp_sub_block(re, &text, enc, &p)
                }
            }
            other => {
                let replacement = convert::to_rstr(other)?.lock().to_utf8_lossy().into_owned();
                if global {
                    crate::regexp_gsub(re, &text, &replacement)
                } else {
                    crate::regexp_sub(re, &text, &replacement)
                }
            }
        },
        (RubyValue::Regexp(re), Some(p)) => {
            if global {
                crate::regexp_gsub_block(re, &text, enc, &p)
            } else {
                crate::regexp_sub_block(re, &text, enc, &p)
            }
        }
        (RubyValue::Str(pattern), None) => {
            let pattern = pattern.lock().to_utf8_lossy().into_owned();
            // A String pattern matches literally, so its "matched substring" is
            // always the pattern itself. A Hash replacement looks that up (a
            // default-aware) and is inserted literally.
            if let RubyValue::Hash(h) = &args[1] {
                let key = str_value_in(enc, &pattern);
                // Through `[]`, so a miss takes the hash's DEFAULT rather than
                // stringifying nil to the empty string.
                let replacement = crate::dispatch::send_value(
                    &RubyValue::Hash(h.clone()),
                    crate::Symbol::intern("[]"),
                    std::slice::from_ref(&key),
                    None,
                )?
                .to_display_string();
                return Ok(str_value_in(
                    enc,
                    &if global {
                        text.replace(&pattern, &replacement)
                    } else {
                        text.replacen(&pattern, &replacement, 1)
                    },
                ));
            }
            // A String replacement still processes replacement escapes (`\\`,
            // `\&`/`\0`, `\``, `\'`) per match, exactly like the Regexp form;
            // `\1`..`\9` insert nothing (a String pattern has no groups).
            let template = arg_str!(args, 1).lock().to_utf8_lossy().into_owned();
            // An empty pattern matches (emptily) at every character boundary and
            // at the end: gsub inserts the replacement before each char and at
            // the end (`"hi".gsub("", "-") == "-h-i-"`); sub only at the start.
            if pattern.is_empty() {
                let mut out = String::new();
                for (k, (pos, ch)) in char_boundaries(&text).enumerate() {
                    if global || k == 0 {
                        out.push_str(&expand_str_replacement(
                            &template,
                            &text[..pos],
                            "",
                            &text[pos..],
                        ));
                    }
                    if let Some(ch) = ch {
                        out.push(ch);
                    }
                }
                return Ok(str_value_in(enc, &out));
            }
            let mut out = String::new();
            let mut rest = text.as_str();
            let mut consumed = 0usize;
            loop {
                match rest.find(&pattern) {
                    Some(pos) if !pattern.is_empty() => {
                        out.push_str(&rest[..pos]);
                        let mstart = consumed + pos;
                        let mend = mstart + pattern.len();
                        out.push_str(&expand_str_replacement(
                            &template,
                            &text[..mstart],
                            &pattern,
                            &text[mend..],
                        ));
                        rest = &rest[pos + pattern.len()..];
                        consumed = mend;
                        if !global {
                            break;
                        }
                    }
                    _ => break,
                }
            }
            out.push_str(rest);
            Ok(str_value_in(enc, &out))
        }
        (RubyValue::Str(pattern), Some(p)) => {
            let pattern = pattern.lock().to_utf8_lossy().into_owned();
            if pattern.is_empty() {
                let mut out = String::new();
                for (k, (_, ch)) in char_boundaries(&text).enumerate() {
                    if global || k == 0 {
                        let replaced = p.call(&[str_value_in(enc, "")])?;
                        out.push_str(&replaced.to_display_string());
                    }
                    if let Some(ch) = ch {
                        out.push(ch);
                    }
                }
                return Ok(str_value_in(enc, &out));
            }
            let mut out = String::new();
            let mut rest = text.as_str();
            loop {
                match rest.find(&pattern) {
                    Some(pos) if !pattern.is_empty() => {
                        out.push_str(&rest[..pos]);
                        let replaced = p.call(&[str_value_in(enc, &pattern)])?;
                        out.push_str(&replaced.to_display_string());
                        rest = &rest[pos + pattern.len()..];
                        if !global {
                            break;
                        }
                    }
                    _ => break,
                }
            }
            out.push_str(rest);
            Ok(str_value_in(enc, &out))
        }
        (_, _) => unreachable!("sub/gsub pattern normalized to Str/Regexp above"),
    };
    result.map(|v| reencode_strs(&v, enc))
}

pub(super) enum Pad {
    Center,
    Left,
    Right,
}

pub(super) fn pad(
    recv: &RubyValue,
    width: &RubyValue,
    fill: Option<&RubyValue>,
    kind: Pad,
) -> Result<RubyValue, Signal> {
    let text = match recv {
        RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
        _ => unreachable!("String table row dispatched on a non-String receiver"),
    };
    let width = convert::to_index(width)?;
    // The result's encoding combines the receiver's with the PAD's, and
    // CRuby checks that before it checks whether any padding is needed at
    // all -- `"café".ljust(1, latin1)` raises even though it pads nothing.
    let recv_buf = recv_str!(recv).lock().clone();
    let (fill, enc) = match fill {
        None => (" ".to_string(), recv_buf.encoding()),
        Some(f) => {
            let pad = convert::to_rstr(f)?.lock().clone();
            let enc = encode::combined_encoding(&recv_buf, &pad)?;
            (pad.to_utf8_lossy().into_owned(), enc)
        }
    };
    if fill.is_empty() {
        return Err(arg_error!("zero width padding"));
    }
    let len = text.chars().count() as i64;
    let total = (width - len).max(0) as usize;
    let fill_n = |n: usize| -> String { fill.chars().cycle().take(n).collect() };
    let out = match kind {
        Pad::Left => format!("{text}{}", fill_n(total)),
        Pad::Right => format!("{}{text}", fill_n(total)),
        Pad::Center => {
            let left = total / 2;
            format!("{}{text}{}", fill_n(left), fill_n(total - left))
        }
    };
    // Already wide enough: the pad contributed no byte, so the answer keeps
    // the receiver's encoding. The compatibility CHECK above still ran, which
    // is CRuby's order -- `"café".ljust(1, latin1)` raises.
    Ok(encode::str_value_in_enc(
        match total {
            0 => recv_buf.encoding(),
            _ => enc,
        },
        &out,
    ))
}

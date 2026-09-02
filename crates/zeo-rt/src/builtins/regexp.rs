//! `Regexp` (CRuby re.c) -- stage B carries only `===` (pattern-match
//! case equality; Kernel's equality default would silently never match).
//! The dynamic-path breadth (`match`/`=~`/`source`/...) rides stage D with
//! String's, sharing `crate::regexp`'s helpers with the static paths.

use crate::RubyValue;
use crate::builtins::inherited_row;
use crate::builtins::regexp_error;
use zeo_macros::ruby_class;

ruby_class! {
    Regexp = zeo_abi::REGEXP_CLASS < zeo_abi::OBJECT_CLASS;

    allocate regexp_allocate;

    seed seed_regexp_constants;

    // `Regexp.timeout` -- the process-wide default match timeout, in seconds.
    // A pattern's own `timeout:` overrides it; the engine raises
    // `Regexp::TimeoutError` past either.
    def self."timeout" (_recv) {
        Ok(timeout_value(crate::regexp::global_timeout()))
    }
    def self."timeout=" (_recv, seconds) {
        crate::regexp::set_global_timeout(crate::regexp::timeout_seconds(seconds)?);
        Ok(seconds.clone())
    }
    // `~re` -- match against `$_`, answering the match position or nil. The
    // one operator that reads the last-read-line global rather than an operand.
    def "~" (recv) {
        let line = crate::globals::global_get(0, "$_");
        if !matches!(line, RubyValue::Str(_)) {
            return Ok(RubyValue::Nil);
        }
        crate::dispatch::send_value(recv, crate::Symbol::intern("=~"), &[line], None)
    }

    // `Regexp.last_match` / `Regexp.last_match(n)` -- the thread-local `$~`
    // (whole MatchData), or its nth capture group when given an index.
    def self."last_match"(_recv, arg?) {
        match arg {
            None => Ok(crate::lastmatch::last_match()),
            // A Symbol or String selects a NAMED group, not an index:
            // `rb_reg_s_last_match` forwards anything that is not an Integer
            // to `match_aref`, so the whole `MatchData#[]` key language works
            // here. Routing through it is also what keeps the two in step.
            Some(v) => match crate::lastmatch::last_match() {
                RubyValue::MatchData(md) => crate::regexp::matchdata_get(&md, v),
                _ => Ok(RubyValue::Nil),
            },
        }
    }

    // `Regexp.escape(str)` / `.quote(str)`: a source-safe literal of `str`.
    def self."escape" | "quote"(_recv, arg) {
        // `reg_operand` takes a SYMBOL as its own name, beside the `to_str`
        // protocol -- `Regexp.escape(:"a.b")` is `"a\\.b"`.
        let text = match arg {
            RubyValue::Symbol(sym) => sym.name(),
            other => crate::builtins::convert::to_rstr(other)?
                .lock()
                .to_utf8_lossy()
                .into_owned(),
        };
        let escaped = escape_regexp_source(&text);
        Ok(RubyValue::Str(crate::string_new(escaped)))
    }

    // `Regexp.new(str_or_regexp, flags = nil)` / `Regexp.compile(...)`. A
    // Regexp source is copied with its own flags; a string source takes its
    // flags from the second argument -- an Integer bitmask of the three
    // `Regexp::` constants, or `true` (case-insensitive) / `false`/`nil`
    // (none), matching CRuby's historical boolean shorthand.
    // Ruby reaches `Regexp.new` through `Class#new`, but declares `compile` on
    // Regexp itself -- so the marker is per-name, not per-def.
    def self."new" inherits | "compile" cfunc (_recv, arg1, arg2?, **opts) {
        let timeout = timeout_kwarg(opts)?;
        // A Regexp source: clone it verbatim (flags and all), ignoring any
        // extra options -- CRuby warns but reuses the original.
        if let Some(re) = crate::regexp::as_regexp(arg1) {
            return crate::regexp_new(&re.source, re.ignore_case, re.extended, re.multiline)
                .map(|re| RubyValue::Regexp(re.with_timeout(timeout)))
                .map_err(|e| regexp_error!("{e}"));
        }
        let s = &crate::builtins::convert::to_rstr(arg1)?;
        // The SOURCE STRING's encoding decides how wide a character is, which
        // is what `\M-a` (one byte past 0x7f) turns on: whole in binary,
        // half a character in UTF-8.
        let binary = s.lock().encoding() == crate::encoding::ASCII_8BIT;
        let source = s.lock().to_utf8_lossy().into_owned();
        let (ignore_case, extended, multiline) = match arg2 {
            None | Some(RubyValue::Nil) | Some(RubyValue::Bool(false)) => (false, false, false),
            Some(RubyValue::Bool(true)) => (true, false, false),
            Some(RubyValue::Int(f)) => {
                (f & IGNORECASE != 0, f & EXTENDED != 0, f & MULTILINE != 0)
            }
            // A String spells the flags out. Ruby names the WHOLE string in
            // the refusal, not the one letter that was wrong.
            Some(RubyValue::Str(o)) => {
                let text = o.lock().to_utf8_lossy().into_owned();
                let mut flags = (false, false, false);
                for ch in text.chars() {
                    match ch {
                        'i' => flags.0 = true,
                        'x' => flags.1 = true,
                        'm' => flags.2 = true,
                        _ => {
                            return Err(crate::builtins::arg_error!(
                                "unknown regexp option: {text}"
                            ));
                        }
                    }
                }
                flags
            }
            Some(other) => (other.truthy(), false, false),
        };
        let enc = match binary {
            true => zeo_abi::RegexpEncoding::None,
            false => zeo_abi::RegexpEncoding::Source,
        };
        crate::regexp::regexp_new_enc(&source, ignore_case, extended, multiline, enc)
            .map(|re| RubyValue::Regexp(re.with_timeout(timeout)))
            .map_err(|e| regexp_error!("{e}"))
    }

    // `Regexp.union(pat, ...)` / `Regexp.union([pat, ...])`: an alternation of
    // the patterns. A String member is escaped; a Regexp member keeps its own
    // flags via its `(?-mix:src)` form. Empty -> the never-matching `(?!)`.
    def self."union"(_recv, *args, &_block) {
        let items: Vec<RubyValue> = match args {
            [RubyValue::Array(a)] => a.lock().to_vec(),
            _ => args.to_vec(),
        };
        let source = if items.is_empty() {
            "(?!)".to_string()
        } else {
            let mut parts = Vec::with_capacity(items.len());
            for item in &items {
                match crate::regexp::as_regexp(item) {
                    Some(re) => parts.push(regexp_to_s_string(&re)),
                    None => {
                        let s = crate::builtins::convert::to_rstr(item)?;
                        parts.push(escape_regexp_source(&s.lock().to_utf8_lossy()))
                    }
                }
            }
            parts.join("|")
        };
        crate::regexp_new(&source, false, false, false)
            .map(RubyValue::Regexp)
            .map_err(|e| regexp_error!("{e}"))
    }

    // `Regexp.try_convert(obj)` -- `obj` if it is already a Regexp, its
    // `to_regexp` if it defines one, else `nil`. Only a present-and-lying
    // `to_regexp` raises.
    def self."try_convert" (_recv, arg) {
        Ok(crate::builtins::convert::try_convert_value(arg, "Regexp", "to_regexp")?
            .unwrap_or(RubyValue::Nil))
    }

    // `Regexp.linear_time?(re_or_str, flags = nil)` -- whether matching the
    // pattern is guaranteed linear-time. True unless it uses a backreference
    // (lookaround and nested quantifiers stay linear); oracle-verified. A
    // CLASS method only: CRuby has no `Regexp#linear_time?`.
    def self."linear_time?" cfunc (_recv, arg1, _arg2?) {
        let source = match crate::regexp::as_regexp(arg1) {
            Some(re) => re.source.clone(),
            None => crate::builtins::convert::to_rstr(arg1)?
                .lock()
                .to_utf8_lossy()
                .into_owned(),
        };
        Ok(RubyValue::Bool(!has_backreference(&source)))
    }

    def "===" (recv, other) {
        Ok(RubyValue::Bool(recv.rb_case_eq(other)?))
    }
    // A literal that FORCED an encoding (`/e`, `/s`, `/u`) answers that one;
    // `/n` and a plain literal answer what their own source bytes compute to.
    def "encoding" (recv) {
        let RubyValue::Regexp(re) = recv else {
            unreachable!("the Regexp table only dispatches on Regexp receivers")
        };
        // A blank has no source to compute an encoding FROM, and ruby answers
        // the binary one rather than deriving US-ASCII from nothing.
        if re.uninitialized {
            return Ok(crate::builtins::encoding::encoding_value(
                crate::encoding::ASCII_8BIT,
            ));
        }
        let id = match re.encoding {
            zeo_abi::RegexpEncoding::EucJp => crate::encoding::EUC_JP,
            zeo_abi::RegexpEncoding::Windows31j => crate::encoding::WINDOWS_31J,
            zeo_abi::RegexpEncoding::Utf8 => crate::encoding::UTF_8,
            zeo_abi::RegexpEncoding::None | zeo_abi::RegexpEncoding::Source => {
                crate::builtins::encoding::computed_encoding_of(&re.source)
            }
        };
        Ok(crate::builtins::encoding::encoding_value(id))
    }
    // Every reachable zeo Regexp is compiled: a frozen one (every literal)
    // answers FrozenError, anything else CRuby's "already initialized
    // regexp" -- the oracle's two answers, in its order.
    private def "initialize" cfunc (recv, *_args) {
        Err(regexp_reinit_refusal(recv))
    }
    private def "initialize_copy"(recv, _other) {
        Err(regexp_reinit_refusal(recv))
    }
    def "source" (recv) {
        Ok(crate::regexp_source(live_re(recv)?))
    }
    // `#match?` tests for a match without building a `MatchData` or touching
    // `$~`; `#match` and `#=~` do build one (and set `$~`) via the runtime
    // helpers String's own rows share.
    def "match?" cfunc (recv, arg1, arg2?) {
        let Some((h, _enc)) = subject_arg(live_re(recv)?, arg1)? else { return Ok(RubyValue::Bool(false)) };
        // An optional start position (char offset, end-relative when negative)
        // anchors the search; a position past the end is simply no match.
        let Some(at) = crate::builtins::string::match_haystack(&h, arg2)? else {
            return Ok(RubyValue::Bool(false));
        };
        Ok(RubyValue::Bool(crate::regexp::regexp_is_match_at(
            re_of(recv),
            &h,
            at,
        )?))
    }
    def "match" cfunc (recv, arg1, arg2?, &block) {
        let Some((h, enc)) = subject_arg(live_re(recv)?, arg1)? else { return Ok(RubyValue::Nil) };
        // The optional start position, same rules as `match?` above; outside
        // the string is nil without running the engine.
        let m = match crate::builtins::string::match_haystack(&h, arg2)? {
            Some(at) => crate::regexp::regexp_match_at(re_of(recv), &h, at, enc)?,
            None => RubyValue::Nil,
        };
        // With a block, ruby YIELDS the MatchData on a hit and the call
        // evaluates to the BLOCK's value; a miss answers nil without running
        // it. Only the String-receiver form had this arm, so with a Regexp
        // receiver the block never ran and the MatchData was the value.
        if let (Some(RubyValue::Proc(p)), false) = (&block, m.is_nil()) {
            return p.call(std::slice::from_ref(&m));
        }
        Ok(m)
    }
    def "=~" (recv, other) {
        let Some((h, enc)) = subject_arg(live_re(recv)?, other)? else { return Ok(RubyValue::Nil) };
        crate::regexp_search_index(re_of(recv), &h, enc)
    }
    // `casefold?` reports the `/i` flag.
    def "casefold?" (recv) {
        Ok(RubyValue::Bool(live_re(recv)?.ignore_case))
    }
    // A regexp is fixed-encoding when it is tied to a specific encoding rather
    // than the ASCII-agnostic default. Two ways to get there: a flag PINNED one
    // (`/e`, `/s`, `/u` -- but not `/n`, which declares the opposite), or the
    // source itself carries a non-ASCII character, so `computed_encoding_of`
    // resolves past US-ASCII (`/café/` -> UTF-8 -> true; `/abc/` -> false).
    def "fixed_encoding?" (recv) {
        let re = live_re(recv)?;
        if re.encoding.is_fixed() {
            return Ok(RubyValue::Bool(true));
        }
        let enc = crate::builtins::encoding::computed_encoding_of(&re.source);
        Ok(RubyValue::Bool(enc != crate::encoding::US_ASCII))
    }
    // `names` lists the named capture groups in order; `named_captures` maps
    // each name to its 1-based capture position(s).
    def "names" (recv) {
        // Each distinct name once, in first-appearance order (a name reused by
        // several groups -- `/(?<a>x)(?<a>z)/` -- lists once, as CRuby does).
        // Parsed from the source, since the engine collapses repeated names.
        let mut seen: Vec<String> = Vec::new();
        for (n, _) in crate::regexp::named_group_positions(&live_re(recv)?.source) {
            if !seen.contains(&n) {
                seen.push(n);
            }
        }
        let out = seen.into_iter().map(|n| RubyValue::Str(crate::string_new(n))).collect();
        Ok(RubyValue::Array(crate::array_new(out)))
    }
    def "named_captures" (recv) {
        // Map each name to the LIST of its 1-based group indices, in
        // first-appearance order: a name shared by several groups
        // (`/(?<a>x)(?<a>z)/`) collects all of them (`{"a" => [1, 2]}`), not
        // just the last -- CRuby's `named_captures`.
        let mut order: Vec<String> = Vec::new();
        let mut indices: std::collections::HashMap<String, Vec<RubyValue>> = std::collections::HashMap::new();
        for (n, i) in crate::regexp::named_group_positions(&live_re(recv)?.source) {
            indices
                .entry(n.clone())
                .or_insert_with(|| {
                    order.push(n.clone());
                    Vec::new()
                })
                .push(RubyValue::Int(i as i64));
        }
        let pairs = order
            .into_iter()
            .map(|n| {
                let idxs = indices.remove(&n).expect("every ordered name has indices");
                (RubyValue::Str(crate::string_new(n)), RubyValue::Array(crate::array_new(idxs)))
            })
            .collect();
        Ok(RubyValue::Hash(crate::hash_new(pairs)))
    }
    // `#timeout` -- this pattern's own `timeout:`, never the global default.
    def "timeout" (recv) {
        Ok(timeout_value(live_re(recv)?.engine.timeout()))
    }
    // `#options` -- the `Regexp::` flag bitmask this pattern was built with.
    def "options" (recv) {
        let re = live_re(recv)?;
        let bits = (re.ignore_case as i64) * IGNORECASE
            + (re.extended as i64) * EXTENDED
            + (re.multiline as i64) * MULTILINE
            // `FIXEDENCODING` (16) for `/e`/`/s`/`/u`, `NOENCODING` (32) for
            // `/n` -- the bits ruby2ruby reads back out of `/x/e.options`.
            + re.encoding.option_bits();
        Ok(RubyValue::Int(bits))
    }

    // ---- rows ruby OWNS on this class while the body lives on an ancestor.
    // Each calls the very row it would otherwise have inherited, so `.owner`
    // and `instance_methods(false)` agree and there is still only one body.
    def "=="(recv, _other) { inherited_row!(basic_object, "==", recv, __args, None) }
    def "eql?"(recv, _other) { inherited_row!(kernel, "eql?", recv, __args, None) }
    // `hash` and `to_s` read the pattern; `==`, `eql?` and `inspect` do not,
    // so a blank still compares and still prints.
    def "hash"(recv) {
        live_re(recv)?;
        inherited_row!(kernel, "hash", recv, __args, None)
    }
    def "inspect"(recv) { Ok(RubyValue::Str(crate::string_new(recv.structural_inspect()?))) }
    // NOT an alias of `#inspect`: `Complex`, `Rational` and `Regexp` all
    // spell the two differently, so each goes to its own Kernel row.
    def "to_s"(recv) {
        live_re(recv)?;
        Ok(RubyValue::Str(crate::string_new(recv.structural_to_s()?)))
    }
}

/// True if `source` contains a backreference (`\1`..`\9` or `\k<name>`/
/// `\k'name'`) -- the only construct that forces non-linear matching in
/// `Regexp.linear_time?`. A backslash always consumes the next character, so
/// an escaped backslash (`\\1`) is a literal, not a backref.
fn has_backreference(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut i = 0;
    // Inside a character class, `\1`/`\k` are octal/literal, NOT backreferences
    // (`/[\1]/` is linear); only a real backref OUTSIDE any class counts. `]`
    // closes the class unless it is the first member, and `\]` is an escaped
    // literal that does not close it.
    let mut in_class = false;
    let mut class_start = false;
    while i < bytes.len() {
        let c = bytes[i];
        if !in_class {
            match c {
                b'[' => {
                    in_class = true;
                    class_start = true;
                    i += 1;
                }
                b'\\' if i + 1 < bytes.len() => {
                    let n = bytes[i + 1];
                    if n.is_ascii_digit() && n != b'0' {
                        return true;
                    }
                    if n == b'k' && matches!(bytes.get(i + 2), Some(b'<' | b'\'')) {
                        return true;
                    }
                    i += 2;
                }
                _ => i += 1,
            }
        } else {
            match c {
                b']' if !class_start => {
                    in_class = false;
                    i += 1;
                }
                b'^' if class_start => i += 1,
                b'\\' if i + 1 < bytes.len() => {
                    class_start = false;
                    i += 2;
                }
                _ => {
                    class_start = false;
                    i += 1;
                }
            }
        }
    }
    false
}

/// A `Regexp`'s `#to_s` (`(?-mix:src)`) as a plain `String` -- `Regexp.union`
/// embeds each member regexp this way, preserving its own flags.
fn regexp_to_s_string(re: &crate::RRegexp) -> String {
    match crate::regexp::regexp_to_s(re) {
        RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
        _ => unreachable!("regexp_to_s always returns a Str"),
    }
}

/// The receiver of a Regexp instance row, already known to be a Regexp.
fn re_of(recv: &RubyValue) -> &crate::RRegexp {
    let RubyValue::Regexp(re) = recv else {
        unreachable!("the Regexp table only dispatches on Regexp receivers")
    };
    re
}

/// [`re_of`] for the rows that need a real PATTERN, which is every row but
/// `#encoding` and object identity. `Regexp.allocate` hands back a receiver
/// with no pattern behind it, and ruby refuses to read one.
fn live_re(recv: &RubyValue) -> Result<&crate::RRegexp, crate::Signal> {
    let re = re_of(recv);
    match re.uninitialized {
        true => Err(crate::builtins::type_error!("uninitialized Regexp")),
        false => Ok(re),
    }
}

/// A timeout as Ruby reports it: a Float, or nil for none.
fn timeout_value(seconds: Option<f64>) -> RubyValue {
    seconds.map_or(RubyValue::Nil, RubyValue::Float)
}

/// `Regexp.new`'s keyword half: `timeout:` is the only one it takes.
fn timeout_kwarg(opts: Option<&RubyValue>) -> Result<Option<f64>, crate::Signal> {
    let Some(RubyValue::Hash(h)) = opts else {
        return Ok(None);
    };
    let key = RubyValue::Symbol(crate::Symbol::intern("timeout"));
    for (k, _) in crate::hash_pairs(h) {
        if !k.rb_eq(&key) {
            return Err(crate::builtins::arg_error!(
                "unknown keyword: {}",
                k.inspect_string()
            ));
        }
    }
    crate::regexp::timeout_seconds(&crate::hash_get(h, &key))
}

/// A blank `Regexp` -- the one value whose `uninitialized` flag is set. Its
/// engine never runs: every row that would reach it goes through [`live_re`].
fn regexp_allocate() -> RubyValue {
    let blank = crate::regexp::regexp_new("", false, false, false)
        .expect("the empty pattern always compiles");
    RubyValue::Regexp(std::sync::Arc::new(crate::regexp::RegexpData {
        engine: blank.engine.clone(),
        source: String::new(),
        uninitialized: true,
        ignore_case: false,
        extended: false,
        multiline: false,
        encoding: zeo_abi::RegexpEncoding::None,
        frozen: std::sync::atomic::AtomicBool::new(false),
    }))
}

/// The subject of `Regexp#=~`/`#match`/`#match?`: a String matches, `nil`
/// answers "no match" (never raises), and any other type raises TypeError --
/// CRuby's rule (`/p/ =~ 5` -> TypeError, not a silent non-match).
/// The decoded subject, WITH the encoding it was decoded from -- every group
/// the match hands back has to come back in it, so the two travel together.
type Subject = (String, crate::encoding::EncodingId);

fn subject_arg(re: &crate::RRegexp, v: &RubyValue) -> Result<Option<Subject>, crate::Signal> {
    match v {
        RubyValue::Nil => Ok(None),
        // A Symbol matches as its name, which is not an implicit String
        // conversion but a case CRuby's regexp entry points special-case --
        // `delegate.rb` filters `private_instance_methods` with `/…/ =~ m`.
        // Its name is the subject, in the encoding the Symbol itself reports.
        RubyValue::Symbol(s) => Ok(Some((s.name(), s.encoding()))),
        other => {
            let handle = crate::builtins::convert::to_rstr(other)?;
            let buf = handle.lock();
            // Matching READS characters, so CRuby refuses a subject whose
            // bytes are not valid in its own encoding (`rb_enc_check` on the
            // way in) rather than matching against replacement characters --
            // which is what `to_utf8_lossy` would silently do here.
            if !buf.valid_encoding() {
                let enc = buf.encoding().name();
                drop(buf);
                return Err(crate::builtins::arg_error!(
                    "invalid byte sequence in {enc}"
                ));
            }
            let (enc, ascii_only) = (buf.encoding(), buf.ascii_only());
            let text = buf.to_utf8_lossy().into_owned();
            drop(buf);
            crate::builtins::encoding::guard_regexp_haystack(re, enc, ascii_only)?;
            Ok(Some((text, enc)))
        }
    }
}

/// Ruby's `Regexp::` flag bits, the second argument to `Regexp.new`.
const IGNORECASE: i64 = 1;
const EXTENDED: i64 = 2;
const MULTILINE: i64 = 4;

/// Seeds the `Regexp::*` option bits -- called once from generated `main()`,
/// alongside the other builtin-constant seeders. `FIXEDENCODING` and
/// `NOENCODING` are the two `#options` bits zeo never sets (its regexps are
/// always encoding-aware), but a program that ANDs against them must still
/// find them defined.
pub fn seed_regexp_constants() {
    let re = zeo_abi::REGEXP_CLASS.0;
    crate::const_set(re, "IGNORECASE", RubyValue::Int(IGNORECASE));
    crate::const_set(re, "EXTENDED", RubyValue::Int(EXTENDED));
    crate::const_set(re, "MULTILINE", RubyValue::Int(MULTILINE));
    crate::const_set(re, "FIXEDENCODING", RubyValue::Int(16));
    crate::const_set(re, "NOENCODING", RubyValue::Int(32));
}

/// `Regexp.escape`/`.quote`: backslash-escapes every regex metacharacter (and
/// renders control characters as their `\t`/`\n`/... escapes) so the result
/// matches the input literally -- CRuby's `rb_reg_quote` character set exactly.
fn escape_regexp_source(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '[' | ']' | '(' | ')' | '{' | '}' | '.' | '?' | '+' | '*' | '^' | '$' | '|' | '#'
            | '-' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            ' ' => out.push_str("\\ "),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{0c}' => out.push_str("\\f"),
            '\u{0b}' => out.push_str("\\v"),
            _ => out.push(c),
        }
    }
    out
}

/// The re-init refusal, in CRuby's precedence: a frozen receiver (every
/// literal) is a FrozenError; anything else "already initialized regexp".
fn regexp_reinit_refusal(recv: &RubyValue) -> crate::Signal {
    if recv.is_frozen() {
        crate::dispatch::raise_error_details(
            "FrozenError",
            format!("can't modify frozen Regexp: {}", recv.inspect_string()),
            &[("receiver", recv.clone())],
        )
    } else {
        crate::builtins::type_error!("already initialized regexp")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        (crate::builtins::registered_table(zeo_abi::REGEXP_CLASS)
            .unwrap()
            .instance
            .as_ref()
            .unwrap()
            .lookup)(name)
        .unwrap()
    }

    #[test]
    fn case_eq_matches_a_string_subject() {
        let re =
            RubyValue::Regexp(crate::regexp_new("ab", false, false, false).expect("valid pattern"));
        let s = RubyValue::Str(crate::string_new("cabs".to_string()));
        assert!(matches!(
            imethod("===")(&re, &[s], None).unwrap(),
            RubyValue::Bool(true)
        ));
        let miss = RubyValue::Str(crate::string_new("xyz".to_string()));
        assert!(matches!(
            imethod("===")(&re, &[miss], None).unwrap(),
            RubyValue::Bool(false)
        ));
    }
}

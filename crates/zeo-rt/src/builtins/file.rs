//! `File` (CRuby file.c) -- the class-level surface: whole-file read/write,
//! the predicates, and the PURE-PATH family (`basename`/`dirname`/`join`/...,
//! which are string manipulation and never touch the disk).
//!
//! `File < IO` (see the ABI table), so a File INSTANCE answers IO's instance
//! methods through the ordinary MRO walk -- nothing is duplicated here.
//!
//! Every path is handled as bytes-as-String: a path round-trips through
//! `String`, and a non-UTF-8 path is out of reach.

use crate::builtins::{arg_error, block_or_enum, type_error};
use crate::dispatch::raise_error;
use crate::{RubyValue, Signal};
use zeo_macros::ruby_class;

/// Map an `io::Error` to the Ruby `Errno::*` raise CRuby would make, with
/// its message shape: `No such file or directory @ rb_sysopen - /nope/x`.
/// `syscall` is the C function CRuby names in the message (`rb_sysopen` for
/// opens, `dir_initialize` for Dir) -- it is part of the observable message,
/// so callers pass the one their operation corresponds to.
pub fn raise_errno(e: &std::io::Error, syscall: &str, path: &str) -> Signal {
    let (class, desc) = errno_class_and_desc(e);
    raise_error(class, format!("{desc} @ {syscall} - {path}"))
}

/// The whole-file read helpers' site: CRuby OPENS (`rb_sysopen`) and then
/// READS (`io_fread`), and only the second step can answer `EISDIR`. One
/// Rust call covers both, so the site is picked from the errno -- otherwise
/// `File.read(a_directory)` blamed the open, which succeeded.
pub fn raise_read_errno(e: &std::io::Error, path: &str) -> Signal {
    let site = match e.raw_os_error() == Some(libc::EISDIR) {
        true => "io_fread",
        false => "rb_sysopen",
    };
    raise_errno(e, site, path)
}

/// [`raise_errno`]'s two-path shape: CRuby prints BOTH operands,
/// parenthesised -- `... @ rb_file_s_rename - (from, to)`. A rename that
/// blamed only one of them named the wrong file half the time.
pub fn raise_errno_pair(e: &std::io::Error, syscall: &str, a: &str, b: &str) -> Signal {
    let (class, desc) = errno_class_and_desc(e);
    raise_error(class, format!("{desc} @ {syscall} - ({a}, {b})"))
}

/// A third shape: the strerror and a TARGET, with no `@ syscall` at all --
/// what a launch failure reports (`No such file or directory - prog`).
pub fn raise_bare_errno_named(e: &std::io::Error, target: &str) -> Signal {
    let (class, desc) = errno_class_and_desc(e);
    raise_error(class, format!("{desc} - {target}"))
}

/// The same mapping with CRuby's OTHER message shape: the bare strerror, no
/// `@ syscall - path` suffix. That is what a failing `close(2)` reports --
/// `Errno::EBADF, "Bad file descriptor"` -- because the operation names no
/// path to blame.
pub fn raise_bare_errno(e: &std::io::Error) -> Signal {
    let (class, desc) = errno_class_and_desc(e);
    raise_error(class, desc)
}

pub(crate) fn errno_class_and_desc_of(e: &std::io::Error) -> (&'static str, String) {
    errno_class_and_desc(e)
}

fn errno_class_and_desc(e: &std::io::Error) -> (&'static str, String) {
    // `raw_os_error` is the whole answer wherever the OS gave one, and
    // `zeo_abi::ERRNO_CLASSES` names a class for every errno this platform
    // defines. The `ErrorKind` arms below only have to cover an error Rust
    // synthesised itself, which carries no OS code.
    let errno = e.raw_os_error().or_else(|| match e.kind() {
        std::io::ErrorKind::NotFound => Some(libc::ENOENT),
        std::io::ErrorKind::PermissionDenied => Some(libc::EACCES),
        std::io::ErrorKind::AlreadyExists => Some(libc::EEXIST),
        _ => None,
    });
    match errno.and_then(zeo_abi::errno_class) {
        Some((_, row)) => (row.name, crate::builtins::exception::strerror(row.errno)),
        // An errno with no dedicated class: SystemCallError is its own
        // parent and CRuby's own fallback for unmapped codes.
        None => ("SystemCallError", "Unknown error".to_string()),
    }
}

/// Run `File`'s class-method row `name` on behalf of `FileTest`, which mixes in
/// the same 26 predicates CRuby shares between the two. The receiver a
/// predicate row sees is the class value and every one of them ignores it, so
/// passing `File` keeps the `BuiltinMethodFn` ABI satisfied without pretending
/// `FileTest` is a class.
pub(crate) fn file_test_forward(name: &str, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let row = lookup_class(name).expect("every FileTest row names a File class method");
    row(&RubyValue::Class(zeo_abi::FILE_CLASS), args, None)
}

/// A path argument -- CRuby's `rb_get_path`: a String, else the `to_path`
/// answer (when the method exists), with the survivor going through the
/// `to_str` protocol. A lying `to_path` therefore reports its ANSWER's
/// class ("no implicit conversion of Integer into String" for `to_path`
/// giving 1 -- oracle-verified, no method-name suffix).
pub fn path_arg(v: &RubyValue, _method: &str) -> Result<String, Signal> {
    match v {
        RubyValue::Str(s) => Ok(s.lock().to_utf8_lossy().into_owned()),
        other => {
            let to_path = crate::Symbol::intern("to_path");
            let candidate = if crate::dispatch::responds_to(other.class_id(), to_path, false) {
                crate::dispatch::send_value(other, to_path, &[], None)?
            } else {
                other.clone()
            };
            Ok(crate::builtins::convert::to_rstr(&candidate)?
                .lock()
                .to_utf8_lossy()
                .into_owned())
        }
    }
}

/// Flatten `File.join`'s arguments: a String (or `to_path`-able) becomes one
/// path component; an Array is recursively flattened.
///
/// An array holding itself is `ArgumentError: recursive array`, as CRuby's
/// `rb_check_array_type` walk reports it; an unguarded descent would run
/// until the machine stack died.
fn collect_join_parts(v: &RubyValue, parts: &mut Vec<String>) -> Result<(), Signal> {
    join_parts(v, parts, &mut crate::value::recursion::Visited::default())
}

fn join_parts(
    v: &RubyValue,
    parts: &mut Vec<String>,
    seen: &mut crate::value::recursion::Visited,
) -> Result<(), Signal> {
    match v {
        RubyValue::Array(el) => {
            let items: Vec<RubyValue> = el.lock().iter().cloned().collect();
            seen.with(v, |seen| {
                for e in &items {
                    join_parts(e, parts, seen)?;
                }
                Ok(())
            })
            .unwrap_or_else(|| Err(arg_error!("recursive array")))
        }
        other => {
            parts.push(path_arg(other, "join")?);
            Ok(())
        }
    }
}

fn str_val(s: String) -> RubyValue {
    RubyValue::Str(crate::collections::string_new(s))
}

/// The raw bytes to write for a value -- a String's own bytes (so a BINARY
/// string round-trips), else its `to_s`.
fn write_bytes(v: &RubyValue) -> Vec<u8> {
    match v {
        RubyValue::Str(s) => s.lock().bytes().to_vec(),
        other => other.to_display_string().into_bytes(),
    }
}

/// The `(external, internal)` encodings a `File.read` should apply, from its
/// trailing options Hash: `mode: "...b..."` forces binary; `encoding:` sets
/// the external (or `"ext:int"` both); `external_encoding:`/
/// `internal_encoding:` override each. Defaults to
/// `(Encoding.default_external, nil)`.
fn read_encodings(
    trailing: Option<&RubyValue>,
) -> Result<
    (
        crate::encoding::EncodingId,
        Option<crate::encoding::EncodingId>,
    ),
    Signal,
> {
    read_encodings_with(trailing, None)
}

/// [`read_encodings`], plus the `:extenc[:intenc]` tail of a POSITIONAL mode
/// string, which the options Hash cannot carry.
///
/// Naming an encoding in both places is CRuby's `ArgumentError: encoding
/// specified twice`, not a silent precedence rule -- and an encoding NAME the
/// tail does not know warns and falls back, where the `encoding:` option
/// raises. Both asymmetries are CRuby's; neither is derivable from the other
/// side.
pub(crate) fn read_encodings_with(
    trailing: Option<&RubyValue>,
    mode_enc: Option<&str>,
) -> Result<
    (
        crate::encoding::EncodingId,
        Option<crate::encoding::EncodingId>,
    ),
    Signal,
> {
    let mut ext = crate::encoding::default_external();
    let mut int = None;
    let named_in_hash = matches!(trailing, Some(RubyValue::Hash(h))
    if ["encoding", "external_encoding", "internal_encoding"].iter().any(|k| {
        !crate::collections::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern(k))).is_nil()
    }));
    // A `mode:` keyword carries the same tail a positional mode string does.
    let from_kwarg = kwarg_str(trailing, "mode").and_then(|m| split_mode(&m).1.map(str::to_string));
    let mode_enc = mode_enc.map(str::to_string).or(from_kwarg);
    if let Some(spec) = mode_enc.as_deref() {
        if named_in_hash {
            return Err(arg_error!("encoding specified twice"));
        }
        apply_encoding_spec(spec, &mut ext, &mut int);
        return Ok((ext, int));
    }
    let Some(RubyValue::Hash(h)) = trailing else {
        return Ok((ext, int));
    };
    let get = |name: &str| {
        crate::collections::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern(name)))
    };
    if let RubyValue::Str(m) = get("mode")
        && split_mode(&m.lock().to_utf8_lossy()).0.contains('b')
    {
        ext = crate::encoding::ASCII_8BIT;
    }
    match get("encoding") {
        // A STRING `encoding:` is a spec, read by the same parser a mode
        // tail is -- so an unknown name warns and falls back. Anything else is
        // an Encoding argument, and an unknown one raises. CRuby's own split:
        // `rb_check_string_type` picks `parse_mode_enc`, else `rb_to_encoding`.
        RubyValue::Str(s) => {
            let spec = s.lock().to_utf8_lossy().into_owned();
            apply_encoding_spec(&spec, &mut ext, &mut int);
        }
        v if !v.is_nil() => ext = crate::builtins::encoding::arg_encoding(&v)?,
        _ => {}
    }
    let external = get("external_encoding");
    if !external.is_nil() {
        ext = crate::builtins::encoding::arg_encoding(&external)?;
    }
    let internal = get("internal_encoding");
    if !internal.is_nil() {
        // `-` asks for NO transcoding rather than naming an encoding, and the
        // test comes before any lookup: `Encoding.find("-")` raises.
        int = match &internal {
            RubyValue::Str(s) if s.lock().to_utf8_lossy() == "-" => None,
            v => Some(crate::builtins::encoding::arg_encoding(v)?),
        };
    }
    Ok((ext, int))
}

/// Apply an encoding SPEC -- `"ext"`, `"ext:int"`, `"BOM|ext"` -- the way
/// CRuby's `parse_mode_enc` does.
///
/// Two rules live only on this path. A name it does not know WARNS and leaves
/// the slot at its default, where `external_encoding:` raises instead. And an
/// internal encoding spelled `-` asks for NO transcoding: there is no encoding
/// by that name, so looking it up would warn about a spelling that is correct.
fn apply_encoding_spec(
    spec: &str,
    ext: &mut crate::encoding::EncodingId,
    int: &mut Option<crate::encoding::EncodingId>,
) {
    let (e, i) = mode_encoding_names(spec);
    match crate::encoding::find(e) {
        Some(id) => *ext = id,
        None => crate::builtins::warning::rb_warn(&format!("Unsupported encoding {e} ignored")),
    }
    match i {
        None | Some("-") => {}
        Some(i) => match crate::encoding::find(i) {
            Some(id) => *int = Some(id),
            None => crate::builtins::warning::rb_warn(&format!("Unsupported encoding {i} ignored")),
        },
    }
}

/// Builds a read string: bytes tagged with `external`, transcoded to
/// `internal` when one is set (and different).
fn build_read_string(
    bytes: Vec<u8>,
    external: crate::encoding::EncodingId,
    internal: Option<crate::encoding::EncodingId>,
) -> Result<crate::RStr, Signal> {
    match internal {
        Some(int) if int != external => {
            let opts = crate::encoding::TranscodeOptions::default();
            let out = crate::encoding::transcode(&bytes, external, int, &opts, None)
                .map_err(crate::encoding::transcode_signal)?;
            Ok(crate::string_from_bytes(out, int))
        }
        _ => Ok(crate::string_from_bytes(bytes, external)),
    }
}

/// Whether a trailing keyword Hash carries `name: <truthy>`. Keywords reach
/// a builtin as one trailing `RubyValue::Hash` (the kwargs convention), so this
/// is the shared reader for the option keywords the File rows accept.
/// An option keyword's raw value from a trailing Hash, `None` when absent --
/// the distinction `kwarg_truthy` collapses, and which a keyword defaulting to
/// true (`autoclose:`) needs.
pub(crate) fn kwarg(trailing: Option<&RubyValue>, name: &str) -> Option<RubyValue> {
    let RubyValue::Hash(h) = trailing? else {
        return None;
    };
    let key = RubyValue::Symbol(crate::Symbol::intern(name));
    match crate::collections::hash_get(h, &key) {
        RubyValue::Nil => None,
        v => Some(v),
    }
}

fn kwarg_truthy(trailing: Option<&RubyValue>, name: &str) -> bool {
    let Some(RubyValue::Hash(h)) = trailing else {
        return false;
    };
    let key = RubyValue::Symbol(crate::Symbol::intern(name));
    // `hash_get` answers nil for a miss, which is falsy -- exactly the
    // "absent means off" rule these option keywords want.
    crate::collections::hash_get(h, &key).truthy()
}

/// `File.new(fd, ...)` -- CRuby's descriptor form. `path:` names the file the
/// handle reports; `autoclose: false` adopts a `dup` instead, so closing this
/// handle leaves the caller's descriptor open (`#fileno` then answers the copy,
/// where CRuby shares the number).
fn file_from_fd(fd: i64, opts: Option<&RubyValue>) -> Result<RubyValue, Signal> {
    use std::os::fd::FromRawFd;
    // Absent means autoclose, so only an explicit falsy value takes the dup.
    let raw = match kwarg(opts, "autoclose") {
        Some(v) if !v.truthy() => {
            let copy = unsafe { libc::dup(fd as libc::c_int) };
            if copy < 0 {
                return Err(raise_errno(&std::io::Error::last_os_error(), "dup", ""));
            }
            copy
        }
        _ => fd as libc::c_int,
    };
    // SAFETY: the caller vouches for the descriptor, as it does for `IO.new`.
    let f = unsafe { std::fs::File::from_raw_fd(raw) };
    Ok(crate::builtins::io::file_value(f, kwarg_str(opts, "path")))
}

/// The String value of an option keyword in a trailing Hash (`mode: "w"`),
/// or `None` when absent / not a String.
pub(crate) fn kwarg_str(trailing: Option<&RubyValue>, name: &str) -> Option<String> {
    let RubyValue::Hash(h) = trailing? else {
        return None;
    };
    let key = RubyValue::Symbol(crate::Symbol::intern(name));
    match crate::collections::hash_get(h, &key) {
        RubyValue::Str(s) => Some(s.lock().to_utf8_lossy().into_owned()),
        _ => None,
    }
}

/// [`kwarg_str`]'s Integer twin -- `perm:` is the only caller.
pub(crate) fn kwarg_int(trailing: Option<&RubyValue>, name: &str) -> Option<i64> {
    let RubyValue::Hash(h) = trailing? else {
        return None;
    };
    let key = RubyValue::Symbol(crate::Symbol::intern(name));
    match crate::collections::hash_get(h, &key) {
        RubyValue::Int(n) => Some(n),
        _ => None,
    }
}

/// Split `bytes` into records terminated by `sep` (each keeps its terminator,
/// like `IO#readlines`), stripping the terminator when `chomp`. An empty `sep`
/// is paragraph mode, which the corpus doesn't use -- treated as "\n\n".
/// The `sep`/`limit` pair `readlines`/`foreach` take after the path. Ruby tells
/// them apart BY TYPE, so either may appear alone: a String is the record
/// separator, an Integer a per-line byte limit. A trailing options Hash (the
/// `chomp:` keyword) is neither and is skipped here.
fn sep_and_limit(args: [Option<&RubyValue>; 2]) -> (String, Option<usize>) {
    let (mut sep, mut limit) = ("\n".to_string(), None);
    for a in args.into_iter().flatten() {
        match a {
            RubyValue::Str(s) => sep = s.lock().to_utf8_lossy().into_owned(),
            RubyValue::Int(n) if *n > 0 => limit = Some(*n as usize),
            _ => {}
        }
    }
    (sep, limit)
}

/// Caps each already-split record at `limit` BYTES, spilling a longer one into
/// further entries -- ruby's `limit` bounds a line's length rather than the
/// number of lines.
fn apply_limit(records: Vec<RubyValue>, limit: Option<usize>) -> Vec<RubyValue> {
    let Some(limit) = limit else {
        return records;
    };
    let mut out = Vec::with_capacity(records.len());
    for r in records {
        let text = r.to_display_string();
        if text.len() <= limit {
            out.push(r);
            continue;
        }
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let end = (i + limit).min(bytes.len());
            out.push(str_val(
                String::from_utf8_lossy(&bytes[i..end]).into_owned(),
            ));
            i = end;
        }
    }
    out
}

fn split_records(bytes: &[u8], sep: &str, chomp: bool) -> Vec<RubyValue> {
    let sep = if sep.is_empty() { "\n\n" } else { sep };
    let sep = sep.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(sep) {
            let end = i + sep.len();
            let piece = if chomp {
                &bytes[start..i]
            } else {
                &bytes[start..end]
            };
            out.push(str_val(String::from_utf8_lossy(piece).into_owned()));
            i = end;
            start = end;
        } else {
            i += 1;
        }
    }
    if start < bytes.len() {
        out.push(str_val(
            String::from_utf8_lossy(&bytes[start..]).into_owned(),
        ));
    }
    out
}

/// `File.fnmatch`'s five flags, CRuby's own values.
const FNM_NOESCAPE: i64 = 1;
const FNM_PATHNAME: i64 = 2;
const FNM_DOTMATCH: i64 = 4;
const FNM_CASEFOLD: i64 = 8;
const FNM_EXTGLOB: i64 = 16;

/// Glob-match `name` against a shell pattern, CRuby's `fnmatch` rules.
///
/// `FNM_EXTGLOB` is expansion rather than matching -- `{a,b}` becomes two
/// patterns, and the braces nest -- so it happens here, once, and the
/// matcher below never sees a brace.
fn fnmatch(pattern: &str, name: &str, flags: i64) -> bool {
    if flags & FNM_EXTGLOB != 0 {
        return expand_braces(pattern)
            .iter()
            .any(|p| fnmatch_one(p, name, flags));
    }
    fnmatch_one(pattern, name, flags)
}

/// `{a,b}` -> `["a", "b"]`, applied to the whole pattern and recursively to
/// what each alternative expands into (`{a,{b,c}}` is three patterns). An
/// UNBALANCED brace is a literal, which is what makes `fnmatch("{a,b}",
/// "{a,b}")` true with the flag as well as without it.
fn expand_braces(pattern: &str) -> Vec<String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut open = None;
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '{' => {
                open = Some(i);
                break;
            }
            _ => {}
        }
        i += 1;
    }
    let Some(open) = open else {
        return vec![pattern.to_string()];
    };
    let mut parts: Vec<String> = Vec::new();
    let mut start = open + 1;
    let mut close = None;
    let mut depth = 0usize;
    let mut j = open + 1;
    while j < chars.len() {
        match chars[j] {
            '\\' => j += 1,
            '{' => depth += 1,
            '}' if depth == 0 => {
                parts.push(chars[start..j].iter().collect());
                close = Some(j);
                break;
            }
            '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(chars[start..j].iter().collect());
                start = j + 1;
            }
            _ => {}
        }
        j += 1;
    }
    let Some(close) = close else {
        return vec![pattern.to_string()];
    };
    let prefix: String = chars[..open].iter().collect();
    let suffix: String = chars[close + 1..].iter().collect();
    parts
        .into_iter()
        .flat_map(|part| expand_braces(&format!("{prefix}{part}{suffix}")))
        .collect()
}

/// One brace-free pattern.
///
/// `period` tracks the LEADING-DOT rule: a `.` at the start of the name --
/// and, under `FNM_PATHNAME`, at the start of every component -- is matched
/// only by a literal `.` in the pattern, unless `FNM_DOTMATCH` is set.
fn fnmatch_one(pattern: &str, name: &str, flags: i64) -> bool {
    let fold = flags & FNM_CASEFOLD != 0;
    let pathname = flags & FNM_PATHNAME != 0;
    let dotmatch = flags & FNM_DOTMATCH != 0;
    let escapes = flags & FNM_NOESCAPE == 0;

    fn same(a: char, b: char, fold: bool) -> bool {
        a == b || (fold && a.to_lowercase().eq(b.to_lowercase()))
    }

    /// Does the pattern begin with a LITERAL `.`?
    fn literal_dot(p: &[char], escapes: bool) -> bool {
        match p.first() {
            Some('.') => true,
            Some('\\') if escapes => p.get(1) == Some(&'.'),
            _ => false,
        }
    }

    /// The index of the `]` closing a set that opens at `p[0]`.
    ///
    /// A `]` in the FIRST position is the close, not a member -- glob's
    /// rule is the other way round, but `fnmatch("[]]", "]")` is false in
    /// CRuby, oracle-verified.
    fn set_close(p: &[char], escapes: bool) -> Option<usize> {
        let mut i = 1;
        if matches!(p.get(i), Some('!') | Some('^')) {
            i += 1;
        }
        while i < p.len() {
            match p[i] {
                '\\' if escapes => i += 1,
                ']' => return Some(i),
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// Whether `ch` is in the bracket set `set` (ranges included).
    fn in_set(set: &[char], ch: char, fold: bool, escapes: bool) -> bool {
        let mut i = 0;
        while i < set.len() {
            let lo = match set[i] {
                '\\' if escapes && i + 1 < set.len() => {
                    i += 1;
                    set[i]
                }
                c => c,
            };
            // `a-z`, but a `-` that ENDS the set is a member of it.
            if set.get(i + 1) == Some(&'-') && i + 2 < set.len() {
                let hi = match set[i + 2] {
                    '\\' if escapes && i + 3 < set.len() => set[i + 3],
                    c => c,
                };
                let hit = (lo..=hi).contains(&ch)
                    || (fold
                        && ch
                            .to_lowercase()
                            .chain(ch.to_uppercase())
                            .any(|c| (lo..=hi).contains(&c)));
                if hit {
                    return true;
                }
                i += if matches!(set[i + 2], '\\') && escapes {
                    4
                } else {
                    3
                };
                continue;
            }
            if same(lo, ch, fold) {
                return true;
            }
            i += 1;
        }
        false
    }

    struct Cx {
        fold: bool,
        pathname: bool,
        dotmatch: bool,
        escapes: bool,
    }

    fn rec(p: &[char], n: &[char], period: bool, cx: &Cx) -> bool {
        if period && n.first() == Some(&'.') && !cx.dotmatch && !literal_dot(p, cx.escapes) {
            return false;
        }
        match p.first() {
            None => n.is_empty(),
            Some('*') => {
                let mut i = 1;
                while p.get(i) == Some(&'*') {
                    i += 1;
                }
                let rest = &p[i..];
                if rest.is_empty() {
                    // `*` never crosses a separator under FNM_PATHNAME.
                    return !(cx.pathname && n.contains(&'/'));
                }
                let mut k = 0;
                loop {
                    if rec(rest, &n[k..], false, cx) {
                        return true;
                    }
                    if k >= n.len() || (cx.pathname && n[k] == '/') {
                        return false;
                    }
                    k += 1;
                }
            }
            Some('?') => match n.first() {
                Some(&c) if !(cx.pathname && c == '/') => rec(&p[1..], &n[1..], false, cx),
                _ => false,
            },
            Some('[') => {
                let Some(close) = set_close(p, cx.escapes) else {
                    // A stray `[` is a literal.
                    return n.first() == Some(&'[') && rec(&p[1..], &n[1..], false, cx);
                };
                let inner = &p[1..close];
                let (set, negate) = match inner.first() {
                    Some('!') | Some('^') => (&inner[1..], true),
                    _ => (inner, false),
                };
                let Some(&ch) = n.first() else { return false };
                // A set never matches the separator under FNM_PATHNAME,
                // negated or not.
                if cx.pathname && ch == '/' {
                    return false;
                }
                (in_set(set, ch, cx.fold, cx.escapes) != negate)
                    && rec(&p[close + 1..], &n[1..], false, cx)
            }
            Some('\\') if cx.escapes && p.len() > 1 => match (p[1], n.first()) {
                (c, Some(&x)) if same(c, x, cx.fold) => {
                    rec(&p[2..], &n[1..], cx.pathname && c == '/', cx)
                }
                _ => false,
            },
            Some(&c) => match n.first() {
                Some(&x) if same(c, x, cx.fold) => {
                    rec(&p[1..], &n[1..], cx.pathname && c == '/', cx)
                }
                _ => false,
            },
        }
    }

    rec(
        &pattern.chars().collect::<Vec<_>>(),
        &name.chars().collect::<Vec<_>>(),
        true,
        &Cx {
            fold,
            pathname,
            dotmatch,
            escapes,
        },
    )
}

/// `File.basename(path)` / `File.basename(path, suffix)`. Pure string work:
/// trailing slashes are stripped first (`File.basename("/a/b/")` is `"b"`),
/// and a `".*"` suffix means "any extension".
pub(crate) fn basename_of(path: &str, suffix: Option<&str>) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        // The path was "/" (or all slashes) -- basename is "/".
        return if path.is_empty() {
            String::new()
        } else {
            "/".to_string()
        };
    }
    let base = trimmed.rsplit('/').next().unwrap_or(trimmed).to_string();
    match suffix {
        None => base,
        Some(".*") => match base.rfind('.') {
            // A leading dot is not an extension (`.bashrc`).
            Some(i) if i > 0 => base[..i].to_string(),
            _ => base,
        },
        Some(sfx) if base.len() > sfx.len() && base.ends_with(sfx) => {
            base[..base.len() - sfx.len()].to_string()
        }
        Some(_) => base,
    }
}

/// `File.dirname` -- everything before the last `/`, or `"."` when there is
/// no `/` at all.
pub(crate) fn dirname_of(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return if path.starts_with('/') {
            "/".to_string()
        } else {
            ".".to_string()
        };
    }
    match trimmed.rfind('/') {
        None => ".".to_string(),
        // A single leading slash IS the dirname: `/x` -> `/`.
        Some(0) => "/".to_string(),
        Some(i) => trimmed[..i].to_string(),
    }
}

/// `File.extname` -- the last `.` onward, empty when there is none or when
/// the only dot LEADS the basename (`.bashrc` is a name, not an extension).
///
/// A TRAILING dot is an extension of its own: `File.extname("foo.")` is
/// `"."` (oracle-verified -- and worth stating, because it reads like an
/// off-by-one; `".."` and `"."` are still empty, since their dots lead).
pub(crate) fn extname_of(path: &str) -> String {
    let base = basename_of(path, None);
    // A name of nothing but dots has no extension (`.`, `..`).
    if base.chars().all(|c| c == '.') {
        return String::new();
    }
    match base.rfind('.') {
        Some(i) if i > 0 => base[i..].to_string(),
        _ => String::new(),
    }
}

/// `File.expand_path` -- absolutize against `base` (default: cwd), resolving
/// `~`, `.` and `..` LEXICALLY (no symlink resolution, which is
/// `realpath`'s job, matching CRuby).
pub(crate) fn expand_path_of(path: &str, base: Option<&str>) -> Result<String, Signal> {
    let start = if path.starts_with('/') {
        String::new()
    } else if let Some(rest) = path.strip_prefix('~') {
        // `~` / `~/x` -- HOME. (`~user` is not supported; it needs the
        // passwd database, which is the `etc` extension's job.)
        if rest.is_empty() || rest.starts_with('/') {
            std::env::var("HOME").unwrap_or_default()
        } else {
            return Err(arg_error!(
                "user {} doesn't exist",
                rest.split('/').next().unwrap_or("")
            ));
        }
    } else {
        match base {
            Some(b) => expand_path_of(b, None)?,
            None => std::env::current_dir()
                .map_err(|e| raise_errno(&e, "getcwd", "."))?
                .to_string_lossy()
                .into_owned(),
        }
    };
    let joined = if path.starts_with('/') {
        path.to_string()
    } else if let Some(rest) = path.strip_prefix('~') {
        format!("{start}{rest}")
    } else {
        format!("{start}/{path}")
    };
    // Resolve `.`/`..` lexically.
    let mut out: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    Ok(format!("/{}", out.join("/")))
}

/// The `File::Stat`-free predicates: `metadata` answers `None` for any error
/// (a missing path, an unreadable parent), which is exactly what every
/// `File.<predicate>?` wants -- they answer false rather than raising.
fn meta(path: &str) -> Option<std::fs::Metadata> {
    std::fs::metadata(path).ok()
}

/// The special file kinds `pipe?`/`socket?`/`blockdev?`/`chardev?` test for.
enum SpecialKind {
    Fifo,
    Socket,
    Block,
    Char,
}

/// Whether `path`'s `stat` (following a final symlink) is of the given special
/// kind. Absent/unreadable -> false.
fn ftype_is(path: &str, kind: SpecialKind) -> bool {
    use std::os::unix::fs::FileTypeExt;
    let Some(m) = meta(path) else { return false };
    let t = m.file_type();
    match kind {
        SpecialKind::Fifo => t.is_fifo(),
        SpecialKind::Socket => t.is_socket(),
        SpecialKind::Block => t.is_block_device(),
        SpecialKind::Char => t.is_char_device(),
    }
}

/// Whether `path`'s permission bits include every bit in `mask` (the set-uid,
/// set-gid, and sticky checks). Absent/unreadable -> false.
fn mode_has(path: &str, mask: libc::mode_t) -> bool {
    use std::os::unix::fs::MetadataExt;
    let mask = crate::mode_u32(mask);
    meta(path).is_some_and(|m| (m.mode() & mask) == mask)
}

/// The `(external, internal)` names a mode string's `:extenc[:intenc]` tail
/// carries, with the `bom|` request stripped off the external name.
///
/// zeo reads the BOM on demand (`#set_encoding_by_bom`) rather than at open,
/// so the prefix decides nothing here; CRuby answers `UTF-8` for
/// `"r:bom|utf-8"` either way.
fn mode_encoding_names(spec: &str) -> (&str, Option<&str>) {
    let (ext, int) = match spec.split_once(':') {
        Some((e, i)) => (e, Some(i)),
        None => (spec, None),
    };
    let ext = ext
        .strip_prefix("bom|")
        .or_else(|| ext.strip_prefix("BOM|"))
        .unwrap_or(ext);
    (ext, int)
}

/// A whole-second `timespec` -- what `utimensat` takes where `utimes` takes a
/// `timeval`.
fn timespec_secs(secs: libc::time_t) -> libc::timespec {
    libc::timespec {
        tv_sec: secs,
        tv_nsec: 0,
    }
}

/// A `chown`/`lchown` uid or gid argument. `nil` means "leave this half
/// alone", which the kernel spells as `-1`.
fn owner_id(v: &RubyValue, who: &str) -> Result<libc::uid_t, Signal> {
    match v {
        RubyValue::Nil => Ok(libc::uid_t::MAX),
        other => Ok(crate::builtins::convert::to_index(other)
            .map_err(|_| type_error!("no implicit conversion into Integer for {who}"))?
            as libc::uid_t),
    }
}

/// `lchmod(2)` where the platform has it. macOS does; Linux does not, and
/// CRuby raises `NotImplementedError` there, so the caller reports the same by
/// way of the `false` this answers with `ENOSYS` set.
fn lchmod_at(path: &std::ffi::CStr, mode: libc::mode_t) -> bool {
    #[cfg(target_vendor = "apple")]
    {
        // The `libc` crate has no binding for it, so declare the one symbol.
        unsafe extern "C" {
            fn lchmod(path: *const libc::c_char, mode: libc::mode_t) -> libc::c_int;
        }
        // SAFETY: `path` is a valid NUL-terminated C string.
        unsafe { lchmod(path.as_ptr(), mode) == 0 }
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        let _ = (path, mode);
        // SAFETY: `__errno_location` returns a live per-thread errno slot.
        unsafe { *libc::__errno_location() = libc::ENOSYS };
        false
    }
}

/// `utime`/`lutime`'s "stamp both with NOW": ruby reads the pair only when at
/// least one of them is non-nil, so BOTH nil is the null time array.
fn now_times(atime: &RubyValue, mtime: &RubyValue) -> bool {
    matches!(atime, RubyValue::Nil) && matches!(mtime, RubyValue::Nil)
}

fn time_secs(v: &RubyValue) -> Result<libc::time_t, Signal> {
    match v {
        RubyValue::Int(i) => Ok(*i as libc::time_t),
        RubyValue::Float(f) => Ok(*f as libc::time_t),
        // `time_timespec`'s own protocol: `to_time`, then that Time's second
        // count. NOT `to_i` -- every String answers `to_i` with 0, so
        // `File.utime("x", "x", path)` stamped the epoch where ruby says
        // "can't convert String into time". Nor the generic implicit
        // conversion, whose message is a different sentence.
        // A real Time answers with its own second count.
        RubyValue::Object(o) if o.as_any().is::<crate::builtins::time::RTime>() => {
            match crate::dispatch::send_value(v, crate::Symbol::intern("to_i"), &[], None)? {
                RubyValue::Int(i) => Ok(i as libc::time_t),
                _ => Err(type_error!("can't convert Time into time")),
            }
        }
        other => {
            let t = crate::builtins::convert::check_to_time(other)?.ok_or_else(|| {
                type_error!(
                    "can't convert {} into time",
                    crate::builtins::class_name_of(other)
                )
            })?;
            match crate::dispatch::send_value(&t, crate::Symbol::intern("to_i"), &[], None)? {
                RubyValue::Int(i) => Ok(i as libc::time_t),
                _ => Err(type_error!(
                    "can't convert {} into time",
                    crate::builtins::class_name_of(other)
                )),
            }
        }
    }
}

/// `world_readable?`/`world_writable?`: the permission bits (`mode & 0777`) when
/// `others` hold the access `bit` names, else nil -- CRuby returns the mask, not
/// a boolean.
fn world_perm(path: &str, bit: libc::mode_t) -> RubyValue {
    use std::os::unix::fs::MetadataExt;
    match meta(path) {
        Some(m) if m.mode() & crate::mode_u32(bit) != 0 => {
            RubyValue::Int((m.mode() & 0o777) as i64)
        }
        _ => RubyValue::Nil,
    }
}

/// `File.ftype`'s answer: the file type of `path` WITHOUT following a final
/// symlink (`lstat`), as CRuby's fixed strings.
fn ftype_string(path: &str) -> Result<&'static str, Signal> {
    use std::os::unix::fs::FileTypeExt;
    let md =
        std::fs::symlink_metadata(path).map_err(|e| raise_errno(&e, "rb_file_s_lstat", path))?;
    let ft = md.file_type();
    Ok(if ft.is_symlink() {
        "link"
    } else if ft.is_dir() {
        "directory"
    } else if ft.is_file() {
        "file"
    } else if ft.is_fifo() {
        "fifo"
    } else if ft.is_socket() {
        "socket"
    } else if ft.is_char_device() {
        "characterSpecial"
    } else if ft.is_block_device() {
        "blockSpecial"
    } else {
        "unknown"
    })
}

/// A `File.open` integer mode -- the `O_*` bitmask (`File::WRONLY |
/// File::CREAT | File::TRUNC`). The access mode is the low two bits; the rest
/// are creation/append flags.
fn open_options_int(flags: i64) -> std::fs::OpenOptions {
    let mut o = std::fs::OpenOptions::new();
    match flags & libc::O_ACCMODE as i64 {
        x if x == libc::O_WRONLY as i64 => {
            o.write(true);
        }
        x if x == libc::O_RDWR as i64 => {
            o.read(true).write(true);
        }
        _ => {
            o.read(true);
        }
    }
    if flags & libc::O_APPEND as i64 != 0 {
        o.append(true);
    }
    if flags & libc::O_CREAT as i64 != 0 {
        o.create(true);
    }
    if flags & libc::O_TRUNC as i64 != 0 {
        o.truncate(true);
    }
    if flags & libc::O_EXCL as i64 != 0 {
        o.create_new(true);
    }
    o
}

/// A mode string's ACCESS part and its `:extenc[:intenc]` encoding spec --
/// CRuby's `rb_io_extract_modeenc`, where everything after the first `:` names
/// encodings rather than access.
///
/// Splitting is not cosmetic: `"r:big5"` holds a `b`, so a scan of the whole
/// string for the binary flag called it binmode, and the access matcher saw a
/// mode it had never heard of and refused the open outright.
pub(crate) fn split_mode(mode: &str) -> (&str, Option<&str>) {
    match mode.split_once(':') {
        Some((access, enc)) => (access, Some(enc)),
        None => (mode, None),
    }
}

/// A `File.open` mode string (`"r"`, `"w"`, `"a"`, `"r+"`, ... with an
/// optional `b`/`t` suffix, and an optional `:extenc[:intenc]` tail).
pub(crate) fn open_options(mode: &str) -> Result<std::fs::OpenOptions, Signal> {
    // `rb_io_modestr_oflags` reads the FIRST character, then walks the rest as
    // a set of independent flags -- so `b`, `t`, `+` and `x` may appear in any
    // order, and `"wx+"` names the same open as `"w+x"`.
    let access = split_mode(mode).0;
    let base: String = access
        .chars()
        .filter(|c| *c != 'b' && *c != 't' && *c != 'x')
        .collect();
    // `x` is CREAT|EXCL, which ruby allows only on a mode that CREATES.
    let exclusive = access.contains('x');
    let mut o = std::fs::OpenOptions::new();
    match (base.as_str(), exclusive) {
        ("r", false) => o.read(true),
        ("r+", false) => o.read(true).write(true),
        ("w", _) => o.write(true).create(true).truncate(true),
        ("w+", _) => o.read(true).write(true).create(true).truncate(true),
        ("a", false) => o.append(true).create(true),
        ("a+", false) => o.read(true).append(true).create(true),
        _ => {
            return Err(arg_error!("invalid access mode {mode}"));
        }
    };
    if exclusive {
        o.create_new(true).truncate(false);
    }
    Ok(o)
}

/// The open flags a `File.open`/`IO.sysopen` MODE argument names, in each of
/// the four shapes ruby takes it in -- a mode string, an `O_*` bitmask, a
/// `mode:` keyword in `opts`, or absent (`"r"`) -- plus the PERM bits a newly
/// created file is given.
///
/// Shared so the two entry points cannot drift: `IO.sysopen` and `File.open`
/// honour the same mode and the same `perm`, so `File.open(path, "w",
/// 0o600)` leaves a 0600 file behind.
///
/// A Hash in the POSITIONAL `mode` slot is not options -- keywords have
/// already been peeled -- so it converts to a String and raises, which is
/// where `File.open(path, {mode: "w"})`'s TypeError comes from.
/// A mode argument that is an object offering `to_int` and NOT `to_str`, as
/// the `O_*` bitmask it stands for. Ruby reads the mode slot either way
/// (`rb_io_extract_modeenc`), trying the string spelling first, so a class
/// with both is left to the String path.
pub(crate) fn int_mode(v: &RubyValue) -> Option<Result<RubyValue, Signal>> {
    if matches!(v, RubyValue::Str(_) | RubyValue::Int(_) | RubyValue::Nil) {
        return None;
    }
    let int = crate::Symbol::intern("to_int");
    let has = |n| crate::dispatch::responds_to_value(v, n, true);
    (!has(crate::Symbol::intern("to_str")) && has(int)).then(|| crate::builtins::convert::to_int(v))
}

pub(crate) fn open_options_for(
    mode: Option<&RubyValue>,
    perm: Option<&RubyValue>,
    opts: Option<&RubyValue>,
) -> Result<std::fs::OpenOptions, Signal> {
    let converted = mode.and_then(int_mode).transpose()?;
    let mode = converted.as_ref().or(mode);
    let mut o = match mode {
        None | Some(RubyValue::Nil) => {
            open_options(kwarg_str(opts, "mode").as_deref().unwrap_or("r"))?
        }
        Some(RubyValue::Int(flags)) => open_options_int(*flags),
        Some(v) => open_options(&path_arg(v, "open")?)?,
    };
    // Only an Integer is a permission: the third argument is also where an
    // options Hash lands (`File.open(path, "w", external_encoding: ...)`).
    // `perm:` is the keyword spelling of the same slot, and the positional
    // wins where both are written. umask still applies, exactly as it does to
    // open(2).
    let bits = match perm {
        Some(RubyValue::Int(bits)) => Some(*bits),
        _ => kwarg_int(opts, "perm"),
    };
    if let Some(bits) = bits {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(bits as u32);
    }
    Ok(o)
}

/// `File::Constants` -- the open/lock/fnmatch flags, in their own module
/// because CRuby includes them into `IO` as well as `File`. Its own inline
/// `mod`: one `ruby_class!`/`ruby_module!` per module scope, since each emits
/// a `lookup` of its own.
mod file_constants {
    use crate::RubyValue;
    use zeo_macros::ruby_module;

    ruby_module! {
        Constants = zeo_abi::FILE_CONSTANTS_MODULE;

        // `FNM_*` are Darwin fnmatch flags (SHORTNAME/SYSCASE are 0 on a
        // case-sensitive fs); the `open(2)` and `flock(2)` bits come straight
        // from libc so they match the host headers exactly.
        const FNM_NOESCAPE = RubyValue::Int(1);
        const FNM_PATHNAME = RubyValue::Int(2);
        const FNM_DOTMATCH = RubyValue::Int(4);
        const FNM_CASEFOLD = RubyValue::Int(8);
        const FNM_EXTGLOB = RubyValue::Int(16);
        const FNM_SHORTNAME = RubyValue::Int(0);
        const FNM_SYSCASE = RubyValue::Int(0);
        const RDONLY = RubyValue::Int(libc::O_RDONLY as i64);
        const WRONLY = RubyValue::Int(libc::O_WRONLY as i64);
        const RDWR = RubyValue::Int(libc::O_RDWR as i64);
        const APPEND = RubyValue::Int(libc::O_APPEND as i64);
        const CREAT = RubyValue::Int(libc::O_CREAT as i64);
        const TRUNC = RubyValue::Int(libc::O_TRUNC as i64);
        const EXCL = RubyValue::Int(libc::O_EXCL as i64);
        const NONBLOCK = RubyValue::Int(libc::O_NONBLOCK as i64);
        const LOCK_SH = RubyValue::Int(libc::LOCK_SH as i64);
        const LOCK_EX = RubyValue::Int(libc::LOCK_EX as i64);
        const LOCK_UN = RubyValue::Int(libc::LOCK_UN as i64);
        const LOCK_NB = RubyValue::Int(libc::LOCK_NB as i64);
        const NOCTTY = RubyValue::Int(libc::O_NOCTTY as i64);
        const NOFOLLOW = RubyValue::Int(libc::O_NOFOLLOW as i64);
        const SYNC = RubyValue::Int(libc::O_SYNC as i64);
        const DSYNC = RubyValue::Int(libc::O_DSYNC as i64);
        // Windows-only flags; 0 on POSIX, exactly as CRuby defines them here.
        const BINARY = RubyValue::Int(0);
        const SHARE_DELETE = RubyValue::Int(0);
        const NULL = RubyValue::Str(crate::string_new("/dev/null".to_string()));
    }
}

/// `File.open`'s half of the shared `IO.open` row -- CRuby's
/// `rb_file_initialize`, which a File RECEIVER reaches where an IO receiver
/// reaches `rb_io_initialize`. Answers the open handle; the block form is the
/// caller's business.
///
/// An Integer `path` is a descriptor, exactly as it is for `IO.new`, and the
/// options Hash still applies to it.
pub(crate) fn open_path(
    path: &RubyValue,
    mode: Option<&RubyValue>,
    perm: Option<&RubyValue>,
    opts: Option<&RubyValue>,
) -> Result<RubyValue, Signal> {
    let trailing = opts;
    if let RubyValue::Int(fd) = path {
        return file_from_fd(*fd, trailing);
    }
    let path = path_arg(path, "open")?;
    {
        let flags = open_options_for(mode, perm, opts)?;
        // Gvl-released: open(2) itself can block (a FIFO with no peer).
        let f = crate::gvl::without_gvl(|| flags.open(&path))
            .map_err(|e| raise_errno(&e, "rb_sysopen", &path))?;
        // A `mode:` keyword says everything a positional mode string does.
        let mode_str = match mode {
            Some(RubyValue::Str(s)) => Some(s.lock().to_utf8_lossy().into_owned()),
            _ => kwarg_str(opts, "mode"),
        };
        let io = crate::builtins::io::file_value_mode(f, Some(path), mode_str.clone());
        // A `b` in the mode string IS binmode, which `#binmode?` reports and
        // `#set_encoding_by_bom` requires.
        let binmode = mode_str
            .as_deref()
            .is_some_and(|m| split_mode(m).0.contains('b'));
        if binmode {
            crate::dispatch::send_value(&io, crate::Symbol::intern("binmode"), &[], None)?;
        }
        // ...and it decides how a read TAGS its bytes, along with any explicit
        // `encoding:`. Recorded on the handle only when it differs from the
        // default, so an ordinary text open leaves the slot unset and
        // `set_encoding_by_bom` can still claim it.
        let mode_enc = mode_str.as_deref().and_then(|m| split_mode(m).1);
        let (ext, int) = read_encodings_with(trailing, mode_enc)?;
        // `b` forces ASCII-8BIT only when nothing NAMED an encoding: CRuby
        // answers `UTF-8` for `"rb:UTF-8"` and still reports `#binmode?`.
        let binary = binmode && mode_enc.is_none();
        // A NAMED encoding is recorded even when it matches the default: the
        // `binmode` send above has already claimed the slot for ASCII-8BIT,
        // and `"rb:UTF-8"` reports `#binmode?` true with a UTF-8 external.
        if binary
            || mode_enc.is_some()
            || ext != crate::encoding::default_external()
            || int.is_some()
        {
            let ext = match binary {
                true => crate::encoding::ASCII_8BIT,
                false => ext,
            };
            crate::builtins::io::set_handle_encodings(&io, Some(ext), int);
        }
        Ok(io)
    }
}

/// A blank `File` -- an unopened handle tagged `File`, so `#class` answers
/// `File` where `IoBackend::Uninit` alone would say `IO`.
fn file_allocate() -> crate::RubyValue {
    crate::builtins::io::uninit_io(zeo_abi::FILE_CLASS)
}

ruby_class! {
    File = zeo_abi::FILE_CLASS < zeo_abi::IO_CLASS;

    allocate file_allocate;

    // CRuby refuses to re-run initialize on a File, `allocate`'s blank
    // included -- so the blank is a legal RECEIVER and still not openable
    // through this row.
    private def "initialize" cfunc (_recv, *_args, &_block) {
        Err(crate::builtins::runtime_error!("reinitializing File"))
    }

    // The text-read path: bytes are tagged with the EXTERNAL encoding
    // (default `Encoding.default_external`, UTF-8) WITHOUT validation --
    // CRuby's own rule. `encoding:`/`external_encoding:`/`internal_encoding:`
    // options override it; an internal encoding transcodes the bytes.
    def self."read" cfunc (_recv, path, length?, offset?, opt?) {
        let path = path_arg(path, "read")?;
        let mut bytes = crate::gvl::without_gvl(|| std::fs::read(&path))
            .map_err(|e| raise_read_errno(&e, &path))?;
        // `File.read(path, length, offset)`: drop `offset` leading bytes, then
        // cap at `length` (an Integer positional; a trailing Hash is options).
        if let Some(RubyValue::Int(off)) = offset {
            let off = (*off).max(0) as usize;
            bytes = bytes.split_off(off.min(bytes.len()));
        }
        if let Some(RubyValue::Int(len)) = length {
            bytes.truncate((*len).max(0) as usize);
        }
        let (ext, int) = read_encodings(opt.or(offset).or(length))?;
        Ok(RubyValue::Str(build_read_string(bytes, ext, int)?))
    }
    // `binread` always answers ASCII-8BIT bytes, no transcoding.
    def self."binread" cfunc (_recv, path, length?, offset?) {
        let path = path_arg(path, "binread")?;
        let mut bytes = crate::gvl::without_gvl(|| std::fs::read(&path))
            .map_err(|e| raise_read_errno(&e, &path))?;
        // Same window as `File.read`: skip `offset` bytes, then cap at
        // `length`. An offset past the end answers nil rather than "".
        let past_end = matches!(offset, Some(RubyValue::Int(off)) if (*off).max(0) as usize >= bytes.len());
        if let Some(RubyValue::Int(off)) = offset {
            let off = (*off).max(0) as usize;
            bytes = bytes.split_off(off.min(bytes.len()));
        }
        if length.is_some() && past_end {
            return Ok(RubyValue::Nil);
        }
        if let Some(RubyValue::Int(len)) = length {
            bytes.truncate((*len).max(0) as usize);
        }
        Ok(RubyValue::Str(crate::string_from_bytes(bytes, crate::encoding::ASCII_8BIT)))
    }
    def self."binwrite" cfunc (_recv, arg1, arg2, _arg3?) {
        let path = path_arg(arg1, "binwrite")?;
        let data = write_bytes(arg2);
        crate::gvl::without_gvl(|| std::fs::write(&path, &data))
            .map_err(|e| raise_errno(&e, "rb_sysopen", &path))?;
        Ok(RubyValue::Int(data.len() as i64))
    }
    def self."readlines" cfunc (_recv, path, sep?, opt?) {
        let path = path_arg(path, "readlines")?;
        let bytes = crate::gvl::without_gvl(|| std::fs::read(&path))
            .map_err(|e| raise_read_errno(&e, &path))?;
        // A String positional after the path is the record separator (default
        // "\n"); `chomp: true` (trailing Hash) strips it.
        let chomp = kwarg_truthy(opt.or(sep), "chomp");
        let (sep, limit) = sep_and_limit([sep, opt]);
        Ok(RubyValue::Array(crate::collections::array_new(apply_limit(
            split_records(&bytes, &sep, chomp),
            limit,
        ))))
    }
    // `File.foreach(path)` -- yield each line; without a block, an Enumerator.
    // `chomp: true` strips terminators, mirroring `readlines`.
    def self."foreach" cfunc (recv, path, sep?, opt?, &block) {
        let path = path_arg(path, "foreach")?;
        let p = block_or_enum!(recv, __args, block);
        let bytes = crate::gvl::without_gvl(|| std::fs::read(&path))
            .map_err(|e| raise_read_errno(&e, &path))?;
        let chomp = kwarg_truthy(opt.or(sep), "chomp");
        // Through the same (sep, limit) reader `readlines` uses: this used to
        // split on newlines unconditionally, so an explicit separator was
        // accepted and ignored.
        let (sep, limit) = sep_and_limit([sep, opt]);
        for line in apply_limit(split_records(&bytes, &sep, chomp), limit) {
            p.call(&[line])?;
        }
        Ok(RubyValue::Nil)
    }
    // `File.ftype(path)` -- the `lstat` file-type string (doesn't follow a
    // trailing symlink).
    def self."ftype" (_recv, arg) {
        let path = path_arg(arg, "ftype")?;
        Ok(str_val(ftype_string(&path)?.to_string()))
    }
    // `File.stat(path)` follows a final symlink; `File.lstat(path)` does not.
    def self."stat" (_recv, arg) {
        crate::builtins::stat::stat_from_path(&path_arg(arg, "stat")?, true)
    }
    def self."lstat" (_recv, arg) {
        crate::builtins::stat::stat_from_path(&path_arg(arg, "lstat")?, false)
    }
    // `File.truncate(path, len)` -- resize to `len` bytes; answers 0.
    def self."truncate" (_recv, arg1, arg2) {
        let path = path_arg(arg1, "truncate")?;
        let len = &match arg2 {
            RubyValue::Nil => {
                return Err(type_error!("no implicit conversion from nil"));
            }
            v => crate::builtins::convert::to_index(v)?,
        };
        let f = std::fs::OpenOptions::new().write(true).open(&path)
            .map_err(|e| raise_errno(&e, "rb_file_s_truncate", &path))?;
        f.set_len((*len).max(0) as u64).map_err(|e| raise_errno(&e, "rb_file_s_truncate", &path))?;
        Ok(RubyValue::Int(0))
    }
    // `File.absolute_path(path [, base])` -- like `expand_path` but WITHOUT
    // `~` expansion (a leading `~` stays literal).
    def self."absolute_path" cfunc (_recv, arg1, arg2?) {
        let p = path_arg(arg1, "absolute_path")?;
        let base = match arg2 {
            None | Some(RubyValue::Nil) => None,
            Some(v) => Some(path_arg(v, "absolute_path")?),
        };
        Ok(str_val(expand_path_of(&p, base.as_deref())?))
    }
    // `File.path(obj)` -- the path String of a String or `to_path`-able object.
    def self."path" (_recv, arg) {
        Ok(str_val(path_arg(arg, "path")?))
    }
    // `File.fnmatch(pattern, path [, flags])` / `fnmatch?` -- glob match.
    def self."fnmatch" | "fnmatch?" cfunc (_recv, arg1, arg2, arg3?) {
        let pat = path_arg(arg1, "fnmatch")?;
        let name = path_arg(arg2, "fnmatch")?;
        let flags = match arg3 {
            Some(RubyValue::Int(n)) => *n,
            _ => 0,
        };
        Ok(RubyValue::Bool(fnmatch(&pat, &name, flags)))
    }
    def self."write" cfunc (_recv, arg1, arg2, arg3?) {
        let path = path_arg(arg1, "write")?;
        // Bytes are written VERBATIM (a String emits its own bytes, so a
        // BINARY string round-trips unchanged).
        let data = write_bytes(arg2);
        // The third argument is either an Integer offset (write in place,
        // WITHOUT truncating) or an options Hash (`mode: "a"` to append).
        match arg3 {
            Some(RubyValue::Int(off)) => {
                use std::io::{Seek, Write};
                // No truncate: a positional write patches bytes at `off`,
                // leaving the rest of an existing file intact (CRuby's
                // File.write with an offset opens without O_TRUNC).
                let mut f = std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(&path)
                    .map_err(|e| raise_errno(&e, "rb_sysopen", &path))?;
                f.seek(std::io::SeekFrom::Start((*off).max(0) as u64))
                    .map_err(|e| raise_errno(&e, "rb_sysopen", &path))?;
                f.write_all(&data).map_err(|e| raise_errno(&e, "write", &path))?;
            }
            Some(RubyValue::Hash(_)) if kwarg_str(arg3, "mode").as_deref() == Some("a") => {
                use std::io::Write;
                let mut f = std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&path)
                    .map_err(|e| raise_errno(&e, "rb_sysopen", &path))?;
                f.write_all(&data).map_err(|e| raise_errno(&e, "write", &path))?;
            }
            _ => crate::gvl::without_gvl(|| std::fs::write(&path, &data))
                .map_err(|e| raise_errno(&e, "rb_sysopen", &path))?,
        }
        Ok(RubyValue::Int(data.len() as i64))
    }
    // `File.exists?` (with the trailing `s`) was removed in Ruby 3.2 -- only
    // `exist?` remains, so the misspelling raises NoMethodError, as Dir.exist?
    // already does (no `exists?` alias).
    def self."exist?" (_recv, arg) {
        Ok(RubyValue::Bool(meta(&path_arg(arg, "exist?")?).is_some()))
    }
    def self."file?" (_recv, arg) {
        Ok(RubyValue::Bool(
            meta(&path_arg(arg, "file?")?).is_some_and(|m| m.is_file()),
        ))
    }
    def self."directory?" (_recv, arg) {
        Ok(RubyValue::Bool(
            meta(&path_arg(arg, "directory?")?).is_some_and(|m| m.is_dir()),
        ))
    }
    def self."size" (_recv, arg) {
        let path = path_arg(arg, "size")?;
        let m = std::fs::metadata(&path).map_err(|e| raise_errno(&e, "rb_file_s_size", &path))?;
        Ok(RubyValue::Int(m.len() as i64))
    }
    def self."size?" (_recv, arg) {
        // `size?` answers nil for a missing file AND for an empty one --
        // it is the "is there content" predicate, not a size reader.
        Ok(match meta(&path_arg(arg, "size?")?) {
            Some(m) if m.len() > 0 => RubyValue::Int(m.len() as i64),
            _ => RubyValue::Nil,
        })
    }
    def self."zero?" | "empty?" (_recv, arg) {
        Ok(RubyValue::Bool(
            meta(&path_arg(arg, "zero?")?).is_some_and(|m| m.len() == 0),
        ))
    }
    // The permission predicates come in two flavours: the plain ones ask the
    // EFFECTIVE uid/gid, the `*_real?` ones the real one.
    def self."readable?" (_recv, arg) {
        let p = path_arg(arg, "readable?")?;
        Ok(RubyValue::Bool(access_eff(&p, libc::R_OK)))
    }
    def self."writable?" (_recv, arg) {
        let p = path_arg(arg, "writable?")?;
        Ok(RubyValue::Bool(access_eff(&p, libc::W_OK)))
    }
    def self."readable_real?" (_recv, arg) {
        let p = path_arg(arg, "readable_real?")?;
        Ok(RubyValue::Bool(access(&p, libc::R_OK)))
    }
    def self."writable_real?" (_recv, arg) {
        let p = path_arg(arg, "writable_real?")?;
        Ok(RubyValue::Bool(access(&p, libc::W_OK)))
    }
    def self."executable_real?" (_recv, arg) {
        let p = path_arg(arg, "executable_real?")?;
        Ok(RubyValue::Bool(access(&p, libc::X_OK)))
    }
    // File-type predicates (follow a final symlink, unlike `ftype`'s lstat).
    def self."pipe?" (_recv, arg) {
        Ok(RubyValue::Bool(ftype_is(&path_arg(arg, "pipe?")?, SpecialKind::Fifo)))
    }
    def self."socket?" (_recv, arg) {
        Ok(RubyValue::Bool(ftype_is(&path_arg(arg, "socket?")?, SpecialKind::Socket)))
    }
    def self."blockdev?" (_recv, arg) {
        Ok(RubyValue::Bool(ftype_is(&path_arg(arg, "blockdev?")?, SpecialKind::Block)))
    }
    def self."chardev?" (_recv, arg) {
        Ok(RubyValue::Bool(ftype_is(&path_arg(arg, "chardev?")?, SpecialKind::Char)))
    }
    // Set-user/group-id and sticky bits.
    def self."setuid?" (_recv, arg) {
        Ok(RubyValue::Bool(mode_has(&path_arg(arg, "setuid?")?, libc::S_ISUID)))
    }
    def self."setgid?" (_recv, arg) {
        Ok(RubyValue::Bool(mode_has(&path_arg(arg, "setgid?")?, libc::S_ISGID)))
    }
    def self."sticky?" (_recv, arg) {
        Ok(RubyValue::Bool(mode_has(&path_arg(arg, "sticky?")?, libc::S_ISVTX)))
    }
    // Ownership by the effective uid/gid.
    def self."owned?" (_recv, arg) {
        use std::os::unix::fs::MetadataExt;
        let p = path_arg(arg, "owned?")?;
        Ok(RubyValue::Bool(meta(&p).is_some_and(|m| m.uid() == unsafe { libc::geteuid() })))
    }
    def self."grpowned?" (_recv, arg) {
        use std::os::unix::fs::MetadataExt;
        let p = path_arg(arg, "grpowned?")?;
        Ok(RubyValue::Bool(meta(&p).is_some_and(|m| m.gid() == unsafe { libc::getegid() })))
    }
    // `File.identical?(a, b)` -- same device and inode.
    def self."identical?" (_recv, arg1, arg2) {
        use std::os::unix::fs::MetadataExt;
        let a = path_arg(arg1, "identical?")?;
        let b = path_arg(arg2, "identical?")?;
        let same = match (meta(&a), meta(&b)) {
            (Some(x), Some(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
            _ => false,
        };
        Ok(RubyValue::Bool(same))
    }
    // `File.atime(path)` -- last access time as a Time.
    def self."atime" (_recv, arg) {
        let path = path_arg(arg, "atime")?;
        let m = std::fs::metadata(&path).map_err(|e| raise_errno(&e, "rb_file_s_atime", &path))?;
        let t = m
            .accessed()
            .map_err(|e| raise_errno(&e, "rb_file_s_atime", &path))?
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| crate::builtins::system_call_error!("atime before the epoch"))?;
        Ok(crate::builtins::time::time_from_parts(t.as_secs() as i64, t.subsec_nanos()))
    }
    // `File.ctime(path)` -- inode change time as a Time (st_ctime, which std
    // exposes only through the unix `ctime`/`ctime_nsec` seconds pair).
    def self."ctime" (_recv, arg) {
        use std::os::unix::fs::MetadataExt;
        let path = path_arg(arg, "ctime")?;
        let m = std::fs::metadata(&path).map_err(|e| raise_errno(&e, "rb_file_s_ctime", &path))?;
        Ok(crate::builtins::time::time_from_parts(m.ctime(), m.ctime_nsec() as u32))
    }
    // `File.world_readable?`/`world_writable?` -- the low permission bits
    // (`mode & 0777`) when others have the access, else nil (CRuby's contract).
    def self."world_readable?" (_recv, arg) {
        Ok(world_perm(&path_arg(arg, "world_readable?")?, libc::S_IROTH))
    }
    def self."world_writable?" (_recv, arg) {
        Ok(world_perm(&path_arg(arg, "world_writable?")?, libc::S_IWOTH))
    }
    // `File.birthtime(path)` -- the creation time as a Time (st_birthtime).
    def self."birthtime" (_recv, arg) {
        let path = path_arg(arg, "birthtime")?;
        let m = std::fs::metadata(&path).map_err(|e| raise_errno(&e, "rb_file_s_birthtime", &path))?;
        let t = m
            .created()
            .map_err(|e| raise_errno(&e, "rb_file_s_birthtime", &path))?
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| crate::builtins::system_call_error!("birthtime before the epoch"))?;
        Ok(crate::builtins::time::time_from_parts(t.as_secs() as i64, t.subsec_nanos()))
    }
    // `File.link(old, new)` -- create a hard link; answers 0.
    def self."link" (_recv, arg1, arg2) {
        let old = path_arg(arg1, "link")?;
        let new = path_arg(arg2, "link")?;
        std::fs::hard_link(&old, &new).map_err(|e| raise_errno(&e, "syserr_fail2_in", &new))?;
        Ok(RubyValue::Int(0))
    }
    // `File.realpath(path [, dir])` -- the absolute, symlink-resolved path.
    // Every component, the last one included, must exist.
    def self."realpath" cfunc (_recv, path, dir?) {
        let joined = realpath_join(path, dir, "realpath")?;
        let real = std::fs::canonicalize(&joined).map_err(|e| raise_errno(&e, "rb_check_realpath_internal", &joined))?;
        Ok(str_val(real.to_string_lossy().into_owned()))
    }
    // `File.realdirpath(path [, dir])` -- `realpath` where only the DIRECTORY
    // part must exist. A resolvable path answers exactly what `realpath` does,
    // symlinked last component included; otherwise the parent resolves and the
    // basename rides along unresolved.
    def self."realdirpath" cfunc (_recv, path, dir?) {
        let joined = realpath_join(path, dir, "realdirpath")?;
        if let Ok(real) = std::fs::canonicalize(&joined) {
            return Ok(str_val(real.to_string_lossy().into_owned()));
        }
        let path = std::path::Path::new(&joined);
        let (Some(parent), Some(base)) = (path.parent(), path.file_name()) else {
            return Err(raise_errno(
                &std::io::Error::from(std::io::ErrorKind::NotFound),
                "realdirpath",
                &joined,
            ));
        };
        let parent = if parent.as_os_str().is_empty() { std::path::Path::new(".") } else { parent };
        let real = std::fs::canonicalize(parent)
            .map_err(|e| raise_errno(&e, "realdirpath", &joined))?;
        Ok(str_val(real.join(base).to_string_lossy().into_owned()))
    }
    // `File.symlink(target, link)` -- create a symbolic link; answers 0.
    def self."symlink" (_recv, arg1, arg2) {
        let target = path_arg(arg1, "symlink")?;
        let link = path_arg(arg2, "symlink")?;
        std::os::unix::fs::symlink(&target, &link).map_err(|e| raise_errno(&e, "syserr_fail2_in", &link))?;
        Ok(RubyValue::Int(0))
    }
    def self."symlink?" (_recv, arg) {
        let p = path_arg(arg, "symlink?")?;
        Ok(RubyValue::Bool(
            std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()),
        ))
    }
    // `File.mkfifo(path, mode = 0666)` -- create a FIFO special file; answers 0
    // (`libc::mkfifo`, the process umask applies to `mode` as usual).
    def self."mkfifo" cfunc (_recv, path, mode?, &_block) {
        let path = path_arg(path, "mkfifo")?;
        let mode: libc::mode_t = match mode {
            Some(RubyValue::Nil) => {
                return Err(type_error!("no implicit conversion of nil into Integer"));
            }
            Some(v) => crate::builtins::convert::to_index(v)? as libc::mode_t,
            None => 0o666,
        };
        let c = std::ffi::CString::new(path.clone())
            .map_err(|_| arg_error!("string contains null byte"))?;
        // SAFETY: `c` is a valid NUL-terminated path.
        if unsafe { libc::mkfifo(c.as_ptr(), mode) } != 0 {
            return Err(raise_errno(&std::io::Error::last_os_error(), "mkfifo", &path));
        }
        Ok(RubyValue::Int(0))
    }
    // `File.readlink(link)` -- the path a symlink points to.
    def self."readlink" (_recv, arg) {
        let p = path_arg(arg, "readlink")?;
        let target = std::fs::read_link(&p).map_err(|e| raise_errno(&e, "rb_readlink", &p))?;
        Ok(str_val(target.to_string_lossy().into_owned()))
    }
    // `File.utime(atime, mtime, *paths)` -- set each file's access and
    // modification times; answers the number of files touched.
    def self."utime" cfunc (_recv, atime, mtime, *paths, &_block) {
        // BOTH nil means NOW, which `utimes` spells as a null time array.
        // Only one nil is an ordinary unconvertible argument (`utime_internal`
        // asks for the pair only when at least one is non-nil).
        let both_nil = now_times(atime, mtime);
        let tv = match both_nil {
            true => [libc::timeval { tv_sec: 0, tv_usec: 0 }; 2],
            false => [
                libc::timeval { tv_sec: time_secs(atime)?, tv_usec: 0 },
                libc::timeval { tv_sec: time_secs(mtime)?, tv_usec: 0 },
            ],
        };
        let tvp = if both_nil { std::ptr::null() } else { tv.as_ptr() };
        for p in paths {
            let path = path_arg(p, "utime")?;
            let c = std::ffi::CString::new(path.clone())
                .map_err(|_| arg_error!("string contains null byte"))?;
            // SAFETY: `c` is a valid NUL-terminated path, and `tvp` is either
            // null (meaning now) or a 2-element array that outlives the call.
            if unsafe { libc::utimes(c.as_ptr(), tvp) } != 0 {
                return Err(raise_errno(&std::io::Error::last_os_error(), "apply2files", &path));
            }
        }
        Ok(RubyValue::Int(paths.len() as i64))
    }
    // `File.lutime(atime, mtime, *paths)` -- `utime` that does NOT follow a
    // final symlink, so it stamps the link itself.
    def self."lutime" cfunc (_recv, atime, mtime, *paths, &_block) {
        // Both nil means NOW -- see `utime`. `utimensat` spells it null too.
        let both_nil = now_times(atime, mtime);
        let tv = match both_nil {
            true => [timespec_secs(0); 2],
            false => [
                timespec_secs(time_secs(atime)?),
                timespec_secs(time_secs(mtime)?),
            ],
        };
        let tvp = if both_nil { std::ptr::null() } else { tv.as_ptr() };
        for p in paths {
            let path = path_arg(p, "lutime")?;
            let c = std::ffi::CString::new(path.clone())
                .map_err(|_| arg_error!("string contains null byte"))?;
            // SAFETY: `c` is a valid NUL-terminated path, and `tvp` is either
            // null (meaning now) or a 2-element array that outlives the call.
            // `AT_FDCWD` resolves it relative to the cwd, as `utimes` does.
            let rc = unsafe {
                libc::utimensat(libc::AT_FDCWD, c.as_ptr(), tvp, libc::AT_SYMLINK_NOFOLLOW)
            };
            if rc != 0 {
                return Err(raise_errno(&std::io::Error::last_os_error(), "apply2files", &path));
            }
        }
        Ok(RubyValue::Int(paths.len() as i64))
    }
    // `File.chown(uid, gid, *paths)` -- set each file's owner and group;
    // answers the number of files changed. A nil uid or gid leaves that half
    // alone, which the kernel spells as -1.
    def self."chown" cfunc (_recv, uid, gid, *paths, &_block) {
        let (uid, gid) = (owner_id(uid, "uid")?, owner_id(gid, "gid")?);
        for p in paths {
            let path = path_arg(p, "chown")?;
            let c = std::ffi::CString::new(path.clone())
                .map_err(|_| arg_error!("string contains null byte"))?;
            // SAFETY: `c` is a valid NUL-terminated path.
            if unsafe { libc::chown(c.as_ptr(), uid, gid) } != 0 {
                return Err(raise_errno(&std::io::Error::last_os_error(), "apply2files", &path));
            }
        }
        Ok(RubyValue::Int(paths.len() as i64))
    }
    // `File.lchown` -- `chown` on the LINK rather than its target.
    def self."lchown" cfunc (_recv, uid, gid, *paths, &_block) {
        let (uid, gid) = (owner_id(uid, "uid")?, owner_id(gid, "gid")?);
        for p in paths {
            let path = path_arg(p, "lchown")?;
            let c = std::ffi::CString::new(path.clone())
                .map_err(|_| arg_error!("string contains null byte"))?;
            // SAFETY: `c` is a valid NUL-terminated path.
            if unsafe { libc::lchown(c.as_ptr(), uid, gid) } != 0 {
                return Err(raise_errno(&std::io::Error::last_os_error(), "apply2files", &path));
            }
        }
        Ok(RubyValue::Int(paths.len() as i64))
    }
    // `File.lchmod` -- `chmod` on the LINK. Not every platform has the
    // syscall; where it is absent CRuby raises NotImplementedError, and so
    // does this.
    def self."lchmod" cfunc (_recv, mode, *paths, &_block) {
        let mode = crate::builtins::convert::to_index(mode)? as libc::mode_t;
        for p in paths {
            let path = path_arg(p, "lchmod")?;
            let c = std::ffi::CString::new(path.clone())
                .map_err(|_| arg_error!("string contains null byte"))?;
            if !lchmod_at(&c, mode) {
                return Err(raise_errno(&std::io::Error::last_os_error(), "apply2files", &path));
            }
        }
        Ok(RubyValue::Int(paths.len() as i64))
    }
    // `File.umask` -- the current file-creation mask; `File.umask(mask)` sets it
    // and answers the previous value. Reading is non-destructive (set-then-restore).
    def self."umask" (_recv, arg?) {
        match arg {
            Some(v) => {
                let new = match v {
                    RubyValue::Nil => {
                        return Err(type_error!("no implicit conversion of nil into Integer"));
                    }
                    v => crate::builtins::convert::to_index(v)? as libc::mode_t,
                };
                Ok(RubyValue::Int(unsafe { libc::umask(new) } as i64))
            }
            None => {
                // No portable getter: set to 0, read the old value, restore it.
                let old = unsafe { libc::umask(0) };
                unsafe { libc::umask(old) };
                Ok(RubyValue::Int(old as i64))
            }
        }
    }
    // `File.chmod(mode, *paths)` -- set each file's permission bits; answers the
    // number of files changed.
    def self."chmod" cfunc (_recv, mode, *paths, &_block) {
        use std::os::unix::fs::PermissionsExt;
        let mode = &match mode {
            RubyValue::Nil => {
                return Err(type_error!("no implicit conversion of nil into Integer"));
            }
            v => crate::builtins::convert::to_index(v)?,
        };
        for p in paths {
            let path = path_arg(p, "chmod")?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(*mode as u32))
                .map_err(|e| raise_errno(&e, "apply2files", &path))?;
        }
        Ok(RubyValue::Int(paths.len() as i64))
    }
    def self."executable?" (_recv, arg) {
        let p = path_arg(arg, "executable?")?;
        Ok(RubyValue::Bool(access_eff(&p, libc::X_OK)))
    }
    def self."delete" | "unlink" (_recv, *args, &_block) {
        // Variadic: deletes every path given, answers how many.
        let mut n = 0;
        for a in args {
            let path = path_arg(a, "delete")?;
            std::fs::remove_file(&path).map_err(|e| raise_errno(&e, "apply2files", &path))?;
            n += 1;
        }
        Ok(RubyValue::Int(n))
    }
    def self."rename" (_recv, arg1, arg2) {
        let from = path_arg(arg1, "rename")?;
        let to = path_arg(arg2, "rename")?;
        std::fs::rename(&from, &to)
            .map_err(|e| raise_errno_pair(&e, "rb_file_s_rename", &from, &to))?;
        Ok(RubyValue::Int(0))
    }
    // --- The pure-path family: string work, never touches the disk --------
    def self."basename" cfunc (_recv, arg1, arg2?) {
        let path = path_arg(arg1, "basename")?;
        let suffix = match arg2 {
            None => None,
            Some(v) => Some(path_arg(v, "basename")?),
        };
        Ok(str_val(basename_of(&path, suffix.as_deref())))
    }
    def self."dirname" cfunc (_recv, arg1, arg2?) {
        let mut path = path_arg(arg1, "dirname")?;
        // `File.dirname(path, level)` strips `level` trailing components.
        let level = match arg2 {
            Some(RubyValue::Int(n)) => *n,
            _ => 1,
        };
        for _ in 0..level {
            path = dirname_of(&path);
        }
        Ok(str_val(path))
    }
    def self."extname" (_recv, arg) {
        Ok(str_val(extname_of(&path_arg(arg, "extname")?)))
    }
    def self."split" (_recv, arg) {
        let p = path_arg(arg, "split")?;
        Ok(RubyValue::Array(crate::collections::array_new(vec![
            str_val(dirname_of(&p)),
            str_val(basename_of(&p, None)),
        ])))
    }
    def self."join" (_recv, *args, &_block) {
        // `File.join("a", ["b", ["c"]])` flattens arbitrarily nested arrays.
        let mut parts = Vec::new();
        for a in args {
            collect_join_parts(a, &mut parts)?;
        }
        // Ruby collapses a separator at the seam rather than doubling it:
        // `File.join("a/", "/b")` is `"a/b"`.
        let mut out = String::new();
        for (i, p) in parts.iter().enumerate() {
            if i == 0 {
                out.push_str(p);
                continue;
            }
            let left_slash = out.ends_with('/');
            let right_slash = p.starts_with('/');
            match (left_slash, right_slash) {
                (true, true) => out.push_str(p.trim_start_matches('/')),
                (false, false) => {
                    out.push('/');
                    out.push_str(p);
                }
                _ => out.push_str(p),
            }
        }
        Ok(str_val(out))
    }
    def self."expand_path" cfunc (_recv, arg1, arg2?) {
        let p = path_arg(arg1, "expand_path")?;
        let base = match arg2 {
            None => None,
            Some(v) => Some(path_arg(v, "expand_path")?),
        };
        Ok(str_val(expand_path_of(&p, base.as_deref())?))
    }
    def self."absolute_path?" (_recv, arg) {
        Ok(RubyValue::Bool(path_arg(arg, "absolute_path?")?.starts_with('/')))
    }
    def self."mtime" (_recv, arg) {
        let path = path_arg(arg, "mtime")?;
        let m = std::fs::metadata(&path).map_err(|e| raise_errno(&e, "rb_file_s_mtime", &path))?;
        let t = m
            .modified()
            .map_err(|e| raise_errno(&e, "rb_file_s_mtime", &path))?
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| crate::builtins::system_call_error!("mtime before the epoch"))?;
        Ok(crate::builtins::time::time_from_parts(
            t.as_secs() as i64,
            t.subsec_nanos(),
        ))
    }

    // -- the open-descriptor surface CRuby puts on File, not IO. Each one
    // needs a path or a real file behind the fd, which is exactly why a pipe
    // or a socket does not answer them. The bodies reach io.rs's `with_file`,
    // since the receiver is still an `RIo`.

    // `#flock(op)` -- advisory whole-file lock via `flock(2)`; answers 0.
    def "flock" (recv, operation, &_blk) {
        use std::os::fd::AsRawFd;
        let op = crate::builtins::convert::to_index(operation)?;
        crate::builtins::io::with_file(recv, |f, path| {
            // SAFETY: `f` owns a valid fd for the call's duration.
            if unsafe { libc::flock(f.as_raw_fd(), op as libc::c_int) } != 0 {
                return Err(raise_errno(&std::io::Error::last_os_error(), "flock", path));
            }
            Ok(RubyValue::Int(0))
        })
    }

    // `#lstat` -- stat the open file's path WITHOUT following a final symlink.
    // Unlike `#stat` (which `fstat`s the fd), this must go through the stored
    // path, since the fd already resolved the link at open time.
    def "lstat" (recv, &_blk) {
        crate::builtins::io::with_file(recv, |_f, path| {
            crate::builtins::stat::stat_from_path(path, false)
        })
    }

    // `#chown(uid, gid)` -- `fchown(2)`; a nil arg leaves that id unchanged
    // (`-1` to the syscall). Answers 0.
    def "chown" (recv, owner, group, &_blk) {
        use std::os::fd::AsRawFd;
        let id = |v: &RubyValue| -> libc::uid_t {
            match v {
                RubyValue::Int(i) => *i as libc::uid_t,
                _ => u32::MAX, // -1: leave unchanged
            }
        };
        let (uid, gid) = (id(owner), id(group));
        crate::builtins::io::with_file(recv, |f, path| {
            // SAFETY: `f` owns a valid fd for the call's duration.
            if unsafe { libc::fchown(f.as_raw_fd(), uid, gid) } != 0 {
                return Err(raise_errno(&std::io::Error::last_os_error(), "apply2files", path));
            }
            Ok(RubyValue::Int(0))
        })
    }

    // `#chmod(mode)` -- set the open file's permission bits; answers 0.
    def "chmod" (recv, mode_arg, &_blk) {
        use std::os::fd::AsRawFd;
        let mode = crate::builtins::io::int_of(mode_arg)?;
        crate::builtins::io::with_file(recv, |f, path| {
            // SAFETY: `f` owns a valid fd for the call's duration.
            if unsafe { libc::fchmod(f.as_raw_fd(), mode as libc::mode_t) } != 0 {
                return Err(raise_errno(&std::io::Error::last_os_error(), "apply2files", path));
            }
            Ok(RubyValue::Int(0))
        })
    }

    // `#truncate(len)` -- resize the open file to `len` bytes; answers 0.
    def "truncate" (recv, length, &_blk) {
        crate::builtins::io::check_writable(recv)?;
        let len = crate::builtins::io::offset_of(length)?;
        crate::builtins::io::with_file(recv, |f, path| {
            f.set_len(len.max(0) as u64)
                .map_err(|e| raise_errno(&e, "rb_file_s_truncate", path))?;
            Ok(RubyValue::Int(0))
        })
    }

    // `#mtime`/`#atime`/`#ctime`/`#birthtime`/`#size` -- read off the same
    // `fstat` snapshot `#stat` answers, so each costs one syscall and the
    // family agrees with itself.
    def "mtime" (recv, &_blk) {
        stat_field(recv, "mtime")
    }
    def "atime" (recv, &_blk) {
        stat_field(recv, "atime")
    }
    def "ctime" (recv, &_blk) {
        stat_field(recv, "ctime")
    }
    def "birthtime" (recv, &_blk) {
        stat_field(recv, "birthtime")
    }
    def "size" (recv, &_blk) {
        stat_field(recv, "size")
    }
}

/// One field of the open file's `fstat` snapshot, by its `File::Stat` name.
fn stat_field(recv: &RubyValue, name: &str) -> Result<RubyValue, Signal> {
    let st = crate::builtins::io::stat_value(recv)?;
    crate::dispatch::send_value(&st, crate::Symbol::intern(name), &[], None)
}

/// `access(2)` -- the real permission question, rather than inferring from
/// metadata (which would ignore ACLs and the effective uid).
/// The `path [, dir]` argument pair `realpath`/`realdirpath` share, joined into
/// one path. An absolute `path` ignores `dir`.
fn realpath_join(path: &RubyValue, dir: Option<&RubyValue>, who: &str) -> Result<String, Signal> {
    let raw = path_arg(path, who)?;
    Ok(match dir {
        Some(d) if !d.is_nil() => {
            let base = path_arg(d, who)?;
            if raw.starts_with('/') {
                raw
            } else {
                format!("{base}/{raw}")
            }
        }
        _ => raw,
    })
}

/// `access(2)` against the REAL uid/gid -- what the `*_real?` predicates ask.
fn access(path: &str, mode: libc::c_int) -> bool {
    let Ok(c) = std::ffi::CString::new(path) else {
        // An interior NUL can't name a file; CRuby raises ArgumentError, but
        // for a predicate `false` is the honest answer.
        return false;
    };
    // SAFETY: `c` is a valid NUL-terminated string for the call's duration.
    unsafe { libc::access(c.as_ptr(), mode) == 0 }
}

/// `faccessat(2)` with `AT_EACCESS` -- the EFFECTIVE uid/gid, which is what
/// `readable?`/`writable?`/`executable?` ask and `access(2)` alone cannot
/// answer. The two agree for any process that has not changed credentials.
fn access_eff(path: &str, mode: libc::c_int) -> bool {
    let Ok(c) = std::ffi::CString::new(path) else {
        return false;
    };
    // SAFETY: `c` is a valid NUL-terminated string for the call's duration.
    unsafe { libc::faccessat(libc::AT_FDCWD, c.as_ptr(), mode, libc::AT_EACCESS) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::str_value;

    /// The File class methods are `ruby_class!`-generated (mangled Rust fn
    /// names), so the tests reach them the way dispatch does -- through the
    /// registered class table. (`lookup_class` itself is still emitted by the
    /// macro at module scope, so those calls need no wrapper.)
    fn cmethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::FILE_CLASS)
            .expect("File is a registered builtin table")
            .class
            .as_ref()
            .expect("File has class methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("File.{name} is defined"))
    }
    macro_rules! cwrap {
        ($rust:ident => $ruby:literal) => {
            fn $rust(
                r: &RubyValue,
                a: &[RubyValue],
                b: Option<RubyValue>,
            ) -> Result<RubyValue, Signal> {
                cmethod($ruby)(r, a, b)
            }
        };
    }
    cwrap!(file_read => "read");
    cwrap!(file_readlines => "readlines");
    cwrap!(file_file_p => "file?");
    cwrap!(file_directory_p => "directory?");
    cwrap!(file_size => "size");
    cwrap!(file_size_p => "size?");
    cwrap!(file_zero_p => "zero?");
    cwrap!(file_rename => "rename");
    cwrap!(file_delete => "delete");
    cwrap!(file_write => "write");
    cwrap!(file_exist_p => "exist?");
    cwrap!(file_join => "join");
    cwrap!(file_split => "split");

    // --- The pure-path family: no disk, every case oracle-read ------------

    #[test]
    fn basename_strips_directories_and_an_optional_suffix() {
        assert_eq!(basename_of("/a/b/c.rb", None), "c.rb");
        assert_eq!(basename_of("/a/b/c.rb", Some(".rb")), "c");
        assert_eq!(basename_of("/a/b/c.rb", Some(".*")), "c");
        assert_eq!(basename_of("c.rb", None), "c.rb");
        // A trailing slash is stripped first.
        assert_eq!(basename_of("/a/b/", None), "b");
        assert_eq!(basename_of("/", None), "/");
        // A non-matching suffix is left alone.
        assert_eq!(basename_of("/a/c.rb", Some(".py")), "c.rb");
        // A leading dot is not an extension.
        assert_eq!(basename_of("/a/.bashrc", Some(".*")), ".bashrc");
    }

    #[test]
    fn dirname_answers_dot_when_there_is_no_directory() {
        assert_eq!(dirname_of("/a/b/c.rb"), "/a/b");
        assert_eq!(dirname_of("a"), ".");
        assert_eq!(dirname_of("/x"), "/");
        assert_eq!(dirname_of("/"), "/");
        assert_eq!(dirname_of("a/b"), "a");
    }

    /// Every row oracle-read from ruby 4.0.6 -- including the two that read
    /// like off-by-ones: a TRAILING dot IS an extension (`"foo."` -> `"."`),
    /// while a LEADING one is not (`".bashrc"` -> `""`).
    #[test]
    fn extname_matches_the_oracle() {
        for (path, want) in [
            ("/a/b/c.rb", ".rb"),
            ("x", ""),
            (".bashrc", ""),
            ("foo.", "."),
            ("a.b.c", ".c"),
            ("a..b", ".b"),
            ("foo.rb.", "."),
            (".x.y", ".y"),
            ("..", ""),
            (".", ""),
        ] {
            assert_eq!(extname_of(path), want, "File.extname({path:?})");
        }
    }

    fn cls() -> RubyValue {
        RubyValue::Class(zeo_abi::FILE_CLASS)
    }

    #[test]
    fn join_collapses_separators_at_the_seam() {
        let j = |parts: &[&str]| {
            let args: Vec<RubyValue> = parts.iter().map(|p| str_value(p)).collect();
            file_join(&cls(), &args, None).unwrap().to_display_string()
        };
        assert_eq!(j(&["a", "b", "c"]), "a/b/c");
        assert_eq!(j(&["a/", "b"]), "a/b");
        assert_eq!(j(&["a", "/b"]), "a/b");
        assert_eq!(j(&["a/", "/b"]), "a/b");
        assert_eq!(j(&["/a", "b"]), "/a/b");
        assert_eq!(j(&["a"]), "a");
    }

    /// `File.join` flattens a nested Array argument.
    #[test]
    fn join_flattens_an_array_argument() {
        let arr = RubyValue::Array(crate::collections::array_new(vec![
            str_value("b"),
            str_value("c"),
        ]));
        let got = file_join(&cls(), &[str_value("a"), arr], None).unwrap();
        assert_eq!(got.to_display_string(), "a/b/c");
    }

    #[test]
    fn split_is_dirname_and_basename() {
        let RubyValue::Array(parts) = file_split(&cls(), &[str_value("/a/b/c.rb")], None).unwrap()
        else {
            panic!("expected an Array")
        };
        let got: Vec<String> = parts.lock().iter().map(|p| p.to_display_string()).collect();
        assert_eq!(got, vec!["/a/b".to_string(), "c.rb".to_string()]);
    }

    #[test]
    fn expand_path_absolutizes_and_resolves_dots_lexically() {
        assert_eq!(expand_path_of("b", Some("/a")).unwrap(), "/a/b");
        assert_eq!(expand_path_of("/a/b", None).unwrap(), "/a/b");
        assert_eq!(expand_path_of("../c", Some("/a/b")).unwrap(), "/a/c");
        assert_eq!(expand_path_of("./c", Some("/a")).unwrap(), "/a/c");
        assert_eq!(expand_path_of("/a/./b/../c", None).unwrap(), "/a/c");
    }

    /// `~` expands to HOME.
    #[test]
    fn expand_path_resolves_a_leading_tilde() {
        let home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() {
            return; // no HOME in this environment; nothing to assert
        }
        assert_eq!(expand_path_of("~", None).unwrap(), home);
        assert_eq!(expand_path_of("~/x", None).unwrap(), format!("{home}/x"));
    }

    // --- The disk-touching surface ---------------------------------------

    /// A temp path unique to this test binary + name, so parallel tests
    /// never collide.
    fn tmp(name: &str) -> String {
        format!(
            "{}/zeo_file_test_{}_{}",
            std::env::temp_dir().to_string_lossy(),
            std::process::id(),
            name
        )
    }

    #[test]
    fn write_then_read_round_trips() {
        let p = tmp("roundtrip");
        let n = file_write(&cls(), &[str_value(&p), str_value("hello\n")], None).unwrap();
        assert!(matches!(n, RubyValue::Int(6)));
        let got = file_read(&cls(), &[str_value(&p)], None).unwrap();
        assert_eq!(got.to_display_string(), "hello\n");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn predicates_answer_false_rather_than_raising_for_a_missing_path() {
        let p = tmp("absent");
        for f in [file_exist_p, file_file_p, file_directory_p, file_zero_p] {
            assert!(
                matches!(
                    f(&cls(), &[str_value(&p)], None).unwrap(),
                    RubyValue::Bool(false)
                ),
                "a predicate raised or answered true for a missing path"
            );
        }
        // `size?` is nil (not 0, not an error) for a missing path.
        assert!(matches!(
            file_size_p(&cls(), &[str_value(&p)], None).unwrap(),
            RubyValue::Nil
        ));
    }

    #[test]
    fn predicates_distinguish_files_from_directories() {
        let p = tmp("predicates");
        std::fs::write(&p, "x").unwrap();
        assert!(matches!(
            file_exist_p(&cls(), &[str_value(&p)], None).unwrap(),
            RubyValue::Bool(true)
        ));
        assert!(matches!(
            file_file_p(&cls(), &[str_value(&p)], None).unwrap(),
            RubyValue::Bool(true)
        ));
        assert!(matches!(
            file_directory_p(&cls(), &[str_value(&p)], None).unwrap(),
            RubyValue::Bool(false)
        ));
        let dir = std::env::temp_dir().to_string_lossy().into_owned();
        assert!(matches!(
            file_directory_p(&cls(), &[str_value(&dir)], None).unwrap(),
            RubyValue::Bool(true)
        ));
        assert!(matches!(
            file_file_p(&cls(), &[str_value(&dir)], None).unwrap(),
            RubyValue::Bool(false)
        ));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn size_reads_the_byte_count_and_zero_p_detects_empty() {
        let p = tmp("size");
        std::fs::write(&p, "12345").unwrap();
        assert!(matches!(
            file_size(&cls(), &[str_value(&p)], None).unwrap(),
            RubyValue::Int(5)
        ));
        assert!(matches!(
            file_size_p(&cls(), &[str_value(&p)], None).unwrap(),
            RubyValue::Int(5)
        ));
        assert!(matches!(
            file_zero_p(&cls(), &[str_value(&p)], None).unwrap(),
            RubyValue::Bool(false)
        ));

        std::fs::write(&p, "").unwrap();
        assert!(matches!(
            file_zero_p(&cls(), &[str_value(&p)], None).unwrap(),
            RubyValue::Bool(true)
        ));
        // An EMPTY file's `size?` is nil, though its `size` is 0.
        assert!(matches!(
            file_size_p(&cls(), &[str_value(&p)], None).unwrap(),
            RubyValue::Nil
        ));
        assert!(matches!(
            file_size(&cls(), &[str_value(&p)], None).unwrap(),
            RubyValue::Int(0)
        ));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn readlines_keeps_the_newlines() {
        let p = tmp("readlines");
        std::fs::write(&p, "a\nb\nc").unwrap();
        let RubyValue::Array(lines) = file_readlines(&cls(), &[str_value(&p)], None).unwrap()
        else {
            panic!("expected an Array")
        };
        let got: Vec<String> = lines.lock().iter().map(|l| l.to_display_string()).collect();
        assert_eq!(
            got,
            vec!["a\n".to_string(), "b\n".to_string(), "c".to_string()]
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn delete_and_rename() {
        let a = tmp("rename_from");
        let b = tmp("rename_to");
        std::fs::write(&a, "x").unwrap();
        file_rename(&cls(), &[str_value(&a), str_value(&b)], None).unwrap();
        assert!(!std::path::Path::new(&a).exists());
        assert!(std::path::Path::new(&b).exists());
        let n = file_delete(&cls(), &[str_value(&b)], None).unwrap();
        assert!(matches!(n, RubyValue::Int(1)));
        assert!(!std::path::Path::new(&b).exists());
    }

    /// A missing file raises Errno::ENOENT, not a generic error --
    /// registry-less, `raise_error` surfaces as a panic.
    #[test]
    fn reading_a_missing_file_raises_errno_enoent() {
        let r = std::panic::catch_unwind(|| {
            file_read(&cls(), &[str_value("/nope/definitely/not")], None)
        });
        assert!(r.is_err());
    }

    /// The errno mapping itself, without needing the exception registry:
    /// registry-less, `raise_error` panics with the class + message it would
    /// have raised, so the panic payload IS the assertion. (The `io::Error`
    /// is built inside the closure -- it isn't `RefUnwindSafe`.)
    #[test]
    fn raise_errno_maps_kinds_to_ruby_classes() {
        let msg = std::panic::catch_unwind(|| {
            let e = std::io::Error::from(std::io::ErrorKind::NotFound);
            raise_errno(&e, "rb_sysopen", "/nope")
        })
        .err()
        .map(|p| {
            p.downcast_ref::<String>()
                .cloned()
                .unwrap_or_else(|| "<non-string panic>".to_string())
        })
        .unwrap_or_default();
        assert!(
            msg.contains("Errno::ENOENT")
                && msg.contains("No such file or directory @ rb_sysopen - /nope"),
            "unexpected raise: {msg}"
        );
    }

    #[test]
    fn a_non_string_path_is_a_type_error() {
        let r = std::panic::catch_unwind(|| file_read(&cls(), &[RubyValue::Int(5)], None));
        assert!(r.is_err());
    }

    #[test]
    fn lookup_finds_the_file_names() {
        assert!(lookup_class("read").is_some());
        assert!(lookup_class("basename").is_some());
        assert!(lookup_class("join").is_some());
        assert!(lookup_class("nope").is_none());
    }
}

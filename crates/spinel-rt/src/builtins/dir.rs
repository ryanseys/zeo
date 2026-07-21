//! `Dir` (CRuby dir.c) -- directory listing, the working directory, and
//! `glob`.
//!
//! The glob matcher is hand-ported rather than delegating to the `glob`
//! crate: Ruby's own semantics differ in ways that matter (`**` spans
//! directories only as a whole path SEGMENT, `{a,b}` alternation is Ruby's
//! not the shell's, and a leading `.` is hidden from `*` unless matched
//! literally). Wrapping a crate would mean fighting its opinions at every
//! one of those points; the rules themselves are short.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use crate::builtins::file::{path_arg, raise_errno};
use crate::builtins::{arity, block_or_enum, builtin_methods};
use crate::dispatch::{raise_error, RObj, RubyObject};
use crate::{RubyValue, Signal};
use spinel_abi::{ClassId, DIR_CLASS};

/// A per-process monotonic counter making each `Dir.mktmpdir` name unique
/// even when the PRNG and pid coincide within one run.
static MKTMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn str_val(s: String) -> RubyValue {
    RubyValue::Str(crate::collections::string_new(s))
}

/// Match one path SEGMENT against one glob segment: `*` (any run, never
/// crossing `/`), `?` (one char), `[abc]`/`[a-z]`/`[!abc]` classes, and
/// `{a,b}` alternation. Recursive descent over both, backtracking on `*`.
fn match_segment(pat: &[char], name: &[char]) -> bool {
    // Both exhausted: matched.
    if pat.is_empty() {
        return name.is_empty();
    }
    match pat[0] {
        '*' => {
            // `*` matches any run, including empty -- try every split.
            for i in 0..=name.len() {
                if match_segment(&pat[1..], &name[i..]) {
                    return true;
                }
            }
            false
        }
        '?' => !name.is_empty() && match_segment(&pat[1..], &name[1..]),
        '[' => {
            if name.is_empty() {
                return false;
            }
            // Find the closing bracket; an unclosed `[` is a literal.
            let Some(close) = pat.iter().position(|&c| c == ']') else {
                return pat[0] == name[0] && match_segment(&pat[1..], &name[1..]);
            };
            let mut set = &pat[1..close];
            let negated = !set.is_empty() && (set[0] == '!' || set[0] == '^');
            if negated {
                set = &set[1..];
            }
            let mut hit = false;
            let mut i = 0;
            while i < set.len() {
                // A range (`a-z`), or a single char.
                if i + 2 < set.len() && set[i + 1] == '-' {
                    if set[i] <= name[0] && name[0] <= set[i + 2] {
                        hit = true;
                    }
                    i += 3;
                } else {
                    if set[i] == name[0] {
                        hit = true;
                    }
                    i += 1;
                }
            }
            hit != negated && match_segment(&pat[close + 1..], &name[1..])
        }
        '{' => {
            // `{a,b,c}` -- try each alternative spliced in place of the group.
            let Some(close) = find_close_brace(pat) else {
                return pat[0] == name[0] && match_segment(&pat[1..], &name[1..]);
            };
            for alt in split_alternatives(&pat[1..close]) {
                let mut spliced: Vec<char> = alt;
                spliced.extend_from_slice(&pat[close + 1..]);
                if match_segment(&spliced, name) {
                    return true;
                }
            }
            false
        }
        '\\' if pat.len() > 1 => {
            // An escaped metacharacter matches literally.
            !name.is_empty() && pat[1] == name[0] && match_segment(&pat[2..], &name[1..])
        }
        c => !name.is_empty() && c == name[0] && match_segment(&pat[1..], &name[1..]),
    }
}

fn find_close_brace(pat: &[char]) -> Option<usize> {
    let mut depth = 0;
    for (i, &c) in pat.iter().enumerate() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Split `a,b,{c,d}` on TOP-LEVEL commas only -- a nested group's commas
/// belong to it.
fn split_alternatives(body: &[char]) -> Vec<Vec<char>> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    let mut depth = 0;
    for &c in body {
        match c {
            '{' => {
                depth += 1;
                cur.push(c);
            }
            '}' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// Whether a glob segment matches a directory entry, applying Ruby's
/// hidden-file rule: a name starting with `.` is invisible to a wildcard,
/// and only matches a pattern that starts with a literal `.`.
fn seg_matches(pat: &str, name: &str, dotmatch: bool) -> bool {
    // A leading `.` is only matched by a pattern that also starts with `.`,
    // UNLESS `File::FNM_DOTMATCH` was given (then a dotfile matches `*` too).
    if !dotmatch && name.starts_with('.') && !pat.starts_with('.') {
        return false;
    }
    let p: Vec<char> = pat.chars().collect();
    let n: Vec<char> = name.chars().collect();
    match_segment(&p, &n)
}

/// Walk `dir` against the remaining glob segments, pushing every match onto
/// `out`. `prefix` is the path built so far (as the caller wants it echoed
/// back -- glob answers paths relative to the same root the pattern was).
fn glob_walk(base: &str, prefix: &str, segs: &[&str], out: &mut Vec<String>, dotmatch: bool) {
    let Some((seg, rest)) = segs.split_first() else {
        return;
    };
    // For an absolute pattern `base` is empty and the root to read is `/`
    // (an empty `read_dir("")` reads nothing -- the bug that made absolute
    // globs return nothing). A relative pattern's base is ".".
    let dir_path = if prefix.is_empty() {
        if base.is_empty() { "/".to_string() } else { base.to_string() }
    } else {
        format!("{base}/{prefix}")
    };

    // `**` spans zero or more whole directory segments.
    if *seg == "**" {
        // Zero segments: try the rest right here.
        if rest.is_empty() {
            // A trailing `**` matches directories themselves.
            if !prefix.is_empty() {
                out.push(prefix.to_string());
            }
        } else {
            glob_walk(base, prefix, rest, out, dotmatch);
        }
        // One or more: descend into every visible subdirectory and retry the
        // whole `**` there.
        let Ok(entries) = std::fs::read_dir(&dir_path) else {
            return;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue; // `**` does not descend into hidden dirs
            }
            if e.path().is_dir() {
                let next = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
                glob_walk(base, &next, segs, out, dotmatch);
            }
        }
        return;
    }

    let Ok(entries) = std::fs::read_dir(&dir_path) else {
        return;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    // `*` with FNM_DOTMATCH also yields `.` (the directory itself) -- never
    // `..`. `read_dir` omits both, so inject `.` explicitly; it can only be a
    // terminal match (descending into it would re-read the same directory).
    if dotmatch {
        names.push(".".to_string());
    }
    names.sort();
    for name in names {
        if !seg_matches(seg, &name, dotmatch) {
            continue;
        }
        if name == "." && !rest.is_empty() {
            continue;
        }
        let next = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        if rest.is_empty() {
            out.push(next);
        } else {
            let child = if prefix.is_empty() {
                format!("{base}/{name}")
            } else {
                format!("{base}/{prefix}/{name}")
            };
            if std::path::Path::new(&child).is_dir() {
                glob_walk(base, &next, rest, out, dotmatch);
            }
        }
    }
}

/// One glob pattern -> matching paths, relative to the cwd (or absolute, if
/// the pattern is). Ruby sorts glob results.
fn glob(pattern: &str, dotmatch: bool) -> Vec<String> {
    let absolute = pattern.starts_with('/');
    let (base, pat) = if absolute { ("", pattern.trim_start_matches('/')) } else { (".", pattern) };
    let segs: Vec<&str> = pat.split('/').filter(|s| !s.is_empty()).collect();
    let mut out = Vec::new();
    glob_walk(if absolute { "" } else { base }, "", &segs, &mut out, dotmatch);
    if absolute {
        out = out.into_iter().map(|p| format!("/{p}")).collect();
    }
    out.sort();
    out.dedup();
    out
}

/// An open `Dir` handle (`Dir.new`/`Dir.open`). Its entries are materialized
/// up front into a `Vec` (a `std::fs::ReadDir` is neither `Send` nor `Sync`,
/// so it can't live behind the `Arc` an `RObj` needs), with an atomic cursor
/// `#read`/`#pos`/`#seek`/`#rewind` move over -- CRuby's own snapshot-at-open
/// semantics.
pub struct RDir {
    path: String,
    /// Every entry, INCLUDING `.` and `..` (what `#read`/`#each` iterate).
    entries: Vec<String>,
    /// The read cursor into `entries`.
    pos: AtomicUsize,
    /// `false` after `#close`; every later operation raises IOError.
    open: AtomicBool,
}

impl RubyObject for RDir {
    fn class_id(&self) -> ClassId {
        DIR_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        false
    }
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(RDir {
            path: self.path.clone(),
            entries: self.entries.clone(),
            pos: AtomicUsize::new(self.pos.load(Ordering::Relaxed)),
            open: AtomicBool::new(self.open.load(Ordering::Relaxed)),
        })
    }
}

/// Open `path` as a `Dir` handle value -- the shared body of `Dir.new`/`.open`.
fn open_dir(path: &str) -> Result<RubyValue, Signal> {
    // `read_names` raises ENOENT for a missing path (the `dir_initialize`
    // syscall name CRuby reports).
    let mut entries = read_names(path)?;
    entries.push(".".to_string());
    entries.push("..".to_string());
    Ok(RubyValue::Object(Arc::new(RDir {
        path: path.to_string(),
        entries,
        pos: AtomicUsize::new(0),
        open: AtomicBool::new(true),
    })))
}

fn recv_dir(recv: &RubyValue) -> Result<&RDir, Signal> {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RDir>()
            .ok_or_else(|| raise_error("TypeError", "not a Dir".to_string())),
        _ => Err(raise_error("TypeError", "not a Dir".to_string())),
    }
}

/// Raise IOError on a closed handle, else hand back the live `RDir`.
fn live_dir(recv: &RubyValue) -> Result<&RDir, Signal> {
    let d = recv_dir(recv)?;
    if !d.open.load(Ordering::Relaxed) {
        return Err(raise_error("IOError", "closed directory".to_string()));
    }
    Ok(d)
}

builtin_methods! {
    pub(crate) fn lookup;

    // `#read` -- the next entry name (INCLUDING `.`/`..`), or nil at the end.
    "read" => fn dir_h_read(recv, args, _block) {
        arity!(args, 0);
        let d = live_dir(recv)?;
        let i = d.pos.fetch_add(1, Ordering::Relaxed);
        Ok(match d.entries.get(i) {
            Some(name) => str_val(name.clone()),
            None => {
                // Don't advance past the end.
                d.pos.store(d.entries.len(), Ordering::Relaxed);
                RubyValue::Nil
            }
        })
    }
    // `#each` -- yield every entry from the current cursor onward (INCLUDING
    // `.`/`..`); without a block, an Enumerator over the entry array.
    "each" => fn dir_h_each(recv, args, block) {
        arity!(args, 0);
        let entries: Vec<RubyValue> = live_dir(recv)?.entries.iter().cloned().map(str_val).collect();
        let p = block_or_enum!(recv, "each", args, block);
        for e in entries {
            p.call(&[e])?;
        }
        Ok(recv.clone())
    }
    // `#each_child` -- like `#each` but WITHOUT `.` and `..`.
    "each_child" => fn dir_h_each_child(recv, args, block) {
        arity!(args, 0);
        let entries: Vec<RubyValue> = live_dir(recv)?
            .entries.iter().filter(|n| *n != "." && *n != "..").cloned().map(str_val).collect();
        let p = block_or_enum!(recv, "each_child", args, block);
        for e in entries {
            p.call(&[e])?;
        }
        Ok(recv.clone())
    }
    // `#children` / `#entries` -- the entry names as an Array (children drops
    // `.`/`..`); both snapshot the whole listing regardless of the cursor.
    "children" => fn dir_h_children(recv, args, _block) {
        arity!(args, 0);
        let out: Vec<RubyValue> = live_dir(recv)?
            .entries.iter().filter(|n| *n != "." && *n != "..").cloned().map(str_val).collect();
        Ok(RubyValue::Array(crate::collections::array_new(out)))
    }
    "entries" => fn dir_h_entries(recv, args, _block) {
        arity!(args, 0);
        let out: Vec<RubyValue> = live_dir(recv)?.entries.iter().cloned().map(str_val).collect();
        Ok(RubyValue::Array(crate::collections::array_new(out)))
    }
    "path" | "to_path" => fn dir_h_path(recv, args, _block) {
        arity!(args, 0);
        Ok(str_val(recv_dir(recv)?.path.clone()))
    }
    // `#pos`/`#tell` read the cursor; `#pos=`/`#seek` set it; `#rewind` zeroes it.
    "pos" | "tell" => fn dir_h_pos(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(live_dir(recv)?.pos.load(Ordering::Relaxed) as i64))
    }
    // `pos=` answers the new position; `seek` answers the Dir itself (so it
    // chains), the one behavioural difference between the two.
    "pos=" => fn dir_h_pos_set(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Int(n) = &args[0] else {
            return Err(raise_error("TypeError", "no implicit conversion into Integer".to_string()));
        };
        live_dir(recv)?.pos.store((*n).max(0) as usize, Ordering::Relaxed);
        Ok(args[0].clone())
    }
    "seek" => fn dir_h_seek(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Int(n) = &args[0] else {
            return Err(raise_error("TypeError", "no implicit conversion into Integer".to_string()));
        };
        live_dir(recv)?.pos.store((*n).max(0) as usize, Ordering::Relaxed);
        Ok(recv.clone())
    }
    "rewind" => fn dir_h_rewind(recv, args, _block) {
        arity!(args, 0);
        live_dir(recv)?.pos.store(0, Ordering::Relaxed);
        Ok(recv.clone())
    }
    "close" => fn dir_h_close(recv, args, _block) {
        arity!(args, 0);
        recv_dir(recv)?.open.store(false, Ordering::Relaxed);
        Ok(RubyValue::Nil)
    }
    "fileno" => fn dir_h_fileno(recv, args, _block) {
        arity!(args, 0);
        // We hold a materialized snapshot, not a live fd; -1 is the honest
        // answer (and nothing in the corpus reads it).
        let _ = recv_dir(recv)?;
        Ok(RubyValue::Int(-1))
    }
    "inspect" | "to_s" => fn dir_h_inspect(recv, args, _block) {
        arity!(args, 0);
        Ok(str_val(format!("#<Dir:{}>", recv_dir(recv)?.path)))
    }
}

builtin_methods! {
    pub(crate) fn lookup_class;

    // `Dir.new(path)` / `Dir.open(path)` -- a handle over the directory's
    // entries. The block form of `open` yields the handle and closes it after.
    "new" | "open" => fn dir_open(_recv, args, block) {
        arity!(args, 1..=2);
        let path = path_arg(&args[0], "open")?;
        let dir = open_dir(&path)?;
        let Some(RubyValue::Proc(p)) = block else {
            return Ok(dir);
        };
        let out = p.call(&[dir.clone()]);
        let _ = dir_h_close(&dir, &[], None);
        out
    }
    "pwd" | "getwd" => fn dir_pwd(_recv, args, _block) {
        arity!(args, 0);
        let d = std::env::current_dir().map_err(|e| raise_errno(&e, "getcwd", "."))?;
        Ok(str_val(d.to_string_lossy().into_owned()))
    }
    "chdir" => fn dir_chdir(_recv, args, block) {
        arity!(args, 0..=1);
        let target = match args.first() {
            Some(v) => path_arg(v, "chdir")?,
            None => std::env::var("HOME").unwrap_or_else(|_| "/".to_string()),
        };
        // The block form restores the previous directory afterwards, even if
        // the block raises -- CRuby's own contract.
        if let Some(RubyValue::Proc(p)) = block {
            let prev = std::env::current_dir().map_err(|e| raise_errno(&e, "getcwd", "."))?;
            std::env::set_current_dir(&target).map_err(|e| raise_errno(&e, "chdir", &target))?;
            let r = p.call(&[str_val(target)]);
            let _ = std::env::set_current_dir(prev);
            return r;
        }
        std::env::set_current_dir(&target).map_err(|e| raise_errno(&e, "chdir", &target))?;
        Ok(RubyValue::Int(0))
    }
    // `entries` INCLUDES `.` and `..`; `children` excludes them.
    "entries" => fn dir_entries(_recv, args, _block) {
        arity!(args, 1..=2);
        let path = path_arg(&args[0], "entries")?;
        let mut names = read_names(&path)?;
        names.push(".".to_string());
        names.push("..".to_string());
        names.sort();
        Ok(RubyValue::Array(crate::collections::array_new(
            names.into_iter().map(str_val).collect(),
        )))
    }
    "children" => fn dir_children(_recv, args, _block) {
        arity!(args, 1..=2);
        let path = path_arg(&args[0], "children")?;
        let mut names = read_names(&path)?;
        names.sort();
        Ok(RubyValue::Array(crate::collections::array_new(
            names.into_iter().map(str_val).collect(),
        )))
    }
    // `Dir.foreach(path)` -- yield each entry name (INCLUDING `.` and `..`,
    // like `entries`); without a block, an Enumerator.
    "foreach" => fn dir_foreach(recv, args, block) {
        arity!(args, 1..=2);
        let path = path_arg(&args[0], "foreach")?;
        let p = block_or_enum!(recv, "foreach", args, block);
        let mut names = read_names(&path)?;
        names.push(".".to_string());
        names.push("..".to_string());
        names.sort();
        for n in names {
            p.call(&[str_val(n)])?;
        }
        Ok(RubyValue::Nil)
    }
    "each_child" => fn dir_each_child(_recv, args, block) {
        arity!(args, 1);
        let path = path_arg(&args[0], "each_child")?;
        let Some(RubyValue::Proc(p)) = block else {
            return Err(crate::dispatch::raise_no_block_yield());
        };
        let mut names = read_names(&path)?;
        names.sort();
        for n in names {
            p.call(&[str_val(n)])?;
        }
        Ok(RubyValue::Nil)
    }
    "exist?" => fn dir_exist_p(_recv, args, _block) {
        arity!(args, 1);
        let path = path_arg(&args[0], "exist?")?;
        Ok(RubyValue::Bool(std::path::Path::new(&path).is_dir()))
    }
    "empty?" => fn dir_empty_p(_recv, args, _block) {
        arity!(args, 1);
        let path = path_arg(&args[0], "empty?")?;
        // A missing path raises (ENOENT); an existing NON-directory is simply
        // not an empty directory -> false (CRuby doesn't raise there).
        let md = std::fs::metadata(&path)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "empty?", &path))?;
        if !md.is_dir() {
            return Ok(RubyValue::Bool(false));
        }
        Ok(RubyValue::Bool(read_names(&path)?.is_empty()))
    }
    "mkdir" => fn dir_mkdir(_recv, args, _block) {
        arity!(args, 1..=2);
        let path = path_arg(&args[0], "mkdir")?;
        std::fs::create_dir(&path).map_err(|e| raise_errno(&e, "mkdir", &path))?;
        Ok(RubyValue::Int(0))
    }
    // `mktmpdir([prefix | [prefix, suffix]], [parent])`: create a fresh
    // temporary directory. With a block, yield its path and remove the
    // directory (recursively) afterward -- even if the block raises --
    // answering the block's value; without a block, answer the path for the
    // caller to clean up.
    "mktmpdir" => fn dir_mktmpdir(_recv, args, block) {
        arity!(args, 0..=2);
        let (prefix, suffix) = match args.first() {
            None | Some(RubyValue::Nil) => ("d".to_string(), String::new()),
            Some(RubyValue::Str(s)) => (s.lock().to_utf8_lossy().into_owned(), String::new()),
            Some(RubyValue::Array(a)) => {
                let a = a.lock();
                (
                    a.first().map(|v| v.to_display_string()).unwrap_or_default(),
                    a.get(1).map(|v| v.to_display_string()).unwrap_or_default(),
                )
            }
            Some(other) => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!("no implicit conversion of {} into String", crate::builtins::convert_name_of(other)),
                ))
            }
        };
        let parent = match args.get(1) {
            Some(RubyValue::Str(s)) => std::path::PathBuf::from(s.lock().to_utf8_lossy().into_owned()),
            _ => std::env::temp_dir(),
        };
        let pid = std::process::id();
        let path = loop {
            let rand = crate::builtins::kernel::prng_next();
            let n = MKTMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let candidate = parent.join(format!("{prefix}{pid}-{n}-{rand:x}{suffix}"));
            match std::fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(raise_errno(&e, "mkdir", &candidate.to_string_lossy())),
            }
        };
        let path_str = path.to_string_lossy().into_owned();
        match &block {
            Some(RubyValue::Proc(p)) => {
                let result = p.call(&[str_val(path_str)]);
                // Cleanup runs on both the normal and the raising path (ensure).
                let _ = std::fs::remove_dir_all(&path);
                result
            }
            _ => Ok(str_val(path_str)),
        }
    }
    "rmdir" | "unlink" | "delete" => fn dir_rmdir(_recv, args, _block) {
        arity!(args, 1);
        let path = path_arg(&args[0], "rmdir")?;
        std::fs::remove_dir(&path).map_err(|e| raise_errno(&e, "rmdir", &path))?;
        Ok(RubyValue::Int(0))
    }
    "home" => fn dir_home(_recv, args, _block) {
        arity!(args, 0..=1);
        Ok(str_val(std::env::var("HOME").unwrap_or_else(|_| "/".to_string())))
    }
    // NOTE: no `tmpdir` row. `Dir.tmpdir` LOOKS like core Dir but is
    // stdlib's tmpdir.rb -- `Dir.tmpdir` without `require "tmpdir"` is a
    // NoMethodError in real Ruby (oracle-verified, the hard way: an example
    // using it failed under the oracle, not under us). Defining it here
    // would make us accept a program CRuby rejects, which is worse than the
    // missing convenience. It arrives with the stdlib work, as Ruby's own.
    // `Dir.glob(pat)` / `Dir[pat]` -- the block form yields each match and
    // answers nil; otherwise an Array. Multiple patterns union.
    "glob" | "[]" => fn dir_glob(_recv, args, block) {
        if args.is_empty() {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                "wrong number of arguments (given 0, expected 1+)".to_string(),
            ));
        }
        // A trailing Integer FNM flags argument (`File::FNM_DOTMATCH`, ...)
        // governs matching for all patterns. FNM_DOTMATCH is bit 0x4.
        let dotmatch = args
            .iter()
            .filter_map(|a| if let RubyValue::Int(f) = a { Some(*f) } else { None })
            .any(|f| f & 0x4 != 0);
        let mut all = Vec::new();
        for a in args {
            match a {
                RubyValue::Array(pats) => {
                    for p in pats.lock().iter() {
                        all.extend(glob(&path_arg(p, "glob")?, dotmatch));
                    }
                }
                // A trailing options Hash (`base:`) or the Integer FNM flags
                // argument itself is not a pattern.
                // TODO(plan P-B): honor `base:`.
                RubyValue::Hash(_) | RubyValue::Int(_) => {}
                v => all.extend(glob(&path_arg(v, "glob")?, dotmatch)),
            }
        }
        all.sort();
        all.dedup();
        if let Some(RubyValue::Proc(p)) = block {
            for m in all {
                p.call(&[str_val(m)])?;
            }
            return Ok(RubyValue::Nil);
        }
        Ok(RubyValue::Array(crate::collections::array_new(
            all.into_iter().map(str_val).collect(),
        )))
    }
}

/// The entry names in `path`, excluding `.`/`..` -- the shared read the
/// listing rows use, with the ENOENT raise they all need.
fn read_names(path: &str) -> Result<Vec<String>, Signal> {
    let entries = std::fs::read_dir(path).map_err(|e| raise_errno(&e, "dir_initialize", path))?;
    Ok(entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> RubyValue {
        RubyValue::Str(crate::collections::string_new(v.to_string()))
    }
    fn cls() -> RubyValue {
        RubyValue::Class(spinel_abi::DIR_CLASS)
    }
    // These exercise the default (non-FNM_DOTMATCH) matching; shadow the real
    // helpers so the assertions below read without a trailing `false`.
    fn seg_matches(pat: &str, name: &str) -> bool {
        super::seg_matches(pat, name, false)
    }
    fn glob(pattern: &str) -> Vec<String> {
        super::glob(pattern, false)
    }

    // --- The matcher: pure, no disk --------------------------------------

    #[test]
    fn star_matches_within_a_segment() {
        assert!(seg_matches("*.rb", "x.rb"));
        assert!(seg_matches("*", "anything"));
        assert!(seg_matches("x*", "xyz"));
        assert!(seg_matches("*z", "xyz"));
        assert!(seg_matches("x*z", "xyz"));
        assert!(!seg_matches("*.rb", "x.txt"));
    }

    #[test]
    fn question_matches_exactly_one_char() {
        assert!(seg_matches("?.rb", "x.rb"));
        assert!(!seg_matches("?.rb", "xy.rb"));
        assert!(!seg_matches("?", ""));
    }

    /// A leading dot is hidden from a wildcard -- Ruby's rule, and the one a
    /// naive matcher gets wrong.
    #[test]
    fn a_leading_dot_is_hidden_from_wildcards() {
        assert!(!seg_matches("*", ".hidden"));
        assert!(!seg_matches("*.rb", ".x.rb"));
        // ...but a literal leading dot in the pattern sees it.
        assert!(seg_matches(".*", ".hidden"));
        assert!(seg_matches(".hidden", ".hidden"));
    }

    #[test]
    fn brace_alternation_tries_each_branch() {
        assert!(seg_matches("*.{rb,txt}", "x.rb"));
        assert!(seg_matches("*.{rb,txt}", "y.txt"));
        assert!(!seg_matches("*.{rb,txt}", "z.md"));
        assert!(seg_matches("{a,b}c", "ac"));
        assert!(seg_matches("{a,b}c", "bc"));
        assert!(!seg_matches("{a,b}c", "cc"));
    }

    #[test]
    fn character_classes_and_ranges() {
        assert!(seg_matches("[abc].rb", "a.rb"));
        assert!(seg_matches("[a-z].rb", "q.rb"));
        assert!(!seg_matches("[a-z].rb", "Q.rb"));
        assert!(seg_matches("[!a].rb", "b.rb"));
        assert!(!seg_matches("[!a].rb", "a.rb"));
    }

    /// A nested group's commas belong to it, not to the outer split.
    #[test]
    fn nested_braces_split_only_at_top_level() {
        let body: Vec<char> = "a,{b,c},d".chars().collect();
        let alts = split_alternatives(&body);
        assert_eq!(alts.len(), 3, "expected a | {{b,c}} | d");
        assert_eq!(alts[1].iter().collect::<String>(), "{b,c}");
    }

    // --- The disk-touching surface ---------------------------------------

    /// A unique tree per test: these mutate the filesystem and Rust runs
    /// tests in parallel threads.
    struct Tree(String);
    impl Tree {
        fn new(name: &str) -> Tree {
            let root = format!(
                "{}/spinel_dir_test_{}_{}",
                std::env::temp_dir().to_string_lossy(),
                std::process::id(),
                name
            );
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(format!("{root}/a/b")).unwrap();
            std::fs::write(format!("{root}/x.rb"), "").unwrap();
            std::fs::write(format!("{root}/y.txt"), "").unwrap();
            std::fs::write(format!("{root}/.hidden"), "").unwrap();
            std::fs::write(format!("{root}/a/p.rb"), "").unwrap();
            std::fs::write(format!("{root}/a/b/q.rb"), "").unwrap();
            Tree(root)
        }
    }
    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn names_of(v: RubyValue) -> Vec<String> {
        let RubyValue::Array(a) = v else {
            panic!("expected an Array")
        };
        let mut out: Vec<String> = a.lock().iter().map(|e| e.to_display_string()).collect();
        out.sort();
        out
    }

    #[test]
    fn children_excludes_dot_entries_and_entries_includes_them() {
        let t = Tree::new("listing");
        let kids = names_of(dir_children(&cls(), &[s(&t.0)], None).unwrap());
        assert_eq!(kids, vec![".hidden", "a", "x.rb", "y.txt"]);

        let all = names_of(dir_entries(&cls(), &[s(&t.0)], None).unwrap());
        assert!(all.contains(&".".to_string()) && all.contains(&"..".to_string()));
        assert_eq!(all.len(), kids.len() + 2);
    }

    /// Listing a missing directory raises Errno::ENOENT (registry-less: a
    /// panic), rather than answering an empty list.
    #[test]
    fn listing_a_missing_directory_raises() {
        let r = std::panic::catch_unwind(|| dir_entries(&cls(), &[s("/nope/nope")], None));
        assert!(r.is_err());
    }

    #[test]
    fn exist_p_is_true_only_for_directories() {
        let t = Tree::new("exist");
        assert!(matches!(
            dir_exist_p(&cls(), &[s(&t.0)], None).unwrap(),
            RubyValue::Bool(true)
        ));
        // A FILE is not a directory.
        assert!(matches!(
            dir_exist_p(&cls(), &[s(&format!("{}/x.rb", t.0))], None).unwrap(),
            RubyValue::Bool(false)
        ));
        assert!(matches!(
            dir_exist_p(&cls(), &[s("/nope/nope")], None).unwrap(),
            RubyValue::Bool(false)
        ));
    }

    /// `glob` against the oracle's own answers for this exact tree.
    #[test]
    fn glob_matches_the_oracle_shapes() {
        let t = Tree::new("glob");
        let prev = std::env::current_dir().unwrap();
        // `chdir` is process-global; keep the window tiny and restore it.
        std::env::set_current_dir(&t.0).unwrap();

        assert_eq!(glob("*.rb"), vec!["x.rb"]);
        assert_eq!(glob("a/*.rb"), vec!["a/p.rb"]);
        assert_eq!(glob("?.rb"), vec!["x.rb"]);
        let mut braces = glob("*.{rb,txt}");
        braces.sort();
        assert_eq!(braces, vec!["x.rb", "y.txt"]);
        // `**` spans directories, as whole segments.
        let mut deep = glob("**/*.rb");
        deep.sort();
        assert_eq!(deep, vec!["a/b/q.rb", "a/p.rb", "x.rb"]);
        // A hidden file stays hidden from a wildcard.
        assert!(!glob("*").contains(&".hidden".to_string()));

        std::env::set_current_dir(prev).unwrap();
    }

    #[test]
    fn mkdir_and_rmdir_round_trip() {
        let t = Tree::new("mkdir");
        let p = format!("{}/fresh", t.0);
        dir_mkdir(&cls(), &[s(&p)], None).unwrap();
        assert!(std::path::Path::new(&p).is_dir());
        assert!(matches!(
            dir_empty_p(&cls(), &[s(&p)], None).unwrap(),
            RubyValue::Bool(true)
        ));
        dir_rmdir(&cls(), &[s(&p)], None).unwrap();
        assert!(!std::path::Path::new(&p).exists());
    }

    #[test]
    fn pwd_answers_an_absolute_path() {
        let got = dir_pwd(&cls(), &[], None).unwrap().to_display_string();
        assert!(got.starts_with('/'), "pwd was not absolute: {got}");
    }

    #[test]
    fn lookup_finds_the_dir_names() {
        assert!(lookup_class("pwd").is_some());
        assert!(lookup_class("glob").is_some());
        assert!(lookup_class("[]").is_some());
        assert!(lookup_class("nope").is_none());
    }
}

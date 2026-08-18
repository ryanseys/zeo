//! `Dir` (CRuby dir.c) -- directory listing, the working directory, and
//! `glob`.
//!
//! The glob matcher is hand-ported rather than delegating to the `glob`
//! crate: Ruby's own semantics differ in ways that matter (`**` spans
//! directories only as a whole path SEGMENT, `{a,b}` alternation is Ruby's
//! not the shell's, and a leading `.` is hidden from `*` unless matched
//! literally). Wrapping a crate would mean fighting its opinions at every
//! one of those points; the rules themselves are short.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use parking_lot::Mutex;
use zeo_macros::ruby_class;

use crate::builtins::file::{path_arg, raise_errno};
use crate::builtins::{arg_error, block_or_enum, io_error, type_error};
use crate::dispatch::{RObj, RubyObject};
use crate::{RubyValue, Signal};
use zeo_abi::{ClassId, DIR_CLASS};

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
#[allow(clippy::too_many_arguments)]
fn glob_walk(
    base: &str,
    prefix: &str,
    segs: &[&str],
    out: &mut Vec<String>,
    dotmatch: bool,
    dirs_only: bool,
) {
    let Some((seg, rest)) = segs.split_first() else {
        return;
    };
    // For an absolute pattern `base` is empty and the root to read is `/`
    // (an empty `read_dir("")` reads nothing -- the bug that made absolute
    // globs return nothing). A relative pattern's base is ".".
    let dir_path = if prefix.is_empty() {
        if base.is_empty() {
            "/".to_string()
        } else {
            base.to_string()
        }
    } else {
        format!("{base}/{prefix}")
    };

    // `**` spans zero or more whole directory segments.
    if *seg == "**" {
        // Zero segments: try the rest right here.
        if rest.is_empty() {
            // A trailing `**` matches directories themselves.
            if !prefix.is_empty() {
                out.push(finish(prefix, dirs_only));
            }
        } else {
            glob_walk(base, prefix, rest, out, dotmatch, dirs_only);
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
                let next = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}/{name}")
                };
                glob_walk(base, &next, segs, out, dotmatch, dirs_only);
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
        let next = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let child = if prefix.is_empty() {
            format!("{base}/{name}")
        } else {
            format!("{base}/{prefix}/{name}")
        };
        if rest.is_empty() {
            // A pattern ending in `/` matches DIRECTORIES only, and every
            // answer keeps that slash (`Dir.glob("**/")` is `["sub/"]`).
            if !dirs_only || std::path::Path::new(&child).is_dir() {
                out.push(finish(&next, dirs_only));
            }
        } else if std::path::Path::new(&child).is_dir() {
            glob_walk(base, &next, rest, out, dotmatch, dirs_only);
        }
    }
}

/// A matched path as the glob ANSWERS it -- with the trailing slash a
/// directory-only pattern asked for.
fn finish(path: &str, dirs_only: bool) -> String {
    match dirs_only {
        true => format!("{path}/"),
        false => path.to_string(),
    }
}

/// One glob pattern -> matching paths, relative to `root` (the `base:` keyword
/// or the cwd), or absolute if the pattern is. Ruby sorts glob results.
fn glob(pattern: &str, dotmatch: bool, root: Option<&str>) -> Vec<String> {
    let absolute = pattern.starts_with('/');
    let dirs_only = pattern.ends_with('/');
    let (base, pat) = if absolute {
        ("", pattern.trim_start_matches('/'))
    } else {
        (root.unwrap_or("."), pattern)
    };
    let segs: Vec<&str> = pat.split('/').filter(|s| !s.is_empty()).collect();
    let mut out = Vec::new();
    glob_walk(
        if absolute { "" } else { base },
        "",
        &segs,
        &mut out,
        dotmatch,
        dirs_only,
    );
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
    /// The directory this was opened from, as given. `None` for a `Dir.for_fd`
    /// handle, which has only a descriptor -- exactly what `#path` reports.
    /// Mutex-wrapped (with `entries`) only so the private `#initialize` row
    /// can re-seed the handle in place.
    path: Mutex<Option<String>>,
    /// Every entry, INCLUDING `.` and `..` (what `#read`/`#each` iterate).
    entries: Mutex<Vec<String>>,
    /// The read cursor into `entries`.
    pos: AtomicUsize,
    /// `false` after `#close`; every later operation raises IOError.
    open: AtomicBool,
    /// The live descriptor `#fileno` answers, and which `Dir.for_fd`/`fchdir`
    /// need. `-1` when there is none.
    fd: std::sync::atomic::AtomicI32,
}

impl Drop for RDir {
    fn drop(&mut self) {
        let fd = self.fd.load(Ordering::Relaxed);
        if fd >= 0 {
            unsafe { libc::close(fd) };
        }
    }
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
        // The copy gets its own descriptor, so closing either leaves the other
        // usable -- CRuby's `Dir#dup` reopens for the same reason.
        let fd = match self.fd.load(Ordering::Relaxed) {
            n if n >= 0 => unsafe { libc::dup(n) },
            _ => -1,
        };
        Arc::new(RDir {
            path: Mutex::new(self.path.lock().clone()),
            entries: Mutex::new(self.entries.lock().clone()),
            pos: AtomicUsize::new(self.pos.load(Ordering::Relaxed)),
            open: AtomicBool::new(self.open.load(Ordering::Relaxed)),
            fd: std::sync::atomic::AtomicI32::new(fd),
        })
    }
}

/// A `Dir` handle value over an already-read listing.
fn dir_value(path: Option<String>, mut entries: Vec<String>, fd: libc::c_int) -> RubyValue {
    entries.push(".".to_string());
    entries.push("..".to_string());
    RubyValue::Object(Arc::new(RDir {
        path: Mutex::new(path),
        entries: Mutex::new(entries),
        pos: AtomicUsize::new(0),
        open: AtomicBool::new(true),
        fd: std::sync::atomic::AtomicI32::new(fd),
    }))
}

/// Open `path` as a `Dir` handle value -- the shared body of `Dir.new`/`.open`.
fn open_dir(path: &str) -> Result<RubyValue, Signal> {
    // `read_names` raises ENOENT for a missing path (the `dir_initialize`
    // syscall name CRuby reports).
    let entries = read_names(path)?;
    // A descriptor beside the snapshot, so `#fileno` and `Dir.for_fd` work.
    let fd = std::ffi::CString::new(path)
        .ok()
        .map(|c| unsafe { libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY) })
        .unwrap_or(-1);
    Ok(dir_value(Some(path.to_string()), entries, fd))
}

/// The entry names behind an already-open descriptor. Reads through a DUP, so
/// `closedir` frees only the copy and the caller's descriptor stays open.
fn read_names_fd(fd: libc::c_int) -> Result<Vec<String>, Signal> {
    let bad = || crate::dispatch::raise_error("Errno::EBADF", "Bad file descriptor".to_string());
    let copy = unsafe { libc::dup(fd) };
    if copy < 0 {
        return Err(bad());
    }
    let handle = unsafe { libc::fdopendir(copy) };
    if handle.is_null() {
        unsafe { libc::close(copy) };
        return Err(bad());
    }
    let mut names = Vec::new();
    loop {
        let entry = unsafe { libc::readdir(handle) };
        if entry.is_null() {
            break;
        }
        let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) };
        let name = name.to_string_lossy().into_owned();
        if name != "." && name != ".." {
            names.push(name);
        }
    }
    unsafe { libc::closedir(handle) };
    Ok(names)
}

fn recv_dir(recv: &RubyValue) -> Result<&RDir, Signal> {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RDir>()
            .ok_or_else(|| type_error!("not a Dir")),
        _ => Err(type_error!("not a Dir")),
    }
}

/// Raise IOError on a closed handle, else hand back the live `RDir`.
fn live_dir(recv: &RubyValue) -> Result<&RDir, Signal> {
    let d = recv_dir(recv)?;
    if !d.open.load(Ordering::Relaxed) {
        return Err(io_error!("closed directory"));
    }
    Ok(d)
}

/// The matcher behind `Dir.glob` and `Dir[]`. Multiple patterns union; a block
/// takes each match and the call answers nil.
fn glob_matches(args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    // A trailing Integer FNM flags argument (`File::FNM_DOTMATCH`, ...)
    // governs matching for all patterns. FNM_DOTMATCH is bit 0x4.
    let dotmatch = args
        .iter()
        .filter_map(|a| {
            if let RubyValue::Int(f) = a {
                Some(*f)
            } else {
                None
            }
        })
        .any(|f| f & 0x4 != 0);
    // `base:` roots the search there and answers paths RELATIVE to it.
    let base_key = RubyValue::Symbol(crate::Symbol::intern("base"));
    let root = args.iter().find_map(|a| match a {
        RubyValue::Hash(h) if crate::collections::hash_has_key(h, &base_key) => {
            match crate::hash_get(h, &base_key) {
                RubyValue::Nil => None,
                v => Some(path_arg(&v, "glob")),
            }
        }
        _ => None,
    });
    let root = match root {
        Some(r) => Some(r?),
        None => None,
    };
    let root = root.as_deref();
    let mut all = Vec::new();
    for a in args {
        match a {
            RubyValue::Array(pats) => {
                for p in pats.lock().iter() {
                    all.extend(glob(&path_arg(p, "glob")?, dotmatch, root));
                }
            }
            // A trailing options Hash (`base:`) or the Integer FNM flags
            // argument itself is not a pattern.
            RubyValue::Hash(_) | RubyValue::Int(_) => {}
            v => all.extend(glob(&path_arg(v, "glob")?, dotmatch, root)),
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

ruby_class! {
    Dir = zeo_abi::DIR_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::ENUMERABLE_CLASS;

    // `Dir.new(path)` / `Dir.open(path)` -- a handle over the directory's
    // entries. The block form of `open` yields the handle and closes it after.
    // `Dir.open` takes `(name, encoding: nil, &block)` and so reports -2;
    // `Dir.new` is a C function that discarded its signature and reports -1.
    // Same handle either way, so they share `open_handle`.
    def self."open" as open_handle (_recv, arg1, _arg2?, &block) {
        let path = path_arg(arg1, "open")?;
        let dir = open_dir(&path)?;
        let Some(RubyValue::Proc(p)) = block else {
            return Ok(dir);
        };
        let out = p.call(std::slice::from_ref(&dir));
        let _ = dir_h_close(&dir, &[], None);
        out
    }
    def self."new" cfunc (recv, arg1, _arg2?, &block) {
        open_handle(recv, std::slice::from_ref(arg1), block)
    }
    // `Dir.for_fd(fd)` -- a handle over an already-open directory descriptor.
    // It lists through the descriptor, so it has no path and `#path` is nil.
    def self."for_fd"(_recv, arg) {
        let fd = crate::builtins::convert::to_index(arg)? as libc::c_int;
        let entries = read_names_fd(fd)?;
        Ok(dir_value(None, entries, fd))
    }
    // `Dir.fchdir(fd)` -- `chdir` to an open directory descriptor. The block
    // form restores the previous directory afterwards, as `Dir.chdir` does.
    def self."fchdir"(_recv, arg, &block) {
        let fd = crate::builtins::convert::to_index(arg)? as libc::c_int;
        let previous = std::env::current_dir().ok();
        if unsafe { libc::fchdir(fd) } != 0 {
            return Err(raise_errno(&std::io::Error::last_os_error(), "fchdir", ""));
        }
        let Some(RubyValue::Proc(p)) = block else {
            return Ok(RubyValue::Int(0));
        };
        let out = p.call(&[]);
        if let Some(prev) = previous {
            let _ = std::env::set_current_dir(prev);
        }
        out
    }
    def self."pwd" | "getwd"(_recv) {
        let d = std::env::current_dir().map_err(|e| raise_errno(&e, "getcwd", "."))?;
        Ok(str_val(d.to_string_lossy().into_owned()))
    }
    // `Dir.chroot(path)` -- needs root, and reports the kernel's refusal
    // (`EPERM`) verbatim when it does not have it.
    def self."chroot"(_recv, arg) {
        let path = path_arg(arg, "chroot")?;
        let c = std::ffi::CString::new(path.clone())
            .map_err(|_| crate::builtins::arg_error!("string contains null byte"))?;
        // SAFETY: `c` is a valid NUL-terminated path.
        if unsafe { libc::chroot(c.as_ptr()) } != 0 {
            return Err(raise_errno(&std::io::Error::last_os_error(), "chroot", &path));
        }
        Ok(RubyValue::Int(0))
    }
    def self."chdir"(_recv, arg?, &block) {
        let target = match arg {
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
    def self."entries" cfunc (_recv, arg1, _arg2?) {
        let path = path_arg(arg1, "entries")?;
        let mut names = read_names(&path)?;
        names.push(".".to_string());
        names.push("..".to_string());
        names.sort();
        Ok(RubyValue::Array(crate::collections::array_new(
            names.into_iter().map(str_val).collect(),
        )))
    }
    def self."children" cfunc (_recv, arg1, _arg2?) {
        let path = path_arg(arg1, "children")?;
        let mut names = read_names(&path)?;
        names.sort();
        Ok(RubyValue::Array(crate::collections::array_new(
            names.into_iter().map(str_val).collect(),
        )))
    }
    // `Dir.foreach(path)` -- yield each entry name (INCLUDING `.` and `..`,
    // like `entries`); without a block, an Enumerator.
    def self."foreach" cfunc (recv, dirname, _opt?, &block) {
        let path = path_arg(dirname, "foreach")?;
        let p = block_or_enum!(recv, __args, block);
        let mut names = read_names(&path)?;
        names.push(".".to_string());
        names.push("..".to_string());
        names.sort();
        for n in names {
            p.call(&[str_val(n)])?;
        }
        Ok(RubyValue::Nil)
    }
    // Blockless answers an Enumerator, like every `RETURN_ENUMERATOR` row --
    // it was raising `no block given (yield)`.
    def self."each_child" cfunc (recv, arg, &block) {
        let path = path_arg(arg, "each_child")?;
        let p = block_or_enum!(recv, __args, block);
        let mut names = read_names(&path)?;
        names.sort();
        for n in names {
            p.call(&[str_val(n)])?;
        }
        Ok(RubyValue::Nil)
    }
    def self."exist?"(_recv, arg) {
        let path = path_arg(arg, "exist?")?;
        Ok(RubyValue::Bool(std::path::Path::new(&path).is_dir()))
    }
    def self."empty?"(_recv, arg) {
        let path = path_arg(arg, "empty?")?;
        // A missing path raises (ENOENT); an existing NON-directory is simply
        // not an empty directory -> false (CRuby doesn't raise there).
        let md = std::fs::metadata(&path)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "empty?", &path))?;
        if !md.is_dir() {
            return Ok(RubyValue::Bool(false));
        }
        Ok(RubyValue::Bool(read_names(&path)?.is_empty()))
    }
    def self."mkdir" cfunc (_recv, arg1, _arg2?) {
        let path = path_arg(arg1, "mkdir")?;
        std::fs::create_dir(&path).map_err(|e| raise_errno(&e, "dir_s_mkdir", &path))?;
        Ok(RubyValue::Int(0))
    }
    // `mktmpdir([prefix | [prefix, suffix]], [parent])`: create a fresh
    // temporary directory. With a block, yield its path and remove the
    // directory (recursively) afterward -- even if the block raises --
    // answering the block's value; without a block, answer the path for the
    // caller to clean up.
    def self."mktmpdir"(_recv, arg1?, arg2?, &block) {
        let (prefix, suffix) = match arg1 {
            None | Some(RubyValue::Nil) => ("d".to_string(), String::new()),
            Some(RubyValue::Str(s)) => (s.lock().to_utf8_lossy().into_owned(), String::new()),
            Some(RubyValue::Array(a)) => {
                let a = a.lock();
                (
                    a.first().map(|v| v.to_display_string()).unwrap_or_default(),
                    a.get(1).map(|v| v.to_display_string()).unwrap_or_default(),
                )
            }
            // NOT an implicit-conversion site: CRuby's Dir.mktmpdir rejects
            // a non-String/Array prefix with its own ArgumentError
            // ("unexpected prefix: 1", oracle-verified).
            Some(other) => {
                return Err(arg_error!("unexpected prefix: {}", other.inspect_string()))
            }
        };
        let parent = match arg2 {
            Some(RubyValue::Str(s)) => std::path::PathBuf::from(s.lock().to_utf8_lossy().into_owned()),
            _ => std::env::temp_dir(),
        };
        let pid = std::process::id();
        let path = loop {
            let rand = crate::builtins::kernel::prng_limited(u64::MAX);
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
    def self."rmdir" | "unlink" | "delete"(_recv, arg) {
        let path = path_arg(arg, "rmdir")?;
        std::fs::remove_dir(&path).map_err(|e| raise_errno(&e, "dir_s_rmdir", &path))?;
        Ok(RubyValue::Int(0))
    }
    def self."home"(_recv, _arg?) {
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
    def self."glob" (_recv, _pattern, _flags?, **_opts, &block) {
        glob_matches(__args, block)
    }
    // `Dir[]` is the same matcher with a looser signature -- any number of
    // patterns, and none at all answers `[]` where `glob` would raise.
    def self."[]" (_recv, *args, &block) {
        glob_matches(args, block)
    }

    // Re-init reopens the handle over a new path (`d.send(:initialize, "/")`):
    // a fresh snapshot and descriptor, cursor rewound, the old descriptor
    // closed. No frozen check -- a frozen Dir re-inits fine (oracle-pinned).
    // Accepts `(name, encoding: ...)` like `Dir.new`; the kwargs are ignored.
    private def "initialize"(recv, name, **_opts) {
        let d = recv_dir(recv)?;
        let path = path_arg(name, "open")?;
        let mut entries = read_names(&path)?;
        entries.push(".".to_string());
        entries.push("..".to_string());
        let fd = std::ffi::CString::new(path.clone())
            .ok()
            .map(|c| unsafe { libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY) })
            .unwrap_or(-1);
        let old = d.fd.swap(fd, Ordering::Relaxed);
        if old >= 0 {
            unsafe { libc::close(old) };
        }
        *d.entries.lock() = entries;
        *d.path.lock() = Some(path);
        d.pos.store(0, Ordering::Relaxed);
        d.open.store(true, Ordering::Relaxed);
        Ok(recv.clone())
    }
    // `#read` -- the next entry name (INCLUDING `.`/`..`), or nil at the end.
    def "read"(recv) {
        let d = live_dir(recv)?;
        let i = d.pos.fetch_add(1, Ordering::Relaxed);
        let entries = d.entries.lock();
        Ok(match entries.get(i) {
            Some(name) => str_val(name.clone()),
            None => {
                // Don't advance past the end.
                d.pos.store(entries.len(), Ordering::Relaxed);
                RubyValue::Nil
            }
        })
    }
    // `#each` -- yield every entry from the current cursor onward (INCLUDING
    // `.`/`..`); without a block, an Enumerator over the entry array.
    def "each"(recv, &block) {
        let entries: Vec<RubyValue> =
            live_dir(recv)?.entries.lock().iter().cloned().map(str_val).collect();
        let p = block_or_enum!(recv, &[], block);
        for e in entries {
            p.call(&[e])?;
        }
        Ok(recv.clone())
    }
    // `#each_child` -- like `#each` but WITHOUT `.` and `..`.
    def "each_child"(recv, &block) {
        let entries: Vec<RubyValue> = live_dir(recv)?
            .entries.lock().iter().filter(|n| *n != "." && *n != "..").cloned().map(str_val).collect();
        let p = block_or_enum!(recv, &[], block);
        for e in entries {
            p.call(&[e])?;
        }
        Ok(recv.clone())
    }
    // `#children` / `#entries` -- the entry names as an Array (children drops
    // `.`/`..`); both snapshot the whole listing regardless of the cursor.
    def "children"(recv) {
        let out: Vec<RubyValue> = live_dir(recv)?
            .entries.lock().iter().filter(|n| *n != "." && *n != "..").cloned().map(str_val).collect();
        Ok(RubyValue::Array(crate::collections::array_new(out)))
    }
    def "entries" cfunc (recv) {
        let out: Vec<RubyValue> =
            live_dir(recv)?.entries.lock().iter().cloned().map(str_val).collect();
        Ok(RubyValue::Array(crate::collections::array_new(out)))
    }
    // `Dir#chdir` -- change to the directory this handle was opened on. The
    // block form restores the previous directory afterwards, as the class
    // method's does.
    def "chdir"(recv, &block) {
        let Some(target) = recv_dir(recv)?.path.lock().clone() else {
            return Err(crate::builtins::io_error!("closed directory"));
        };
        let prev = std::env::current_dir().map_err(|e| raise_errno(&e, "getcwd", "."))?;
        std::env::set_current_dir(&target).map_err(|e| raise_errno(&e, "chdir", &target))?;
        let Some(RubyValue::Proc(p)) = block else {
            return Ok(RubyValue::Int(0));
        };
        let r = p.call(&[str_val(target)]);
        let _ = std::env::set_current_dir(prev);
        r
    }
    def "path" | "to_path"(recv) {
        Ok(match &*recv_dir(recv)?.path.lock() {
            Some(p) => str_val(p.clone()),
            None => RubyValue::Nil,
        })
    }
    // `#pos`/`#tell` read the cursor; `#pos=`/`#seek` set it; `#rewind` zeroes it.
    def "pos" | "tell"(recv) {
        Ok(RubyValue::Int(live_dir(recv)?.pos.load(Ordering::Relaxed) as i64))
    }
    // `pos=` answers the new position; `seek` answers the Dir itself (so it
    // chains), the one behavioural difference between the two.
    def "pos="(recv, arg) {
        let n = crate::builtins::convert::to_index(arg)?;
        live_dir(recv)?.pos.store(n.max(0) as usize, Ordering::Relaxed);
        Ok((*arg).clone())
    }
    def "seek"(recv, arg) {
        let n = &crate::builtins::convert::to_index(arg)?;
        live_dir(recv)?.pos.store((*n).max(0) as usize, Ordering::Relaxed);
        Ok(recv.clone())
    }
    def "rewind"(recv) {
        live_dir(recv)?.pos.store(0, Ordering::Relaxed);
        Ok(recv.clone())
    }
    def "close" as dir_h_close (recv) {
        let d = recv_dir(recv)?;
        d.open.store(false, Ordering::Relaxed);
        let fd = d.fd.swap(-1, Ordering::Relaxed);
        if fd >= 0 {
            unsafe { libc::close(fd) };
        }
        Ok(RubyValue::Nil)
    }
    def "fileno"(recv) {
        Ok(RubyValue::Int(live_dir(recv)?.fd.load(Ordering::Relaxed) as i64))
    }
    def "inspect" | "to_s"(recv) {
        Ok(str_val(match &*recv_dir(recv)?.path.lock() {
            Some(p) => format!("#<Dir:{p}>"),
            None => "#<Dir>".to_string(),
        }))
    }
}

/// The entry names in `path`, excluding `.`/`..` -- the shared read the
/// listing rows use, with the ENOENT raise they all need.
fn read_names(path: &str) -> Result<Vec<String>, Signal> {
    // Gvl-released: the lazy ReadDir iterator syscalls per entry, so the
    // whole scan runs outside an armed Gvl.
    crate::gvl::without_gvl(|| {
        let entries = std::fs::read_dir(path)?;
        Ok(entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect())
    })
    .map_err(|e: std::io::Error| raise_errno(&e, "dir_initialize", path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        (crate::builtins::registered_table(zeo_abi::DIR_CLASS)
            .unwrap()
            .class
            .as_ref()
            .unwrap()
            .lookup)(name)
        .unwrap()
    }

    fn s(v: &str) -> RubyValue {
        RubyValue::Str(crate::collections::string_new(v.to_string()))
    }
    fn cls() -> RubyValue {
        RubyValue::Class(zeo_abi::DIR_CLASS)
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
                "{}/zeo_dir_test_{}_{}",
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
        let kids = names_of(cmethod("children")(&cls(), &[s(&t.0)], None).unwrap());
        assert_eq!(kids, vec![".hidden", "a", "x.rb", "y.txt"]);

        let all = names_of(cmethod("entries")(&cls(), &[s(&t.0)], None).unwrap());
        assert!(all.contains(&".".to_string()) && all.contains(&"..".to_string()));
        assert_eq!(all.len(), kids.len() + 2);
    }

    /// Listing a missing directory raises Errno::ENOENT (registry-less: a
    /// panic), rather than answering an empty list.
    #[test]
    fn listing_a_missing_directory_raises() {
        let r = std::panic::catch_unwind(|| cmethod("entries")(&cls(), &[s("/nope/nope")], None));
        assert!(r.is_err());
    }

    #[test]
    fn exist_p_is_true_only_for_directories() {
        let t = Tree::new("exist");
        assert!(matches!(
            cmethod("exist?")(&cls(), &[s(&t.0)], None).unwrap(),
            RubyValue::Bool(true)
        ));
        // A FILE is not a directory.
        assert!(matches!(
            cmethod("exist?")(&cls(), &[s(&format!("{}/x.rb", t.0))], None).unwrap(),
            RubyValue::Bool(false)
        ));
        assert!(matches!(
            cmethod("exist?")(&cls(), &[s("/nope/nope")], None).unwrap(),
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
        cmethod("mkdir")(&cls(), &[s(&p)], None).unwrap();
        assert!(std::path::Path::new(&p).is_dir());
        assert!(matches!(
            cmethod("empty?")(&cls(), &[s(&p)], None).unwrap(),
            RubyValue::Bool(true)
        ));
        cmethod("rmdir")(&cls(), &[s(&p)], None).unwrap();
        assert!(!std::path::Path::new(&p).exists());
    }

    #[test]
    fn pwd_answers_an_absolute_path() {
        let got = cmethod("pwd")(&cls(), &[], None)
            .unwrap()
            .to_display_string();
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

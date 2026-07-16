//! `Dir` (CRuby dir.c) -- directory listing, the working directory, and
//! `glob`.
//!
//! The glob matcher is hand-ported rather than delegating to the `glob`
//! crate: Ruby's own semantics differ in ways that matter (`**` spans
//! directories only as a whole path SEGMENT, `{a,b}` alternation is Ruby's
//! not the shell's, and a leading `.` is hidden from `*` unless matched
//! literally). Wrapping a crate would mean fighting its opinions at every
//! one of those points; the rules themselves are short.

use crate::builtins::file::{path_arg, raise_errno};
use crate::builtins::{arity, builtin_methods};
use crate::{RubyValue, Signal};

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
fn seg_matches(pat: &str, name: &str) -> bool {
    if name.starts_with('.') && !pat.starts_with('.') {
        return false;
    }
    let p: Vec<char> = pat.chars().collect();
    let n: Vec<char> = name.chars().collect();
    match_segment(&p, &n)
}

/// Walk `dir` against the remaining glob segments, pushing every match onto
/// `out`. `prefix` is the path built so far (as the caller wants it echoed
/// back -- glob answers paths relative to the same root the pattern was).
fn glob_walk(base: &str, prefix: &str, segs: &[&str], out: &mut Vec<String>) {
    let Some((seg, rest)) = segs.split_first() else {
        return;
    };
    let dir_path = if prefix.is_empty() { base.to_string() } else { format!("{base}/{prefix}") };

    // `**` spans zero or more whole directory segments.
    if *seg == "**" {
        // Zero segments: try the rest right here.
        if rest.is_empty() {
            // A trailing `**` matches directories themselves.
            if !prefix.is_empty() {
                out.push(prefix.to_string());
            }
        } else {
            glob_walk(base, prefix, rest, out);
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
                glob_walk(base, &next, segs, out);
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
    names.sort();
    for name in names {
        if !seg_matches(seg, &name) {
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
                glob_walk(base, &next, rest, out);
            }
        }
    }
}

/// One glob pattern -> matching paths, relative to the cwd (or absolute, if
/// the pattern is). Ruby sorts glob results.
fn glob(pattern: &str) -> Vec<String> {
    let absolute = pattern.starts_with('/');
    let (base, pat) = if absolute { ("", pattern.trim_start_matches('/')) } else { (".", pattern) };
    let segs: Vec<&str> = pat.split('/').filter(|s| !s.is_empty()).collect();
    let mut out = Vec::new();
    glob_walk(if absolute { "" } else { base }, "", &segs, &mut out);
    if absolute {
        out = out.into_iter().map(|p| format!("/{p}")).collect();
    }
    out.sort();
    out.dedup();
    out
}

builtin_methods! {
    pub(crate) fn lookup_class;

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
    "each_child" => fn dir_each_child(_recv, args, block) {
        arity!(args, 1);
        let path = path_arg(&args[0], "each_child")?;
        let Some(RubyValue::Proc(p)) = block else {
            return Err(crate::dispatch::raise_error(
                "LocalJumpError",
                "no block given (yield)".to_string(),
            ));
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
                    format!("no implicit conversion of {} into String", crate::builtins::class_name_of(other)),
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
        let mut all = Vec::new();
        for a in args {
            match a {
                RubyValue::Array(pats) => {
                    for p in pats.lock().iter() {
                        all.extend(glob(&path_arg(p, "glob")?));
                    }
                }
                // A trailing options Hash (`base:`, `File::FNM_*`) is
                // accepted and ignored rather than mis-globbed as a pattern.
                // TODO(plan P-B): honor `base:` and the FNM flags.
                RubyValue::Hash(_) => {}
                v => all.extend(glob(&path_arg(v, "glob")?)),
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

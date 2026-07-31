//! `Pathname` -- the `pathname` stdlib class, a value wrapping a path String.
//!
//! `require`-gated on `"pathname"`. A FOCUSED native port: the ~35 methods
//! rubygems/bundler actually reach (path manipulation over `File`/`std::path`
//! plus the common filesystem predicates/IO). Pure path operations reuse
//! `builtins::file`'s helpers so they match `File`'s semantics exactly; IO goes
//! through `std::fs`. Not every CRuby Pathname method is present -- the rarely
//! used surface (`mountpoint?`, `birthtime`, `world_writable?`, ...) is omitted
//! and can be added incrementally.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::builtins::arity;
use crate::builtins::file::{basename_of, dirname_of, expand_path_of, extname_of, path_arg};
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::{RubyValue, Signal};
use zeo_abi::{ClassId, PATHNAME_CLASS};
use zeo_macros::ruby_class;

/// A `Pathname` instance: an immutable path string. No `Mutex` -- Pathname is a
/// value class (every mutating-looking method returns a fresh Pathname).
pub(crate) struct RPathname {
    path: String,
}

impl RubyObject for RPathname {
    fn class_id(&self) -> ClassId {
        PATHNAME_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        // A Pathname's path is immutable, so it reads as frozen -- CRuby
        // freezes the wrapped string on construction.
        true
    }
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(RPathname {
            path: self.path.clone(),
        })
    }
}

fn str_val(s: String) -> RubyValue {
    RubyValue::Str(crate::string_new(s))
}

/// Wrap a path string as a fresh `Pathname` value.
fn pathname_val(path: String) -> RubyValue {
    RubyValue::Object(Arc::new(RPathname { path }))
}

fn recv_path(recv: &RubyValue) -> &str {
    match recv {
        RubyValue::Object(o) => {
            &o.as_any()
                .downcast_ref::<RPathname>()
                .expect("Pathname row on a non-Pathname receiver")
                .path
        }
        _ => panic!("Pathname row on a non-Object receiver"),
    }
}

/// The path string of an argument that is itself a Pathname or a String
/// (`p + other`, `p == other`) -- Pathname's operators accept both.
fn arg_path(v: &RubyValue) -> Result<String, Signal> {
    if let RubyValue::Object(o) = v {
        if let Some(p) = o.as_any().downcast_ref::<RPathname>() {
            return Ok(p.path.clone());
        }
    }
    path_arg(v, "pathname")
}

/// `Pathname#+` / `#/`: join `base` and `other`, with an absolute `other`
/// replacing `base` entirely (CRuby's `plus`). `PathBuf::join` implements
/// exactly that rule; the result is re-stringified with forward slashes.
fn join_two(base: &str, other: &str) -> String {
    let joined = if base.is_empty() {
        PathBuf::from(other)
    } else {
        Path::new(base).join(other)
    };
    joined.to_string_lossy().into_owned()
}

/// `Pathname#cleanpath` (a conservative subset): collapse `//`, drop `.`
/// segments and resolve `..` lexically where possible. Leading `/` and a
/// trailing single component are preserved.
fn cleanpath(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if matches!(out.last(), Some(&s) if s != "..") {
                    out.pop();
                } else if !absolute {
                    out.push("..");
                }
            }
            s => out.push(s),
        }
    }
    let body = out.join("/");
    match (absolute, body.is_empty()) {
        (true, _) => format!("/{body}"),
        (false, true) => ".".to_string(),
        (false, false) => body,
    }
}

/// `Pathname#relative_path_from(base)` (lexical, like CRuby): the path that,
/// joined onto `base`, reaches `self`. Both are cleaned first; a result that
/// would need to escape an unknown `..` in `base` is an ArgumentError.
fn relative_path_from(target: &str, base: &str) -> Result<String, Signal> {
    let t = cleanpath(target);
    let b = cleanpath(base);
    if t.starts_with('/') != b.starts_with('/') {
        return Err(raise_error(
            "ArgumentError",
            format!("different prefix: {b:?} and {t:?}"),
        ));
    }
    let split = |s: &str| -> Vec<String> {
        s.trim_start_matches('/')
            .split('/')
            .filter(|c| !c.is_empty())
            .map(str::to_string)
            .collect()
    };
    let (tc, bc) = (split(&t), split(&b));
    let common = tc.iter().zip(&bc).take_while(|(a, b)| a == b).count();
    if bc[common..].iter().any(|c| c == "..") {
        return Err(raise_error(
            "ArgumentError",
            format!("base directory may not contain ..: {b:?}"),
        ));
    }
    let ups = std::iter::repeat_n("..".to_string(), bc.len() - common);
    let rest = tc[common..].iter().cloned();
    let parts: Vec<String> = ups.chain(rest).collect();
    Ok(if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    })
}

fn io_err(path: &str, e: &std::io::Error) -> Signal {
    let class = match e.kind() {
        std::io::ErrorKind::NotFound => "Errno::ENOENT",
        std::io::ErrorKind::PermissionDenied => "Errno::EACCES",
        std::io::ErrorKind::AlreadyExists => "Errno::EEXIST",
        _ => "SystemCallError",
    };
    raise_error(class, format!("{e} - {path}"))
}

/// The immediate children of a directory as `Pathname`s. `with_directory`
/// prefixes each with `self` (CRuby's default); `.`/`..` are excluded, matching
/// `Dir.children`.
fn children(base: &str, with_directory: bool) -> Result<RubyValue, Signal> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(base).map_err(|e| io_err(base, &e))?;
    for entry in entries {
        let name = entry.map_err(|e| io_err(base, &e))?.file_name();
        let name = name.to_string_lossy().into_owned();
        let path = if with_directory {
            join_two(base, &name)
        } else {
            name
        };
        out.push(pathname_val(path));
    }
    Ok(RubyValue::Array(crate::array_new(out)))
}

ruby_class! {
    Pathname = zeo_abi::PATHNAME_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::COMPARABLE_CLASS;

    def "to_s" | "to_path" | "to_str" (recv, *args, &_b) { arity!(args, 0); Ok(str_val(recv_path(recv).to_string())) }
    def "inspect" (recv, *args, &_b) { arity!(args, 0); Ok(str_val(format!("#<Pathname:{}>", recv_path(recv)))) }
    def "freeze" (recv, *args, &_b) { arity!(args, 0); Ok(recv.clone()) }
    def "frozen?" (_recv, *args, &_b) { arity!(args, 0); Ok(RubyValue::Bool(true)) }

    // `Pathname#+`/`#/`/`#join` resolve `.`/`..` lexically at the join (CRuby's
    // `plus`), so the result is cleaned -- unlike raw `File.join`.
    def "+" | "/" (recv, *args, &_b) {
        arity!(args, 1);
        Ok(pathname_val(cleanpath(&join_two(recv_path(recv), &arg_path(&args[0])?))))
    }
    def "join" (recv, *args, &_b) {
        let mut acc = recv_path(recv).to_string();
        for a in args { acc = join_two(&acc, &arg_path(a)?); }
        Ok(pathname_val(cleanpath(&acc)))
    }
    def "basename" (recv, *args, &_b) {
        arity!(args, 0..=1);
        let suffix = match args.first() {
            None | Some(RubyValue::Nil) => None,
            Some(v) => Some(arg_path(v)?),
        };
        Ok(pathname_val(basename_of(recv_path(recv), suffix.as_deref())))
    }
    def "dirname" | "parent" (recv, *args, &_b) { arity!(args, 0); Ok(pathname_val(dirname_of(recv_path(recv)))) }
    def "extname" (recv, *args, &_b) { arity!(args, 0); Ok(str_val(extname_of(recv_path(recv)))) }
    def "expand_path" (recv, *args, &_b) {
        arity!(args, 0..=1);
        let base = match args.first() {
            None | Some(RubyValue::Nil) => None,
            Some(v) => Some(arg_path(v)?),
        };
        Ok(pathname_val(expand_path_of(recv_path(recv), base.as_deref())?))
    }
    def "cleanpath" (recv, *args, &_b) { arity!(args, 0..=1); Ok(pathname_val(cleanpath(recv_path(recv)))) }
    def "sub_ext" (recv, *args, &_b) {
        arity!(args, 1);
        let newext = arg_path(&args[0])?;
        let path = recv_path(recv);
        let cur = extname_of(path);
        let stem = &path[..path.len() - cur.len()];
        Ok(pathname_val(format!("{stem}{newext}")))
    }
    def "split" (recv, *args, &_b) {
        arity!(args, 0);
        let p = recv_path(recv);
        Ok(RubyValue::Array(crate::array_new(vec![
            pathname_val(dirname_of(p)),
            pathname_val(basename_of(p, None)),
        ])))
    }
    def "relative_path_from" (recv, *args, &_b) {
        arity!(args, 1);
        Ok(pathname_val(relative_path_from(recv_path(recv), &arg_path(&args[0])?)?))
    }

    def "absolute?" (recv, *args, &_b) { arity!(args, 0); Ok(RubyValue::Bool(recv_path(recv).starts_with('/'))) }
    def "relative?" (recv, *args, &_b) { arity!(args, 0); Ok(RubyValue::Bool(!recv_path(recv).starts_with('/'))) }
    def "root?" (recv, *args, &_b) {
        arity!(args, 0);
        let p = recv_path(recv);
        Ok(RubyValue::Bool(!p.is_empty() && p.chars().all(|c| c == '/')))
    }

    def "==" | "eql?" (recv, *args, &_b) {
        arity!(args, 1);
        let same = match &args[0] {
            RubyValue::Object(o) => o.as_any().downcast_ref::<RPathname>().is_some_and(|p| p.path == recv_path(recv)),
            _ => false,
        };
        Ok(RubyValue::Bool(same))
    }
    def "<=>" (recv, *args, &_b) {
        arity!(args, 1);
        match &args[0] {
            RubyValue::Object(o) => match o.as_any().downcast_ref::<RPathname>() {
                Some(p) => Ok(RubyValue::Int(recv_path(recv).cmp(&p.path) as i64)),
                None => Ok(RubyValue::Nil),
            },
            _ => Ok(RubyValue::Nil),
        }
    }
    def "hash" (recv, *args, &_b) {
        arity!(args, 0);
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        recv_path(recv).hash(&mut h);
        Ok(RubyValue::Int(h.finish() as i64))
    }

    def "exist?" (recv, *args, &_b) { arity!(args, 0); Ok(RubyValue::Bool(Path::new(recv_path(recv)).exists())) }
    def "directory?" (recv, *args, &_b) { arity!(args, 0); Ok(RubyValue::Bool(Path::new(recv_path(recv)).is_dir())) }
    def "file?" (recv, *args, &_b) { arity!(args, 0); Ok(RubyValue::Bool(Path::new(recv_path(recv)).is_file())) }
    def "symlink?" (recv, *args, &_b) { arity!(args, 0); Ok(RubyValue::Bool(std::fs::symlink_metadata(recv_path(recv)).map(|m| m.file_type().is_symlink()).unwrap_or(false))) }

    def "read" (recv, *args, &_b) {
        arity!(args, 0..=2);
        let p = recv_path(recv);
        Ok(str_val(std::fs::read_to_string(p).map_err(|e| io_err(p, &e))?))
    }
    def "binread" (recv, *args, &_b) {
        arity!(args, 0..=1);
        let p = recv_path(recv);
        let bytes = std::fs::read(p).map_err(|e| io_err(p, &e))?;
        Ok(str_val(String::from_utf8_lossy(&bytes).into_owned()))
    }
    def "write" (recv, *args, &_b) {
        arity!(args, 1);
        let p = recv_path(recv);
        let content = arg_path(&args[0])?;
        std::fs::write(p, &content).map_err(|e| io_err(p, &e))?;
        Ok(RubyValue::Int(content.len() as i64))
    }
    def "size" (recv, *args, &_b) {
        arity!(args, 0);
        let p = recv_path(recv);
        Ok(RubyValue::Int(std::fs::metadata(p).map_err(|e| io_err(p, &e))?.len() as i64))
    }
    def "realpath" (recv, *args, &_b) {
        arity!(args, 0..=1);
        let p = recv_path(recv);
        let canon = std::fs::canonicalize(p).map_err(|e| io_err(p, &e))?;
        Ok(pathname_val(canon.to_string_lossy().into_owned()))
    }
    def "children" (recv, *args, &_b) {
        arity!(args, 0..=1);
        let with_dir = !matches!(args.first(), Some(RubyValue::Bool(false)));
        children(recv_path(recv), with_dir)
    }
    def "mkpath" | "mkdir_p" (recv, *args, &_b) {
        arity!(args, 0..=1);
        let p = recv_path(recv);
        std::fs::create_dir_all(p).map_err(|e| io_err(p, &e))?;
        Ok(RubyValue::Int(0))
    }
    def "mkdir" (recv, *args, &_b) {
        arity!(args, 0..=1);
        let p = recv_path(recv);
        std::fs::create_dir(p).map_err(|e| io_err(p, &e))?;
        Ok(RubyValue::Int(0))
    }
    def "rmtree" | "rm_rf" (recv, *args, &_b) {
        arity!(args, 0..=1);
        let p = recv_path(recv);
        let _ = std::fs::remove_dir_all(p);
        Ok(recv.clone())
    }
    def "unlink" | "delete" (recv, *args, &_b) {
        arity!(args, 0);
        let p = recv_path(recv);
        std::fs::remove_file(p).or_else(|_| std::fs::remove_dir(p)).map_err(|e| io_err(p, &e))?;
        Ok(RubyValue::Int(1))
    }

    def self."new" (_recv, *args, &_b) {
        arity!(args, 1);
        Ok(pathname_val(arg_path(&args[0])?))
    }
    def self."getwd" | "pwd" (_recv, *args, &_b) {
        arity!(args, 0);
        let cwd = std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(pathname_val(cwd))
    }
    def self."glob" (_recv, *args, &_b) {
        arity!(args, 1..=2);
        let pattern = arg_path(&args[0])?;
        // Delegate to the runtime `Dir.glob` for shell-glob semantics, then wrap
        // each result as a Pathname.
        let dir = RubyValue::Class(zeo_abi::DIR_CLASS);
        let matches = crate::dispatch::send_value(&dir, crate::Symbol::intern("glob"), &[str_val(pattern)], None)?;
        let RubyValue::Array(arr) = matches else { return Ok(RubyValue::Array(crate::array_new(vec![]))) };
        let wrapped: Vec<RubyValue> = arr.lock().iter().map(|v| match v {
            RubyValue::Str(s) => pathname_val(s.lock().to_utf8_lossy().into_owned()),
            other => other.clone(),
        }).collect();
        Ok(RubyValue::Array(crate::array_new(wrapped)))
    }
}

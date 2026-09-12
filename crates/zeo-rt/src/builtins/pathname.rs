//! `Pathname` -- a path as a value, reachable with NO `require`.
//!
//! ruby 4.0 loads `pathname.so` before the first line, so 96 instance methods
//! and 4 class methods are there whatever the program does; `require
//! "pathname"` only reopens the class to add `#find` and `#rmtree`. That is
//! why this is a core class here rather than a gated extension.
//!
//! Most of the surface is DELEGATION: every file test, every stat reader,
//! every read and write goes to the `File` or `Dir` row that already
//! implements it, so a Pathname can never answer differently from the same
//! call spelled out. What is written here is the PATH ALGEBRA -- `#+`,
//! `#cleanpath`, `#relative_path_from`, `#ascend` and the `chop_basename`
//! peel they all stand on, ported from CRuby's own decomposition.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;

use crate::builtins::file::{extname_of, path_arg};
use crate::builtins::{arg_error, block_or_enum, frozen_error, type_error};
use crate::dispatch::{RObj, RubyObject};
use crate::{RubyValue, Signal};
use zeo_abi::{ClassId, PATHNAME_CLASS};
use zeo_macros::ruby_class;

/// A `Pathname` instance: a path string. Behind a `Mutex` only because the
/// private `#initialize` row can re-seed it in place; every other
/// mutating-looking method still returns a fresh Pathname.
pub(crate) struct RPathname {
    path: Mutex<String>,
    /// Whether `@path` EXISTS, which is not the same as whether the path is
    /// empty. Ruby keeps the path in that ivar, so `instance_variables` and
    /// `Marshal.dump` both report it -- but only once something assigned it.
    /// `Pathname.allocate` has not, and neither has a program that reopened
    /// `#initialize` in Ruby and never called super.
    has_path: AtomicBool,
    frozen: AtomicBool,
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
        self.frozen.load(Ordering::Acquire)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Release);
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    // Ruby's `Pathname` keeps its path in a plain `@path` ivar, so that is
    // what `instance_variables` reports AND what `Marshal.dump` writes. zeo
    // keeps it in the payload instead, and these three make the one field
    // answer to its ruby name -- without them a dumped Pathname carried no
    // ivars at all and loaded back EMPTY, which looked like it had worked.
    fn ivar_pairs(&self) -> Vec<(String, RubyValue)> {
        match self.has_path.load(Ordering::Acquire) {
            true => vec![("@path".to_string(), str_val(self.path.lock().clone()))],
            false => Vec::new(),
        }
    }
    fn ivar_get_named(&self, name: &str) -> Option<RubyValue> {
        match name == "path" && self.has_path.load(Ordering::Acquire) {
            true => Some(str_val(self.path.lock().clone())),
            false => None,
        }
    }
    fn ivar_set_named(&self, name: &str, v: RubyValue) -> bool {
        match (name, &v) {
            ("path", RubyValue::Str(s)) => {
                *self.path.lock() = s.lock().to_utf8_lossy().into_owned();
                self.has_path.store(true, Ordering::Release);
                true
            }
            _ => false,
        }
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let d = Arc::new(RPathname {
            path: Mutex::new(self.path.lock().clone()),
            has_path: AtomicBool::new(self.has_path.load(Ordering::Acquire)),
            frozen: AtomicBool::new(false),
        });
        if copy_frozen && self.is_frozen() {
            d.set_frozen();
        }
        d
    }
}

fn str_val(s: String) -> RubyValue {
    RubyValue::Str(crate::string_new(s))
}

/// Wrap a path string as a fresh `Pathname` value.
fn pathname_val(path: String) -> RubyValue {
    RubyValue::Object(Arc::new(RPathname {
        path: Mutex::new(path),
        has_path: AtomicBool::new(true),
        frozen: AtomicBool::new(false),
    }))
}

fn recv_path(recv: &RubyValue) -> String {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RPathname>()
            .expect("Pathname row on a non-Pathname receiver")
            .path
            .lock()
            .clone(),
        _ => panic!("Pathname row on a non-Object receiver"),
    }
}

/// The `RPathname` behind a value -- through a SUBCLASS's payload as well as
/// directly, so `Pathname.new("/usr") == MyPath.new("/usr")` holds the way it
/// does in ruby. A `class MyPath < Pathname` instance is a `ValueSubclass`
/// husk whose payload is the real Pathname, and a bare downcast sees only the
/// husk.
fn as_pathname(v: &RubyValue) -> Option<PathnameRef<'_>> {
    match v {
        RubyValue::Object(o) => match o.as_any().downcast_ref::<RPathname>() {
            Some(p) => Some(PathnameRef::Direct(p)),
            None => match o.builtin_payload() {
                Some(RubyValue::Object(inner))
                    if inner.as_any().downcast_ref::<RPathname>().is_some() =>
                {
                    Some(PathnameRef::Payload(inner))
                }
                _ => None,
            },
        },
        _ => None,
    }
}

/// Either borrow of an `RPathname`: one held directly by the value, or one
/// owned by a subclass husk's payload (which must be kept alive to borrow).
enum PathnameRef<'a> {
    Direct(&'a RPathname),
    Payload(RObj),
}

impl std::ops::Deref for PathnameRef<'_> {
    type Target = RPathname;
    fn deref(&self) -> &RPathname {
        match self {
            PathnameRef::Direct(p) => p,
            PathnameRef::Payload(o) => o
                .as_any()
                .downcast_ref::<RPathname>()
                .expect("checked on construction"),
        }
    }
}

/// The path string of an argument that is itself a Pathname or a String
/// (`p + other`, `p == other`) -- Pathname's operators accept both.
fn arg_path(v: &RubyValue) -> Result<String, Signal> {
    if let Some(p) = as_pathname(v) {
        return Ok(p.path.lock().clone());
    }
    path_arg(v, "pathname")
}

// ------------------------------------------------------------ path algebra

/// CRuby's `chop_basename`: the path split into everything BEFORE the last
/// component (its trailing separator included) and the component itself.
/// `None` once nothing but separators is left, which is what ends every peel
/// loop below.
fn chop_basename(path: &str) -> Option<(&str, &str)> {
    let b = path.as_bytes();
    let end = b.iter().rposition(|c| *c != b'/')? + 1;
    let start = b[..end]
        .iter()
        .rposition(|c| *c == b'/')
        .map_or(0, |i| i + 1);
    Some((&path[..start], &path[start..end]))
}

fn is_absolute(path: &str) -> bool {
    path.starts_with('/')
}

/// Every component of `path`, in order, separators dropped.
fn split_names(path: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut pre = path;
    while let Some((p, base)) = chop_basename(pre) {
        names.insert(0, base);
        pre = p;
    }
    names
}

/// `path` without its trailing separators; a path of nothing but separators
/// keeps one.
fn del_trailing_separator(path: &str) -> &str {
    match chop_basename(path) {
        Some((pre, base)) => &path[..pre.len() + base.len()],
        None if path.is_empty() => path,
        None => "/",
    }
}

/// `File.basename`, the slice of it these helpers need: the last component,
/// `"/"` for a path of nothing but separators, `""` for an empty path.
fn file_basename(path: &str) -> &str {
    match chop_basename(path) {
        Some((_, base)) => base,
        None if path.is_empty() => "",
        None => "/",
    }
}

/// `File.dirname`: everything before the last component, trailing separators
/// dropped; `"."` when nothing is left, `"/"` for the root itself.
fn file_dirname(path: &str) -> String {
    match chop_basename(path) {
        Some((pre, _)) => {
            let d = del_trailing_separator(pre);
            if d.is_empty() {
                ".".to_string()
            } else {
                d.to_string()
            }
        }
        None if path.is_empty() => ".".to_string(),
        None => "/".to_string(),
    }
}

/// CRuby's `has_trailing_separator?`: the last component is followed by
/// separators. False for the root, which is ALL separator.
fn has_trailing_separator(path: &str) -> bool {
    match chop_basename(path) {
        Some((pre, base)) => pre.len() + base.len() < path.len(),
        None => false,
    }
}

/// CRuby's `add_trailing_separator`: ensure one, except on a path that
/// already behaves as if it had one (empty, or separator-tailed).
fn add_trailing_separator(path: &str) -> String {
    if path.is_empty() || path.ends_with('/') {
        path.to_string()
    } else {
        format!("{path}/")
    }
}

/// CRuby's `prepend_prefix`: glue a peeled prefix back onto a relative
/// remainder. An empty remainder names the prefix's own directory.
fn prepend_prefix(prefix: &str, relpath: &str) -> String {
    if relpath.is_empty() {
        file_dirname(prefix)
    } else if prefix.contains('/') {
        let mut prefix = file_dirname(prefix);
        if !prefix.ends_with('/') {
            prefix.push('/');
        }
        format!("{prefix}{relpath}")
    } else {
        format!("{prefix}{relpath}")
    }
}

/// Joins components onto a prefix that is either empty or the root. An empty
/// result is `"."`, the path that names no movement.
fn assemble(absolute: bool, names: &[&str]) -> String {
    let mut out = if absolute {
        "/".to_string()
    } else {
        String::new()
    };
    for (i, n) in names.iter().enumerate() {
        if i > 0 {
            out.push('/');
        }
        out.push_str(n);
    }
    if out.is_empty() {
        out.push('.');
    }
    out
}

/// `#cleanpath`: `..` cancels the component before it, `.` vanishes, and a
/// leading `..` is dropped from an ABSOLUTE path -- there is nothing above
/// the root.
fn cleanpath_aggressive(path: &str) -> String {
    let mut names: Vec<&str> = Vec::new();
    let mut pre = path;
    while let Some((p, base)) = chop_basename(pre) {
        pre = p;
        match base {
            "." => {}
            ".." => names.insert(0, base),
            _ => {
                if names.first() == Some(&"..") {
                    names.remove(0);
                } else {
                    names.insert(0, base);
                }
            }
        }
    }
    let absolute = !pre.is_empty();
    if absolute {
        while names.first() == Some(&"..") {
            names.remove(0);
        }
    }
    assemble(absolute, &names)
}

/// `#cleanpath(true)`: only `.` and repeated separators go, because a `..`
/// that follows a symlink does NOT name the directory above it. Ported step
/// for step from `pathname_builtin.rb` -- a `.` LAST component survives
/// (`"a/."` stays `"a/."`), and a component-less path answers its prefix's
/// dirname.
fn cleanpath_conservative(path: &str) -> String {
    let mut names: Vec<&str> = Vec::new();
    let mut pre = path;
    while let Some((p, base)) = chop_basename(pre) {
        pre = p;
        if base != "." {
            names.insert(0, base);
        }
    }
    if !pre.is_empty() {
        while names.first() == Some(&"..") {
            names.remove(0);
        }
    }
    if names.is_empty() {
        return file_dirname(pre);
    }
    if names.last() != Some(&"..") && file_basename(path) == "." {
        names.push(".");
    }
    let result = prepend_prefix(pre, &names.join("/"));
    if !matches!(*names.last().expect("non-empty"), "." | "..") && has_trailing_separator(path) {
        add_trailing_separator(&result)
    } else {
        result
    }
}

/// `Pathname#+`: CRuby's `plus`, which resolves `.` and `..` in the RIGHT
/// operand against the components of the left WITHOUT touching the
/// filesystem. Ported step for step -- the interleaving of the two peels is
/// what makes `"a/b/c" + "../../d"` answer `"a/d"`.
fn plus(path1: &str, path2: &str) -> String {
    let mut prefix2 = path2;
    let mut index_list2: Vec<usize> = Vec::new();
    let mut basename_list2: Vec<&str> = Vec::new();
    while let Some((p, b)) = chop_basename(prefix2) {
        index_list2.insert(0, p.len());
        basename_list2.insert(0, b);
        prefix2 = p;
    }
    // A right operand with a root of its own simply replaces the left.
    if !prefix2.is_empty() {
        return path2.to_string();
    }
    let mut n1 = path1.len();
    loop {
        while basename_list2.first() == Some(&".") {
            index_list2.remove(0);
            basename_list2.remove(0);
        }
        let Some((p, b1)) = chop_basename(&path1[..n1]) else {
            break;
        };
        let peeled = p.len();
        if b1 == "." {
            n1 = peeled;
            continue;
        }
        // A `..` on the right cancels the component just peeled off the
        // left; anything else means the left is done shrinking.
        if b1 == ".." || basename_list2.first() != Some(&"..") {
            n1 = peeled + b1.len();
            break;
        }
        n1 = peeled;
        index_list2.remove(0);
        basename_list2.remove(0);
    }
    let prefix1 = &path1[..n1];
    let rooted = chop_basename(prefix1).is_none() && !prefix1.is_empty();
    if rooted {
        // Nothing is above the root, so a leading `..` on the right dies.
        while basename_list2.first() == Some(&"..") {
            index_list2.remove(0);
            basename_list2.remove(0);
        }
    }
    let r1 = chop_basename(prefix1).is_some() || rooted;
    if !basename_list2.is_empty() {
        let suffix2 = &path2[index_list2[0]..];
        if r1 {
            join_two(prefix1, suffix2)
        } else {
            format!("{prefix1}{suffix2}")
        }
    } else if r1 {
        prefix1.to_string()
    } else {
        ".".to_string()
    }
}

/// `File.join` for two pieces: exactly one separator between them.
fn join_two(a: &str, b: &str) -> String {
    if a.is_empty() || a.ends_with('/') || b.starts_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// `#relative_path_from`: the path leading from `base` to `self`, walked
/// entirely in the two names. Both sides are cleaned first, so the answer
/// never depends on what is actually on disk.
fn relative_path_from(dest: &str, base: &str) -> Result<String, Signal> {
    let dest_clean = cleanpath_aggressive(dest);
    let base_clean = cleanpath_aggressive(base);
    let dest_prefix = if is_absolute(&dest_clean) { "/" } else { "" };
    let base_prefix = if is_absolute(&base_clean) { "/" } else { "" };
    if dest_prefix != base_prefix {
        return Err(arg_error!("different prefix: {dest_prefix:?} and {base:?}"));
    }
    let mut dest_names: Vec<&str> = split_names(&dest_clean)
        .into_iter()
        .filter(|n| *n != ".")
        .collect();
    let mut base_names: Vec<&str> = split_names(&base_clean)
        .into_iter()
        .filter(|n| *n != ".")
        .collect();
    while !dest_names.is_empty() && !base_names.is_empty() && dest_names[0] == base_names[0] {
        dest_names.remove(0);
        base_names.remove(0);
    }
    if base_names.contains(&"..") {
        return Err(arg_error!("base_directory has ..: {base:?}"));
    }
    let mut names: Vec<&str> = base_names.iter().map(|_| "..").collect();
    names.extend(dest_names);
    Ok(assemble(false, &names))
}

/// Every ancestor of `path`, self first -- what `#ascend` walks.
fn ascend_paths(path: &str) -> Vec<String> {
    let mut out = vec![path.to_string()];
    let mut cur = path;
    while let Some((pre, _)) = chop_basename(cur) {
        if pre.is_empty() {
            break;
        }
        out.push(del_trailing_separator(pre).to_string());
        cur = &path[..pre.len()];
    }
    out
}

// ------------------------------------------------------------- delegation

fn file_call(
    meth: &str,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // File's own table first, then IO's -- `open`/`new`/`for_fd`/`sysopen` are
    // IO's rows, which File inherits, exactly as CRuby has them.
    let f = crate::builtins::file::lookup_class(meth)
        .or_else(|| crate::builtins::io::lookup_class(meth))
        .expect("a File or IO class method");
    f(&RubyValue::Class(zeo_abi::FILE_CLASS), args, block)
}

fn dir_call(meth: &str, args: &[RubyValue], block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let f = crate::builtins::dir::lookup_class(meth).expect("a Dir class method");
    f(&RubyValue::Class(zeo_abi::DIR_CLASS), args, block)
}

/// `File.<meth>(self, *rest)`.
fn on_file(
    recv: &RubyValue,
    meth: &str,
    rest: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let mut args = vec![str_val(recv_path(recv).to_string())];
    args.extend_from_slice(rest);
    file_call(meth, &args, block)
}

/// `Dir.<meth>(self, *rest)`.
fn on_dir(
    recv: &RubyValue,
    meth: &str,
    rest: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let mut args = vec![str_val(recv_path(recv).to_string())];
    args.extend_from_slice(rest);
    dir_call(meth, &args, block)
}

/// The same, wrapping a String answer back into a `Pathname`.
fn on_file_path(recv: &RubyValue, meth: &str, rest: &[RubyValue]) -> Result<RubyValue, Signal> {
    Ok(wrap(on_file(recv, meth, rest, None)?))
}

/// A String answer as a `Pathname`; anything else passes through.
fn wrap(v: RubyValue) -> RubyValue {
    match v {
        RubyValue::Str(s) => pathname_val(s.lock().to_utf8_lossy().into_owned()),
        other => other,
    }
}

fn wrap_array(v: RubyValue) -> RubyValue {
    let RubyValue::Array(a) = &v else { return v };
    let items: Vec<RubyValue> = a.lock().iter().cloned().map(wrap).collect();
    RubyValue::Array(crate::array_new(items))
}

/// An optional argument as a slice, so a delegation forwards only what was
/// actually given rather than a trailing nil.
fn opt(v: Option<&RubyValue>) -> Vec<RubyValue> {
    v.filter(|x| !matches!(x, RubyValue::Nil))
        .into_iter()
        .cloned()
        .collect()
}

/// `#children` / `#each_child`: the directory's entries minus the two dots,
/// each prefixed with the receiver unless `with_directory` is false.
fn child_paths(
    recv: &RubyValue,
    with_directory: Option<&RubyValue>,
) -> Result<Vec<RubyValue>, Signal> {
    let base = recv_path(recv);
    let bare = matches!(with_directory, Some(v) if !v.truthy());
    let names = dir_call("children", &[str_val(base.to_string())], None)?;
    let RubyValue::Array(a) = &names else {
        return Ok(Vec::new());
    };
    let items: Vec<RubyValue> = a.lock().iter().cloned().collect();
    items
        .into_iter()
        .map(|n| {
            let name = arg_path(&n)?;
            Ok(if bare {
                pathname_val(name)
            } else {
                pathname_val(plus(&base, &name))
            })
        })
        .collect()
}

/// Every path under `root`, root first -- `#find`'s walk. A symlinked
/// directory is not descended into, matching `Find`.
fn collect_tree(root: &str, out: &mut Vec<RubyValue>) -> Result<(), Signal> {
    out.push(pathname_val(root.to_string()));
    let arg = str_val(root.to_string());
    if !file_call("directory?", std::slice::from_ref(&arg), None)?.truthy()
        || file_call("symlink?", std::slice::from_ref(&arg), None)?.truthy()
    {
        return Ok(());
    }
    let names = dir_call("children", std::slice::from_ref(&arg), None)?;
    let RubyValue::Array(a) = &names else {
        return Ok(());
    };
    let items: Vec<RubyValue> = a.lock().iter().cloned().collect();
    for n in items {
        collect_tree(&plus(root, &arg_path(&n)?), out)?;
    }
    Ok(())
}

/// `#rmtree`: depth-first, so a directory is empty by the time it goes.
fn remove_tree(root: &str) -> Result<(), Signal> {
    let mut all = Vec::new();
    collect_tree(root, &mut all)?;
    for item in all.iter().rev() {
        let arg = str_val(recv_path(item).to_string());
        let is_dir = file_call("directory?", std::slice::from_ref(&arg), None)?.truthy()
            && !file_call("symlink?", std::slice::from_ref(&arg), None)?.truthy();
        if is_dir {
            dir_call("rmdir", std::slice::from_ref(&arg), None)?;
        } else {
            file_call("delete", std::slice::from_ref(&arg), None)?;
        }
    }
    Ok(())
}

/// Yields each of an Array's items to `block` and answers nil, or answers the
/// Array itself when there is no block -- the shape both `#glob` forms take.
fn yield_or_return(all: RubyValue, block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let Some(RubyValue::Proc(proc)) = block else {
        return Ok(all);
    };
    let RubyValue::Array(a) = &all else {
        return Ok(RubyValue::Nil);
    };
    let items: Vec<RubyValue> = a.lock().iter().cloned().collect();
    for item in items {
        proc.call(&[item])?;
    }
    Ok(RubyValue::Nil)
}

/// `Pathname::SEPARATOR_PAT` -- ruby's own `/#{Regexp.quote(File::SEPARATOR)}/`.
///
/// A `const` row rather than a bootstrap seeder because a seeder runs in
/// EVERY program: this compiled a regex before line 1 of `puts 1`, which put
/// the whole regex engine in every binary.
fn separator_pat() -> RubyValue {
    let pat =
        crate::regexp::regexp_new("/", false, false, false).expect("`/` is a valid regexp source");
    RubyValue::Regexp(pat)
}

/// `Kernel#Pathname(str)` -- the private conversion function, present with no
/// require for the same reason the class is.
pub fn kernel_pathname(arg: &RubyValue) -> Result<RubyValue, Signal> {
    pathname_construct(PATHNAME_CLASS, std::slice::from_ref(arg), None)
}

/// The String/Pathname/#to_path acceptance (and null-byte refusal) shared by
/// `Pathname.new` and the private `#initialize` row.
fn construct_path(arg: &RubyValue) -> Result<String, Signal> {
    let path = match as_pathname(arg) {
        Some(p) => p.path.lock().clone(),
        None => path_arg(arg, "pathname")
            .map_err(|_| type_error!("Pathname.new requires a String, #to_path or #to_str"))?,
    };
    if path.contains('\0') {
        return Err(arg_error!("path name contains null byte"));
    }
    Ok(path)
}

/// `Pathname.new` is a CONSTRUCTOR, not a class-method row: CRuby inherits it
/// from `Class`, so it must not show up in `singleton_methods(false)`.
fn pathname_construct(
    _class: ClassId,
    args: &[RubyValue],
    _block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::check_arity(args.len(), 1, Some(1))?;
    let path = construct_path(&args[0])?;
    Ok(pathname_val(path))
}

/// A blank `Pathname` -- an empty path, which is what CRuby's own
/// `Pathname.allocate` answers (`#<Pathname:>`, no ivars). The private
/// `#initialize` row re-seeds the path in place, so an allocated value is a
/// legal receiver for it.
fn pathname_allocate() -> RubyValue {
    let blank = pathname_val(String::new());
    if let RubyValue::Object(o) = &blank
        && let Some(p) = o.as_any().downcast_ref::<RPathname>()
    {
        p.has_path.store(false, Ordering::Release);
    }
    blank
}

pub fn register_pathname(registry: &mut crate::dispatch::ClassRegistry) {
    registry.register(
        PATHNAME_CLASS,
        "Pathname",
        false,
        zeo_abi::declared_ancestors(PATHNAME_CLASS),
        Some(pathname_construct as crate::dispatch::ConstructorFn),
    );
}

ruby_class! {
    Pathname = zeo_abi::PATHNAME_CLASS < zeo_abi::OBJECT_CLASS;

    allocate pathname_allocate;

    const VERSION = str_val("0.4.0".to_string());
    const SEPARATOR_PAT = separator_pat();

    def self."getwd" | "pwd"(_recv) {
        Ok(wrap(dir_call("pwd", &[], None)?))
    }
    def self."glob" params "*args, **kwargs" (_recv, pattern, *rest, &block) {
        let mut args = vec![pattern.clone()];
        args.extend_from_slice(rest);
        yield_or_return(wrap_array(dir_call("glob", &args, None)?), block)
    }
    // pathname.rb's own, not tmpdir.rb's: the ruby half is what adds it, and
    // its body requires tmpdir itself.
    def self."mktmpdir" params "" gated "pathname" (_recv, *args, &block) {
        Ok(wrap(dir_call("mktmpdir", args, block)?))
    }

    // ---- the path itself
    def "to_s" | "to_path"(recv) { Ok(str_val(recv_path(recv).to_string())) }
    // PROTECTED in CRuby, so it is listed by `instance_methods` and still
    // refuses an outside caller.
    protected def "path"(recv) { Ok(str_val(recv_path(recv).to_string())) }
    def "inspect"(recv) { Ok(str_val(format!("#<Pathname:{}>", recv_path(recv)))) }
    def "freeze"(recv) {
        if let RubyValue::Object(o) = recv { o.set_frozen(); }
        Ok(recv.clone())
    }
    // Re-init replaces the stored path in place, accepting the same
    // String/Pathname/#to_path set (and null-byte refusal) as `Pathname.new`.
    private ruby def "initialize"(recv, path) {
        if let RubyValue::Object(o) = recv
            && o.is_frozen()
        {
            return Err(frozen_error!(
                "can't modify frozen Pathname: #<Pathname:{}>",
                recv_path(recv)
            ));
        }
        let new_path = construct_path(path)?;
        if let Some(p) = as_pathname(recv) {
            *p.path.lock() = new_path;
            // The assignment ruby's own `initialize` makes, which is what
            // brings `@path` into existence.
            p.has_path.store(true, Ordering::Release);
        }
        Ok(recv.clone())
    }
    def "hash"(recv) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        recv_path(recv).hash(&mut h);
        Ok(RubyValue::Int(h.finish() as i64))
    }
    // Only another Pathname compares equal -- a String never does.
    ruby def "==" | "===" | "eql?"(recv, other) {
        Ok(RubyValue::Bool(
            as_pathname(other).is_some_and(|p| *p.path.lock() == recv_path(recv)),
        ))
    }
    def "<=>"(recv, other) {
        Ok(match as_pathname(other) {
            Some(p) => RubyValue::Int(match recv_path(recv).cmp(&p.path.lock()) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }),
            None => RubyValue::Nil,
        })
    }

    // ---- path algebra
    ruby def "+" | "/"(recv, other) {
        Ok(pathname_val(plus(&recv_path(recv), &arg_path(other)?)))
    }
    // Right to left, stopping at the first absolute piece: everything to its
    // left is unreachable, which is what makes `join("b", "/c")` answer `/c`.
    ruby def "join"(recv, *args) {
        let mut result: Option<String> = None;
        for arg in args.iter().rev() {
            let piece = arg_path(arg)?;
            result = Some(match result {
                None => piece,
                Some(acc) if is_absolute(&acc) => acc,
                Some(acc) => plus(&piece, &acc),
            });
            if is_absolute(result.as_deref().expect("just set")) {
                return Ok(pathname_val(result.expect("just set")));
            }
        }
        Ok(match result {
            None => recv.clone(),
            Some(rel) => pathname_val(plus(&recv_path(recv), &rel)),
        })
    }
    def "parent"(recv) { Ok(pathname_val(plus(&recv_path(recv), ".."))) }
    ruby def "cleanpath"(recv, consider_symlink?) {
        let p = recv_path(recv);
        Ok(pathname_val(match consider_symlink {
            Some(v) if v.truthy() => cleanpath_conservative(&p),
            _ => cleanpath_aggressive(&p),
        }))
    }
    def "relative_path_from" params "base_directory" (recv, base) {
        Ok(pathname_val(relative_path_from(&recv_path(recv), &arg_path(base)?)?))
    }

    // ---- ruby's private path-algebra helpers (`pathname_builtin.rb`), as
    // real rows so `private_instance_methods` and a dynamic `send` agree
    // with CRuby. Each wraps the Rust fn the public rows already share.
    private ruby def "chop_basename"(_recv, path) {
        let p = arg_path(path)?;
        Ok(match chop_basename(&p) {
            Some((pre, base)) => RubyValue::Array(crate::array_new(vec![
                str_val(pre.to_string()),
                str_val(base.to_string()),
            ])),
            None => RubyValue::Nil,
        })
    }
    // `[prefix, names]` -- the un-peelable head plus every component after it.
    private ruby def "split_names"(_recv, path) {
        let p = arg_path(path)?;
        let mut pre = p.as_str();
        let mut names: Vec<RubyValue> = Vec::new();
        while let Some((q, base)) = chop_basename(pre) {
            names.insert(0, str_val(base.to_string()));
            pre = q;
        }
        Ok(RubyValue::Array(crate::array_new(vec![
            str_val(pre.to_string()),
            RubyValue::Array(crate::array_new(names)),
        ])))
    }
    private ruby def "prepend_prefix"(_recv, prefix, relpath) {
        Ok(str_val(prepend_prefix(&arg_path(prefix)?, &arg_path(relpath)?)))
    }
    private ruby def "has_trailing_separator?"(_recv, path) {
        Ok(RubyValue::Bool(has_trailing_separator(&arg_path(path)?)))
    }
    private ruby def "add_trailing_separator"(_recv, path) {
        Ok(str_val(add_trailing_separator(&arg_path(path)?)))
    }
    private ruby def "del_trailing_separator"(_recv, path) {
        Ok(str_val(del_trailing_separator(&arg_path(path)?).to_string()))
    }
    // Plain equality: ruby 4.0 compares verbatim even on macOS (oracle-pinned).
    private ruby def "same_paths?"(_recv, a, b) {
        Ok(RubyValue::Bool(arg_path(a)? == arg_path(b)?))
    }
    private ruby def "plus"(_recv, path1, path2) {
        Ok(str_val(plus(&arg_path(path1)?, &arg_path(path2)?)))
    }
    private def "cleanpath_aggressive"(recv) {
        Ok(pathname_val(cleanpath_aggressive(&recv_path(recv))))
    }
    private def "cleanpath_conservative"(recv) {
        Ok(pathname_val(cleanpath_conservative(&recv_path(recv))))
    }
    ruby def "sub_ext"(recv, repl) {
        let newext = arg_path(repl)?;
        let path = recv_path(recv);
        let cur = extname_of(&path);
        let stem = &path[..path.len() - cur.len()];
        Ok(pathname_val(format!("{stem}{newext}")))
    }
    def "absolute?"(recv) { Ok(RubyValue::Bool(is_absolute(&recv_path(recv)))) }
    def "relative?"(recv) { Ok(RubyValue::Bool(!is_absolute(&recv_path(recv)))) }
    def "root?"(recv) {
        let p = recv_path(recv);
        Ok(RubyValue::Bool(chop_basename(&p).is_none() && is_absolute(&p)))
    }
    def "each_filename"(recv, &block) {
        let names: Vec<RubyValue> = split_names(&recv_path(recv))
            .into_iter()
            .map(|n| str_val(n.to_string()))
            .collect();
        let proc = block_or_enum!(recv, &[], block);
        for n in names {
            proc.call(&[n])?;
        }
        Ok(RubyValue::Nil)
    }
    def "ascend"(recv, &block) {
        let steps: Vec<RubyValue> = ascend_paths(&recv_path(recv))
            .into_iter()
            .map(pathname_val)
            .collect();
        let proc = block_or_enum!(recv, &[], block);
        for s in steps { proc.call(&[s])?; }
        Ok(RubyValue::Nil)
    }
    def "descend"(recv, &block) {
        let mut steps: Vec<RubyValue> = ascend_paths(&recv_path(recv))
            .into_iter()
            .map(pathname_val)
            .collect();
        steps.reverse();
        let proc = block_or_enum!(recv, &[], block);
        for s in steps { proc.call(&[s])?; }
        Ok(RubyValue::Nil)
    }

    // ---- names, through the File rows that already spell them
    def "basename" params "*, **, &" (recv, suffix?) { on_file_path(recv, "basename", &opt(suffix)) }
    def "dirname"(recv) { on_file_path(recv, "dirname", &[]) }
    def "extname"(recv) { on_file(recv, "extname", &[], None) }
    def "expand_path" params "*, **, &" (recv, base?) { on_file_path(recv, "expand_path", &opt(base)) }
    def "split"(recv) { Ok(wrap_array(on_file(recv, "split", &[], None)?)) }
    def "sub" cfunc (recv, *args, &block) {
        let replaced = crate::dispatch::send_value(
            &str_val(recv_path(recv).to_string()),
            crate::Symbol::intern("sub"),
            args,
            block,
        )?;
        Ok(wrap(replaced))
    }
    // `File.fnmatch` takes the PATTERN first, so the receiver goes second.
    def "fnmatch" params "pattern, *, **, &" | "fnmatch?" params "pattern, *, **, &"(recv, pattern, flags?) {
        let mut args = vec![pattern.clone(), str_val(recv_path(recv).to_string())];
        args.extend(opt(flags));
        file_call("fnmatch", &args, None)
    }

    // ---- what is on disk: each of these IS the File/Dir row
    def "atime"(recv) { on_file(recv, "atime", &[], None) }
    def "birthtime"(recv) { on_file(recv, "birthtime", &[], None) }
    def "ctime"(recv) { on_file(recv, "ctime", &[], None) }
    def "mtime"(recv) { on_file(recv, "mtime", &[], None) }
    def "ftype"(recv) { on_file(recv, "ftype", &[], None) }
    def "size"(recv) { on_file(recv, "size", &[], None) }
    def "size?"(recv) { on_file(recv, "size?", &[], None) }
    def "stat"(recv) { on_file(recv, "stat", &[], None) }
    def "lstat"(recv) { on_file(recv, "lstat", &[], None) }
    def "blockdev?"(recv) { on_file(recv, "blockdev?", &[], None) }
    def "chardev?"(recv) { on_file(recv, "chardev?", &[], None) }
    def "directory?"(recv) { on_file(recv, "directory?", &[], None) }
    def "executable?"(recv) { on_file(recv, "executable?", &[], None) }
    def "executable_real?"(recv) { on_file(recv, "executable_real?", &[], None) }
    def "exist?"(recv) { on_file(recv, "exist?", &[], None) }
    def "file?"(recv) { on_file(recv, "file?", &[], None) }
    def "grpowned?"(recv) { on_file(recv, "grpowned?", &[], None) }
    def "owned?"(recv) { on_file(recv, "owned?", &[], None) }
    def "pipe?"(recv) { on_file(recv, "pipe?", &[], None) }
    def "readable?"(recv) { on_file(recv, "readable?", &[], None) }
    def "readable_real?"(recv) { on_file(recv, "readable_real?", &[], None) }
    def "setgid?"(recv) { on_file(recv, "setgid?", &[], None) }
    def "setuid?"(recv) { on_file(recv, "setuid?", &[], None) }
    def "socket?"(recv) { on_file(recv, "socket?", &[], None) }
    def "sticky?"(recv) { on_file(recv, "sticky?", &[], None) }
    def "symlink?"(recv) { on_file(recv, "symlink?", &[], None) }
    def "world_readable?"(recv) { on_file(recv, "world_readable?", &[], None) }
    def "world_writable?"(recv) { on_file(recv, "world_writable?", &[], None) }
    def "writable?"(recv) { on_file(recv, "writable?", &[], None) }
    def "writable_real?"(recv) { on_file(recv, "writable_real?", &[], None) }
    def "zero?"(recv) { on_file(recv, "zero?", &[], None) }
    // A directory is empty when it holds nothing but the two dots; anything
    // else is empty when it has no bytes.
    def "empty?"(recv) {
        if on_file(recv, "directory?", &[], None)?.truthy() {
            return on_dir(recv, "empty?", &[], None);
        }
        on_file(recv, "zero?", &[], None)
    }
    // A mount point's parent lives on another device -- or IS this path,
    // which is what the root answers.
    def "mountpoint?"(recv) {
        let here = on_file(recv, "lstat", &[], None)?;
        let up = file_call("lstat", &[str_val(plus(&recv_path(recv), ".."))], None)?;
        let read = |v: &RubyValue, m: &str| {
            crate::dispatch::send_value(v, crate::Symbol::intern(m), &[], None)
        };
        let same_dev = read(&here, "dev")?.rb_eq(&read(&up, "dev")?);
        let same_ino = read(&here, "ino")?.rb_eq(&read(&up, "ino")?);
        Ok(RubyValue::Bool((same_dev && same_ino) || !same_dev))
    }

    def "read" params "*, **, &" (recv, *rest) { on_file(recv, "read", rest, None) }
    def "binread" params "*, **, &" (recv, *rest) { on_file(recv, "binread", rest, None) }
    def "readlines" params "*, **, &" (recv, *rest) { on_file(recv, "readlines", rest, None) }
    def "write" params "*, **, &" (recv, *rest) { on_file(recv, "write", rest, None) }
    def "binwrite" params "*, **, &" (recv, *rest) { on_file(recv, "binwrite", rest, None) }
    def "open" params "*, **, &" (recv, *rest, &block) { on_file(recv, "open", rest, block) }
    def "each_line" params "*, **, &" (recv, *rest, &block) { on_file(recv, "foreach", rest, block) }
    def "sysopen" params "*, **, &" (recv, *rest) { on_file(recv, "sysopen", rest, None) }
    def "truncate" params "length" (recv, len) { on_file(recv, "truncate", std::slice::from_ref(len), None) }
    def "unlink" | "delete"(recv) { on_file(recv, "delete", &[], None) }
    def "readlink"(recv) { on_file_path(recv, "readlink", &[]) }
    def "realpath" params "*, **, &" (recv, base?) { on_file_path(recv, "realpath", &opt(base)) }
    def "realdirpath" params "*, **, &" (recv, base?) { on_file_path(recv, "realdirpath", &opt(base)) }
    // These take the path LAST, so they cannot go through `on_file`.
    ruby def "chmod"(recv, mode) {
        file_call("chmod", &[mode.clone(), str_val(recv_path(recv).to_string())], None)
    }
    ruby def "lchmod"(recv, mode) {
        file_call("lchmod", &[mode.clone(), str_val(recv_path(recv).to_string())], None)
    }
    ruby def "chown"(recv, owner, group) {
        file_call(
            "chown",
            &[owner.clone(), group.clone(), str_val(recv_path(recv).to_string())],
            None,
        )
    }
    ruby def "lchown"(recv, owner, group) {
        file_call(
            "lchown",
            &[owner.clone(), group.clone(), str_val(recv_path(recv).to_string())],
            None,
        )
    }
    ruby def "utime"(recv, atime, mtime) {
        file_call(
            "utime",
            &[atime.clone(), mtime.clone(), str_val(recv_path(recv).to_string())],
            None,
        )
    }
    ruby def "lutime"(recv, atime, mtime) {
        file_call(
            "lutime",
            &[atime.clone(), mtime.clone(), str_val(recv_path(recv).to_string())],
            None,
        )
    }
    ruby def "rename"(recv, to) {
        file_call(
            "rename",
            &[str_val(recv_path(recv).to_string()), str_val(arg_path(to)?)],
            None,
        )
    }
    // `old` is the EXISTING file; the receiver is the new name.
    ruby def "make_link"(recv, old) {
        file_call("link", &[str_val(arg_path(old)?), str_val(recv_path(recv).to_string())], None)
    }
    ruby def "make_symlink"(recv, old) {
        file_call("symlink", &[str_val(arg_path(old)?), str_val(recv_path(recv).to_string())], None)
    }

    def "mkdir" params "*, **, &" (recv, *rest) { on_dir(recv, "mkdir", rest, None) }
    def "rmdir"(recv) { on_dir(recv, "rmdir", &[], None) }
    ruby def "opendir"(recv, &block) { on_dir(recv, "open", &[], block) }
    def "entries"(recv) { Ok(wrap_array(on_dir(recv, "entries", &[], None)?)) }
    ruby def "each_entry"(recv, &block) {
        let names = wrap_array(on_dir(recv, "entries", &[], None)?);
        let RubyValue::Array(items) = &names else { return Ok(RubyValue::Nil) };
        let items: Vec<RubyValue> = items.lock().iter().cloned().collect();
        let proc = block_or_enum!(recv, &[], block);
        for n in items { proc.call(&[n])?; }
        Ok(RubyValue::Nil)
    }
    // `with_directory` false answers bare names; the default prefixes each
    // with the receiver, which is what makes the answers usable as paths.
    ruby def "children"(recv, with_directory?) {
        Ok(RubyValue::Array(crate::array_new(child_paths(recv, with_directory)?)))
    }
    def "each_child" params "with_directory = nil, &b" (recv, with_directory?, &block) {
        let kids = child_paths(recv, with_directory)?;
        let proc = block_or_enum!(recv, &[], block);
        for k in kids { proc.call(&[k])?; }
        Ok(recv.clone())
    }
    def "glob" params "*args, **kwargs" (recv, pattern, *rest, &block) {
        let joined = str_val(plus(&recv_path(recv), &arg_path(pattern)?));
        let mut args = vec![joined];
        args.extend_from_slice(rest);
        yield_or_return(wrap_array(dir_call("glob", &args, None)?), block)
    }
    // Every missing directory on the way, root-most first.
    def "mkpath" params "mode: nil" (recv, *_rest) {
        let mut steps = ascend_paths(&cleanpath_aggressive(&recv_path(recv)));
        steps.reverse();
        for step in steps {
            let arg = str_val(step);
            if file_call("directory?", std::slice::from_ref(&arg), None)?.truthy() {
                continue;
            }
            dir_call("mkdir", std::slice::from_ref(&arg), None)?;
        }
        Ok(RubyValue::Nil)
    }
    // `require "pathname"` is what adds these two in ruby; zeo gates whole
    // classes rather than methods, so they are here from the start.
    def "rmtree" params "noop: nil, verbose: nil, secure: nil" gated "pathname" (recv, *_rest) {
        remove_tree(&recv_path(recv))?;
        Ok(RubyValue::Nil)
    }
    def "find" params "ignore_error: true" gated "pathname" (recv, *_rest, &block) {
        let proc = block_or_enum!(recv, &[], block);
        let mut found = Vec::new();
        collect_tree(&recv_path(recv), &mut found)?;
        for f in found { proc.call(&[f])?; }
        Ok(RubyValue::Nil)
    }
}

#[cfg(test)]
mod tests {
    use super::pathname_val;
    use crate::builtins::{MethodTable, ParamRows};
    use crate::method_meta::ParamKind;
    use crate::{RubyValue, string_new};
    use zeo_abi::PATHNAME_CLASS;

    /// Every `Pathname` row's `#parameters` and `#arity`, recorded from ruby
    /// 4.0.6 with `pathname` and `tmpdir` loaded. `#` is an instance row
    /// (private ones included), `.` a class row.
    ///
    /// CRuby writes Pathname in Ruby, so it reports real parameter NAMES where
    /// most builtin rows report none. That is why these rows carry a `ruby def`
    /// or a `params "..."` spelling and most of the runtime's do not.
    const ORACLE: &[(&str, &str, i64)] = &[
        ("#+", "[[:req, :other]]", 1),
        ("#/", "[[:req, :other]]", 1),
        ("#<=>", "[[:req]]", 1),
        ("#==", "[[:req, :other]]", 1),
        ("#===", "[[:req, :other]]", 1),
        ("#absolute?", "[]", 0),
        ("#add_trailing_separator", "[[:req, :path]]", 1),
        ("#ascend", "[]", 0),
        ("#atime", "[]", 0),
        (
            "#basename",
            "[[:rest, :*], [:keyrest, :**], [:block, :&]]",
            -1,
        ),
        (
            "#binread",
            "[[:rest, :*], [:keyrest, :**], [:block, :&]]",
            -1,
        ),
        (
            "#binwrite",
            "[[:rest, :*], [:keyrest, :**], [:block, :&]]",
            -1,
        ),
        ("#birthtime", "[]", 0),
        ("#blockdev?", "[]", 0),
        ("#chardev?", "[]", 0),
        ("#children", "[[:opt, :with_directory]]", -1),
        ("#chmod", "[[:req, :mode]]", 1),
        ("#chop_basename", "[[:req, :path]]", 1),
        ("#chown", "[[:req, :owner], [:req, :group]]", 2),
        ("#cleanpath", "[[:opt, :consider_symlink]]", -1),
        ("#cleanpath_aggressive", "[]", 0),
        ("#cleanpath_conservative", "[]", 0),
        ("#ctime", "[]", 0),
        ("#del_trailing_separator", "[[:req, :path]]", 1),
        ("#delete", "[]", 0),
        ("#descend", "[]", 0),
        ("#directory?", "[]", 0),
        ("#dirname", "[]", 0),
        ("#each_child", "[[:opt, :with_directory], [:block, :b]]", -1),
        ("#each_entry", "[[:block, :block]]", 0),
        ("#each_filename", "[]", 0),
        (
            "#each_line",
            "[[:rest, :*], [:keyrest, :**], [:block, :&]]",
            -1,
        ),
        ("#empty?", "[]", 0),
        ("#entries", "[]", 0),
        ("#eql?", "[[:req, :other]]", 1),
        ("#executable?", "[]", 0),
        ("#executable_real?", "[]", 0),
        ("#exist?", "[]", 0),
        (
            "#expand_path",
            "[[:rest, :*], [:keyrest, :**], [:block, :&]]",
            -1,
        ),
        ("#extname", "[]", 0),
        ("#file?", "[]", 0),
        ("#find", "[[:key, :ignore_error]]", -1),
        (
            "#fnmatch",
            "[[:req, :pattern], [:rest, :*], [:keyrest, :**], [:block, :&]]",
            -2,
        ),
        (
            "#fnmatch?",
            "[[:req, :pattern], [:rest, :*], [:keyrest, :**], [:block, :&]]",
            -2,
        ),
        ("#freeze", "[]", 0),
        ("#ftype", "[]", 0),
        ("#glob", "[[:rest, :args], [:keyrest, :kwargs]]", -1),
        ("#grpowned?", "[]", 0),
        ("#has_trailing_separator?", "[[:req, :path]]", 1),
        ("#hash", "[]", 0),
        ("#initialize", "[[:req, :path]]", 1),
        ("#inspect", "[]", 0),
        ("#join", "[[:rest, :args]]", -1),
        ("#lchmod", "[[:req, :mode]]", 1),
        ("#lchown", "[[:req, :owner], [:req, :group]]", 2),
        ("#lstat", "[]", 0),
        ("#lutime", "[[:req, :atime], [:req, :mtime]]", 2),
        ("#make_link", "[[:req, :old]]", 1),
        ("#make_symlink", "[[:req, :old]]", 1),
        ("#mkdir", "[[:rest, :*], [:keyrest, :**], [:block, :&]]", -1),
        ("#mkpath", "[[:key, :mode]]", -1),
        ("#mountpoint?", "[]", 0),
        ("#mtime", "[]", 0),
        ("#open", "[[:rest, :*], [:keyrest, :**], [:block, :&]]", -1),
        ("#opendir", "[[:block, :block]]", 0),
        ("#owned?", "[]", 0),
        ("#parent", "[]", 0),
        ("#path", "[]", 0),
        ("#pipe?", "[]", 0),
        ("#plus", "[[:req, :path1], [:req, :path2]]", 2),
        ("#prepend_prefix", "[[:req, :prefix], [:req, :relpath]]", 2),
        ("#read", "[[:rest, :*], [:keyrest, :**], [:block, :&]]", -1),
        ("#readable?", "[]", 0),
        ("#readable_real?", "[]", 0),
        (
            "#readlines",
            "[[:rest, :*], [:keyrest, :**], [:block, :&]]",
            -1,
        ),
        ("#readlink", "[]", 0),
        (
            "#realdirpath",
            "[[:rest, :*], [:keyrest, :**], [:block, :&]]",
            -1,
        ),
        (
            "#realpath",
            "[[:rest, :*], [:keyrest, :**], [:block, :&]]",
            -1,
        ),
        ("#relative?", "[]", 0),
        ("#relative_path_from", "[[:req, :base_directory]]", 1),
        ("#rename", "[[:req, :to]]", 1),
        ("#rmdir", "[]", 0),
        (
            "#rmtree",
            "[[:key, :noop], [:key, :verbose], [:key, :secure]]",
            -1,
        ),
        ("#root?", "[]", 0),
        ("#same_paths?", "[[:req, :a], [:req, :b]]", 2),
        ("#setgid?", "[]", 0),
        ("#setuid?", "[]", 0),
        ("#size", "[]", 0),
        ("#size?", "[]", 0),
        ("#socket?", "[]", 0),
        ("#split", "[]", 0),
        ("#split_names", "[[:req, :path]]", 1),
        ("#stat", "[]", 0),
        ("#sticky?", "[]", 0),
        ("#sub", "[[:rest]]", -1),
        ("#sub_ext", "[[:req, :repl]]", 1),
        ("#symlink?", "[]", 0),
        (
            "#sysopen",
            "[[:rest, :*], [:keyrest, :**], [:block, :&]]",
            -1,
        ),
        ("#to_path", "[]", 0),
        ("#to_s", "[]", 0),
        ("#truncate", "[[:req, :length]]", 1),
        ("#unlink", "[]", 0),
        ("#utime", "[[:req, :atime], [:req, :mtime]]", 2),
        ("#world_readable?", "[]", 0),
        ("#world_writable?", "[]", 0),
        ("#writable?", "[]", 0),
        ("#writable_real?", "[]", 0),
        ("#write", "[[:rest, :*], [:keyrest, :**], [:block, :&]]", -1),
        ("#zero?", "[]", 0),
        (".getwd", "[]", 0),
        (".glob", "[[:rest, :args], [:keyrest, :kwargs]]", -1),
        (".mktmpdir", "[]", 0),
        (".pwd", "[]", 0),
    ];

    /// Render a row's signature the way `Method#parameters` prints it, so a
    /// failure reads as ruby's own answer rather than a Rust debug dump.
    fn render(rows: Option<ParamRows>, arity: i64) -> String {
        // No spelling: the anonymous descriptor the arity implies, which is
        // what `method_meta` hands reflection for such a row.
        let Some(rows) = rows else {
            let (required, variadic) = if arity < 0 {
                ((-arity - 1) as usize, true)
            } else {
                (arity as usize, false)
            };
            let mut out: Vec<&str> = vec!["[:req]"; required];
            if variadic {
                out.push("[:rest]");
            }
            return format!("[{}]", out.join(", "));
        };
        let body: Vec<String> = rows
            .iter()
            .map(|(kind, name)| {
                let k = match kind {
                    ParamKind::Req => "req",
                    ParamKind::Opt => "opt",
                    ParamKind::Rest => "rest",
                    ParamKind::KeyReq => "keyreq",
                    ParamKind::Key => "key",
                    ParamKind::KeyRest => "keyrest",
                    ParamKind::Block => "block",
                };
                match name {
                    Some(n) => format!("[:{k}, :{n}]"),
                    None => format!("[:{k}]"),
                }
            })
            .collect();
        format!("[{}]", body.join(", "))
    }

    fn table(side: char) -> &'static MethodTable {
        let t = crate::builtins::registered_table(PATHNAME_CLASS)
            .expect("Pathname is a registered builtin table");
        match side {
            '#' => t.instance.as_ref().expect("Pathname has instance rows"),
            _ => t.class.as_ref().expect("Pathname has class rows"),
        }
    }

    /// The whole surface at once. A per-row assert stops at the first failure
    /// and hides how far a drift goes, so this collects every mismatch.
    #[test]
    fn every_pathname_row_reports_rubys_own_signature() {
        let mut bad: Vec<String> = Vec::new();
        for (label, want_params, want_arity) in ORACLE {
            let (side, name) = label.split_at(1);
            let t = table(side.chars().next().expect("a one-char side"));
            let Some(got_arity) = (t.arity)(name) else {
                bad.push(format!("{label}: no row"));
                continue;
            };
            let got_params = render((t.params)(name), got_arity);
            if got_params != *want_params || got_arity != *want_arity {
                bad.push(format!(
                    "{label}\n       ruby: {want_params} / {want_arity}\n       zeo : {got_params} / {got_arity}"
                ));
            }
        }
        assert!(
            bad.is_empty(),
            "{} rows diverge from ruby:\n     {}",
            bad.len(),
            bad.join("\n     ")
        );
    }

    /// Coverage is part of the claim: a row added to the DSL with no recorded
    /// answer would pass the loop above by never being asked about.
    #[test]
    fn the_oracle_answers_every_row_pathname_declares() {
        let mut missing: Vec<String> = Vec::new();
        for (side, names) in [('#', (table('#').names)()), ('.', (table('.').names)())] {
            for n in names {
                let label = format!("{side}{n}");
                if !ORACLE.iter().any(|(l, _, _)| *l == label) {
                    missing.push(label);
                }
            }
        }
        assert!(
            missing.is_empty(),
            "no recorded ruby answer for: {missing:?}"
        );
    }

    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        ((table('#')).lookup)(name).unwrap_or_else(|| panic!("Pathname#{name} is defined"))
    }

    fn path(s: &str) -> RubyValue {
        pathname_val(s.to_string())
    }

    fn str(s: &str) -> RubyValue {
        RubyValue::Str(string_new(s.to_string()))
    }

    fn call(name: &str, recv: &RubyValue, args: &[RubyValue]) -> String {
        imethod(name)(recv, args, None)
            .unwrap_or_else(|_| panic!("Pathname#{name} answers"))
            .inspect_string()
    }

    /// The path ALGEBRA -- the half written here rather than delegated to
    /// `File`/`Dir`. Every expectation is ruby 4.0.6's own answer.
    #[test]
    fn the_path_algebra_matches_the_oracle() {
        assert_eq!(call("to_s", &path("/a/b"), &[]), "\"/a/b\"");
        assert_eq!(call("inspect", &path("/a"), &[]), "\"#<Pathname:/a>\"");
        assert_eq!(call("+", &path("/a"), &[path("b")]), "#<Pathname:/a/b>");
        assert_eq!(call("+", &path("/a"), &[path("/c")]), "#<Pathname:/c>");
        assert_eq!(call("+", &path("a"), &[path("..")]), "#<Pathname:.>");
        assert_eq!(call("/", &path("/a"), &[str("b")]), "#<Pathname:/a/b>");
        assert_eq!(call("parent", &path("/a/b"), &[]), "#<Pathname:/a>");
        assert_eq!(call("cleanpath", &path("a/../b"), &[]), "#<Pathname:b>");
        assert_eq!(call("cleanpath", &path("/a/./b/"), &[]), "#<Pathname:/a/b>");
        assert_eq!(
            call("sub_ext", &path("a.rb"), &[str(".txt")]),
            "#<Pathname:a.txt>"
        );
        assert_eq!(
            call("join", &path("/a"), &[path("b"), path("c")]),
            "#<Pathname:/a/b/c>"
        );
        assert_eq!(
            call("join", &path("/a"), &[path("b"), path("/c")]),
            "#<Pathname:/c>"
        );
        assert_eq!(
            call("relative_path_from", &path("/a/b/c"), &[path("/a")]),
            "#<Pathname:b/c>"
        );
        assert_eq!(call("absolute?", &path("/a"), &[]), "true");
        assert_eq!(call("relative?", &path("a"), &[]), "true");
        assert_eq!(call("root?", &path("/"), &[]), "true");
        assert_eq!(call("root?", &path("/a"), &[]), "false");
    }

    /// A Pathname equals only another Pathname -- never the String that spells
    /// the same path.
    #[test]
    fn equality_refuses_a_string() {
        assert_eq!(call("==", &path("/a"), &[path("/a")]), "true");
        assert_eq!(call("==", &path("/a"), &[path("/b")]), "false");
        assert_eq!(call("==", &path("/a"), &[str("/a")]), "false");
        assert_eq!(call("eql?", &path("/a"), &[str("/a")]), "false");
        assert_eq!(call("<=>", &path("/a"), &[path("/b")]), "-1");
        assert_eq!(call("<=>", &path("/a"), &[RubyValue::Int(1)]), "nil");
    }
}

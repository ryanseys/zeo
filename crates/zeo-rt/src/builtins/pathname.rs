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

use crate::builtins::file::{extname_of, path_arg};
use crate::builtins::{arg_error, block_or_enum, type_error};
use crate::dispatch::{RObj, RubyObject};
use crate::{RubyValue, Signal};
use zeo_abi::{ClassId, PATHNAME_CLASS};
use zeo_macros::ruby_class;

/// A `Pathname` instance: an immutable path string. No `Mutex` -- Pathname is
/// a value class (every mutating-looking method returns a fresh Pathname).
pub(crate) struct RPathname {
    path: String,
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
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let d = Arc::new(RPathname {
            path: self.path.clone(),
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
        path,
        frozen: AtomicBool::new(false),
    }))
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

fn as_pathname(v: &RubyValue) -> Option<&RPathname> {
    match v {
        RubyValue::Object(o) => o.as_any().downcast_ref::<RPathname>(),
        _ => None,
    }
}

/// The path string of an argument that is itself a Pathname or a String
/// (`p + other`, `p == other`) -- Pathname's operators accept both.
fn arg_path(v: &RubyValue) -> Result<String, Signal> {
    if let Some(p) = as_pathname(v) {
        return Ok(p.path.clone());
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
    let f = crate::builtins::file::lookup_class(meth).expect("a File class method");
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
                pathname_val(plus(base, &name))
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

/// `Pathname::VERSION` and `SEPARATOR_PAT` -- seeded from generated `main()`.
pub fn seed_pathname_constants() {
    crate::const_set(PATHNAME_CLASS.0, "VERSION", str_val("0.4.0".to_string()));
    if let Ok(pat) = crate::regexp::regexp_new("/", false, false, false) {
        crate::const_set(PATHNAME_CLASS.0, "SEPARATOR_PAT", RubyValue::Regexp(pat));
    }
}

/// `Kernel#Pathname(str)` -- the private conversion function, present with no
/// require for the same reason the class is.
pub fn kernel_pathname(arg: &RubyValue) -> Result<RubyValue, Signal> {
    pathname_construct(PATHNAME_CLASS, std::slice::from_ref(arg), None)
}

/// `Pathname.new` is a CONSTRUCTOR, not a class-method row: CRuby inherits it
/// from `Class`, so it must not show up in `singleton_methods(false)`.
fn pathname_construct(
    _class: ClassId,
    args: &[RubyValue],
    _block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    crate::builtins::check_arity(args.len(), 1, Some(1))?;
    let path = match as_pathname(&args[0]) {
        Some(p) => p.path.clone(),
        None => path_arg(&args[0], "pathname")
            .map_err(|_| type_error!("Pathname.new requires a String, #to_path or #to_str"))?,
    };
    if path.contains('\0') {
        return Err(arg_error!("path name contains null byte"));
    }
    Ok(pathname_val(path))
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

    def self."getwd" | "pwd"(_recv) {
        Ok(wrap(dir_call("pwd", &[], None)?))
    }
    def self."glob" cfunc (_recv, pattern, *rest, &block) {
        let mut args = vec![pattern.clone()];
        args.extend_from_slice(rest);
        yield_or_return(wrap_array(dir_call("glob", &args, None)?), block)
    }
    def self."mktmpdir" cfunc (_recv, *args, &block) {
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
    def "hash"(recv) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        recv_path(recv).hash(&mut h);
        Ok(RubyValue::Int(h.finish() as i64))
    }
    // Only another Pathname compares equal -- a String never does.
    def "==" | "===" | "eql?"(recv, other) {
        Ok(RubyValue::Bool(
            as_pathname(other).is_some_and(|p| p.path == recv_path(recv)),
        ))
    }
    def "<=>"(recv, other) {
        Ok(match as_pathname(other) {
            Some(p) => RubyValue::Int(match recv_path(recv).cmp(p.path.as_str()) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }),
            None => RubyValue::Nil,
        })
    }

    // ---- path algebra
    def "+" | "/"(recv, other) {
        Ok(pathname_val(plus(recv_path(recv), &arg_path(other)?)))
    }
    // Right to left, stopping at the first absolute piece: everything to its
    // left is unreachable, which is what makes `join("b", "/c")` answer `/c`.
    def "join" cfunc (recv, *args) {
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
            Some(rel) => pathname_val(plus(recv_path(recv), &rel)),
        })
    }
    def "parent"(recv) { Ok(pathname_val(plus(recv_path(recv), ".."))) }
    def "cleanpath"(recv, consider_symlink?) {
        let p = recv_path(recv);
        Ok(pathname_val(match consider_symlink {
            Some(v) if v.truthy() => cleanpath_conservative(p),
            _ => cleanpath_aggressive(p),
        }))
    }
    def "relative_path_from"(recv, base) {
        Ok(pathname_val(relative_path_from(recv_path(recv), &arg_path(base)?)?))
    }

    // ---- ruby's private path-algebra helpers (`pathname_builtin.rb`), as
    // real rows so `private_instance_methods` and a dynamic `send` agree
    // with CRuby. Each wraps the Rust fn the public rows already share.
    private def "chop_basename"(_recv, path) {
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
    private def "split_names"(_recv, path) {
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
    private def "prepend_prefix"(_recv, prefix, relpath) {
        Ok(str_val(prepend_prefix(&arg_path(prefix)?, &arg_path(relpath)?)))
    }
    private def "has_trailing_separator?"(_recv, path) {
        Ok(RubyValue::Bool(has_trailing_separator(&arg_path(path)?)))
    }
    private def "add_trailing_separator"(_recv, path) {
        Ok(str_val(add_trailing_separator(&arg_path(path)?)))
    }
    private def "del_trailing_separator"(_recv, path) {
        Ok(str_val(del_trailing_separator(&arg_path(path)?).to_string()))
    }
    // Plain equality: ruby 4.0 compares verbatim even on macOS (oracle-pinned).
    private def "same_paths?"(_recv, a, b) {
        Ok(RubyValue::Bool(arg_path(a)? == arg_path(b)?))
    }
    private def "plus"(_recv, path1, path2) {
        Ok(str_val(plus(&arg_path(path1)?, &arg_path(path2)?)))
    }
    private def "cleanpath_aggressive"(recv) {
        Ok(pathname_val(cleanpath_aggressive(recv_path(recv))))
    }
    private def "cleanpath_conservative"(recv) {
        Ok(pathname_val(cleanpath_conservative(recv_path(recv))))
    }
    def "sub_ext"(recv, repl) {
        let newext = arg_path(repl)?;
        let path = recv_path(recv);
        let cur = extname_of(path);
        let stem = &path[..path.len() - cur.len()];
        Ok(pathname_val(format!("{stem}{newext}")))
    }
    def "absolute?"(recv) { Ok(RubyValue::Bool(is_absolute(recv_path(recv)))) }
    def "relative?"(recv) { Ok(RubyValue::Bool(!is_absolute(recv_path(recv)))) }
    def "root?"(recv) {
        let p = recv_path(recv);
        Ok(RubyValue::Bool(chop_basename(p).is_none() && is_absolute(p)))
    }
    def "each_filename"(recv, &block) {
        let names: Vec<RubyValue> = split_names(recv_path(recv))
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
        let steps: Vec<RubyValue> = ascend_paths(recv_path(recv))
            .into_iter()
            .map(pathname_val)
            .collect();
        let proc = block_or_enum!(recv, &[], block);
        for s in steps { proc.call(&[s])?; }
        Ok(RubyValue::Nil)
    }
    def "descend"(recv, &block) {
        let mut steps: Vec<RubyValue> = ascend_paths(recv_path(recv))
            .into_iter()
            .map(pathname_val)
            .collect();
        steps.reverse();
        let proc = block_or_enum!(recv, &[], block);
        for s in steps { proc.call(&[s])?; }
        Ok(RubyValue::Nil)
    }

    // ---- names, through the File rows that already spell them
    def "basename"(recv, suffix?) { on_file_path(recv, "basename", &opt(suffix)) }
    def "dirname"(recv) { on_file_path(recv, "dirname", &[]) }
    def "extname"(recv) { on_file(recv, "extname", &[], None) }
    def "expand_path"(recv, base?) { on_file_path(recv, "expand_path", &opt(base)) }
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
    def "fnmatch" | "fnmatch?"(recv, pattern, flags?) {
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
        let up = file_call("lstat", &[str_val(plus(recv_path(recv), ".."))], None)?;
        let read = |v: &RubyValue, m: &str| {
            crate::dispatch::send_value(v, crate::Symbol::intern(m), &[], None)
        };
        let same_dev = read(&here, "dev")?.rb_eq(&read(&up, "dev")?);
        let same_ino = read(&here, "ino")?.rb_eq(&read(&up, "ino")?);
        Ok(RubyValue::Bool((same_dev && same_ino) || !same_dev))
    }

    def "read" cfunc (recv, *rest) { on_file(recv, "read", rest, None) }
    def "binread" cfunc (recv, *rest) { on_file(recv, "binread", rest, None) }
    def "readlines" cfunc (recv, *rest) { on_file(recv, "readlines", rest, None) }
    def "write" cfunc (recv, *rest) { on_file(recv, "write", rest, None) }
    def "binwrite" cfunc (recv, *rest) { on_file(recv, "binwrite", rest, None) }
    def "open" cfunc (recv, *rest, &block) { on_file(recv, "open", rest, block) }
    def "each_line" cfunc (recv, *rest, &block) { on_file(recv, "foreach", rest, block) }
    def "sysopen" cfunc (recv, *rest) { on_file(recv, "sysopen", rest, None) }
    def "truncate"(recv, len) { on_file(recv, "truncate", std::slice::from_ref(len), None) }
    def "unlink" | "delete"(recv) { on_file(recv, "delete", &[], None) }
    def "readlink"(recv) { on_file_path(recv, "readlink", &[]) }
    def "realpath"(recv, base?) { on_file_path(recv, "realpath", &opt(base)) }
    def "realdirpath"(recv, base?) { on_file_path(recv, "realdirpath", &opt(base)) }
    // These take the path LAST, so they cannot go through `on_file`.
    def "chmod"(recv, mode) {
        file_call("chmod", &[mode.clone(), str_val(recv_path(recv).to_string())], None)
    }
    def "lchmod"(recv, mode) {
        file_call("lchmod", &[mode.clone(), str_val(recv_path(recv).to_string())], None)
    }
    def "chown"(recv, owner, group) {
        file_call(
            "chown",
            &[owner.clone(), group.clone(), str_val(recv_path(recv).to_string())],
            None,
        )
    }
    def "lchown"(recv, owner, group) {
        file_call(
            "lchown",
            &[owner.clone(), group.clone(), str_val(recv_path(recv).to_string())],
            None,
        )
    }
    def "utime"(recv, atime, mtime) {
        file_call(
            "utime",
            &[atime.clone(), mtime.clone(), str_val(recv_path(recv).to_string())],
            None,
        )
    }
    def "lutime"(recv, atime, mtime) {
        file_call(
            "lutime",
            &[atime.clone(), mtime.clone(), str_val(recv_path(recv).to_string())],
            None,
        )
    }
    def "rename"(recv, to) {
        file_call(
            "rename",
            &[str_val(recv_path(recv).to_string()), str_val(arg_path(to)?)],
            None,
        )
    }
    // `old` is the EXISTING file; the receiver is the new name.
    def "make_link"(recv, old) {
        file_call("link", &[str_val(arg_path(old)?), str_val(recv_path(recv).to_string())], None)
    }
    def "make_symlink"(recv, old) {
        file_call("symlink", &[str_val(arg_path(old)?), str_val(recv_path(recv).to_string())], None)
    }

    def "mkdir" cfunc (recv, *rest) { on_dir(recv, "mkdir", rest, None) }
    def "rmdir"(recv) { on_dir(recv, "rmdir", &[], None) }
    def "opendir"(recv, &block) { on_dir(recv, "open", &[], block) }
    def "entries"(recv) { Ok(wrap_array(on_dir(recv, "entries", &[], None)?)) }
    def "each_entry"(recv, &block) {
        let names = wrap_array(on_dir(recv, "entries", &[], None)?);
        let RubyValue::Array(items) = &names else { return Ok(RubyValue::Nil) };
        let items: Vec<RubyValue> = items.lock().iter().cloned().collect();
        let proc = block_or_enum!(recv, &[], block);
        for n in items { proc.call(&[n])?; }
        Ok(RubyValue::Nil)
    }
    // `with_directory` false answers bare names; the default prefixes each
    // with the receiver, which is what makes the answers usable as paths.
    def "children"(recv, with_directory?) {
        Ok(RubyValue::Array(crate::array_new(child_paths(recv, with_directory)?)))
    }
    def "each_child"(recv, with_directory?, &block) {
        let kids = child_paths(recv, with_directory)?;
        let proc = block_or_enum!(recv, &[], block);
        for k in kids { proc.call(&[k])?; }
        Ok(recv.clone())
    }
    def "glob" cfunc (recv, pattern, *rest, &block) {
        let joined = str_val(plus(recv_path(recv), &arg_path(pattern)?));
        let mut args = vec![joined];
        args.extend_from_slice(rest);
        yield_or_return(wrap_array(dir_call("glob", &args, None)?), block)
    }
    // Every missing directory on the way, root-most first.
    def "mkpath" cfunc (recv, *_rest) {
        let mut steps = ascend_paths(&cleanpath_aggressive(recv_path(recv)));
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
    def "rmtree" cfunc (recv, *_rest) {
        remove_tree(recv_path(recv))?;
        Ok(RubyValue::Nil)
    }
    def "find" cfunc (recv, *_rest, &block) {
        let proc = block_or_enum!(recv, &[], block);
        let mut found = Vec::new();
        collect_tree(recv_path(recv), &mut found)?;
        for f in found { proc.call(&[f])?; }
        Ok(RubyValue::Nil)
    }
}

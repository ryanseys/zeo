//! `ARGF` -- the singleton pseudo-IO that reads, in turn, the files named in
//! `ARGV` (or standard input when `ARGV` is empty). Its class is CRuby's
//! literally-named `"ARGF.class"`; it includes `Enumerable`.
//!
//! The read model is CRuby's: `ARGF` consumes `ARGV` lazily -- each file is
//! opened when it is first reached, and the current file's name is what
//! `#filename` reports (`"-"` for standard input). This slice materializes a
//! file's contents at open time (enough for the line/whole-file readers the
//! corpus uses); a streaming rework is only needed for unbounded inputs.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use crate::Signal;
use crate::builtins::{arity, type_error};
use crate::dispatch::{RObj, RubyObject};
use crate::value::RubyValue;
use zeo_abi::{ARGF_CLASS, ClassId};
use zeo_macros::ruby_class;

/// The singleton `ARGF` reader. Its cursor state is the current file name and
/// line number; the file list itself is read live from the `ARGV` constant
/// (so a program that edits `ARGV` before reading is honored, as in CRuby).
pub struct RArgf {
    /// The file currently being read (`"-"` = standard input), or the empty
    /// string before the first read.
    filename: parking_lot::Mutex<String>,
    lineno: AtomicI64,
    /// Set once reading has begun, so `#filename` reports the current file
    /// rather than peeking `ARGV[0]`.
    started: AtomicBool,
}

impl RubyObject for RArgf {
    fn class_id(&self) -> ClassId {
        ARGF_CLASS
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
        Arc::new(RArgf {
            filename: parking_lot::Mutex::new(self.filename.lock().clone()),
            lineno: AtomicI64::new(self.lineno.load(Ordering::Relaxed)),
            started: AtomicBool::new(self.started.load(Ordering::Relaxed)),
        })
    }
}

/// Install the `ARGF` global constant -- called once from generated `main()`.
pub fn seed_argf() {
    let argf = RubyValue::Object(Arc::new(RArgf {
        filename: parking_lot::Mutex::new(String::new()),
        lineno: AtomicI64::new(0),
        started: AtomicBool::new(false),
    }));
    crate::constants::const_set(0, "ARGF", argf);
}

fn recv_argf(recv: &RubyValue) -> Result<&RArgf, Signal> {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RArgf>()
            .ok_or_else(|| type_error!("not ARGF")),
        _ => Err(type_error!("not ARGF")),
    }
}

/// The file paths named in `ARGV` right now (read live, as CRuby does).
fn argv_files() -> Vec<String> {
    match crate::constants::const_get(0, "ARGV") {
        Some(RubyValue::Array(a)) => a.lock().iter().map(|v| v.to_display_string()).collect(),
        _ => Vec::new(),
    }
}

/// Read the whole of every input (each `ARGV` file in turn, or standard input
/// when `ARGV` is empty), returning the concatenated bytes and recording the
/// last file's name as the current `#filename`.
fn read_all(argf: &RArgf) -> Result<Vec<u8>, Signal> {
    argf.started.store(true, Ordering::Relaxed);
    let files = argv_files();
    if files.is_empty() {
        *argf.filename.lock() = "-".to_string();
        let mut buf = Vec::new();
        // Gvl-released: reading standard input blocks until EOF arrives.
        crate::gvl::without_gvl(|| std::io::Read::read_to_end(&mut std::io::stdin(), &mut buf))
            .map_err(|e| crate::builtins::file::raise_errno(&e, "read", "-"))?;
        return Ok(buf);
    }
    let mut out = Vec::new();
    for f in files {
        let bytes = crate::gvl::without_gvl(|| std::fs::read(&f))
            .map_err(|e| crate::builtins::file::raise_errno(&e, "rb_sysopen", &f))?;
        *argf.filename.lock() = f;
        out.extend(bytes);
    }
    Ok(out)
}

/// The lines of all inputs (keeping terminators), the shared basis of
/// `#each_line`/`#readlines`/`#gets`.
fn all_lines(argf: &RArgf) -> Result<Vec<RubyValue>, Signal> {
    let bytes = read_all(argf)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok(crate::builtins::string::split_lines(&text))
}

ruby_class! {
    Argf = zeo_abi::ARGF_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::ENUMERABLE_CLASS;

    // `#filename`/`#path` -- the current file (`"-"` = stdin). Before reading
    // begins, it is `ARGV[0]` (or `"-"` when `ARGV` is empty).
    def "filename" | "path"(recv, *args, &_blk) {
        arity!(args, 0);
        let argf = recv_argf(recv)?;
        if argf.started.load(Ordering::Relaxed) {
            return Ok(str_val(argf.filename.lock().clone()));
        }
        Ok(str_val(argv_files().into_iter().next().unwrap_or_else(|| "-".to_string())))
    }
    def "each_line" | "each"(recv, *args, &blk) {
        arity!(args, 0..=1);
        let Some(RubyValue::Proc(p)) = blk else {
            return Err(crate::dispatch::raise_no_block_yield());
        };
        let argf = recv_argf(recv)?;
        for line in all_lines(argf)? {
            argf.lineno.fetch_add(1, Ordering::Relaxed);
            p.call(&[line])?;
        }
        Ok(recv.clone())
    }
    def "readlines" | "to_a"(recv, *args, &_blk) {
        arity!(args, 0..=1);
        let lines = all_lines(recv_argf(recv)?)?;
        Ok(RubyValue::Array(crate::collections::array_new(lines)))
    }
    def "read"(recv, *args, &_blk) {
        arity!(args, 0..=1);
        let bytes = read_all(recv_argf(recv)?)?;
        Ok(str_val(String::from_utf8_lossy(&bytes).into_owned()))
    }
    def "lineno"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_argf(recv)?.lineno.load(Ordering::Relaxed)))
    }
    def "to_s" | "inspect"(recv, *args, &_blk) {
        arity!(args, 0);
        let _ = recv_argf(recv)?;
        Ok(str_val("ARGF".to_string()))
    }
}

fn str_val(s: String) -> RubyValue {
    RubyValue::Str(crate::collections::string_new(s))
}

#[cfg(test)]
mod tests {
    /// `ARGF.class`'s `ruby_class!` table self-registers via linkme; this pins
    /// that its instance surface (including the pipe-aliases) resolves through
    /// the registry, the path real dispatch uses now the fn names are mangled.
    #[test]
    fn the_table_resolves_the_argf_surface() {
        let tbl = crate::builtins::registered_table(zeo_abi::ARGF_CLASS)
            .expect("ARGF.class is a registered builtin table")
            .instance
            .as_ref()
            .expect("ARGF.class has instance methods");
        assert!((tbl.lookup)("each").is_some());
        assert!((tbl.lookup)("each_line").is_some()); // aliased with each
        assert!((tbl.lookup)("path").is_some()); // aliased with filename
        assert!((tbl.lookup)("read").is_some());
        assert!((tbl.lookup)("nope").is_none());
    }
}

//! `File::Stat` -- the immutable snapshot `File.stat`/`File.lstat`/`File#stat`
//! answer. It wraps a raw `libc::stat` captured at construction time, so every
//! reader (`size`/`mtime`/`uid`/...) and predicate (`file?`/`directory?`/...)
//! is a pure field read with no second trip to the disk -- which is exactly
//! CRuby's contract (a `Stat` reflects the moment it was taken, not the file
//! as it is now).
//!
//! `File::Stat` includes `Comparable` (ordered by `mtime`), so `<`/`between?`/
//! `clamp` fall out of the `comparable` method table once `<=>` exists.

use std::sync::Arc;

use crate::Signal;
use crate::builtins::{arg_error, arity, type_error};
use crate::dispatch::{RObj, RubyObject};
use crate::value::RubyValue;
use zeo_abi::{ClassId, FILE_STAT_CLASS};
use zeo_macros::ruby_class;

/// A captured `stat(2)` result. `libc::stat` is `Copy` and self-contained, so
/// the whole object is a plain value behind the `Arc` every `RObj` needs.
pub struct RStat {
    st: libc::stat,
}

impl RubyObject for RStat {
    fn class_id(&self) -> ClassId {
        FILE_STAT_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    // A `Stat` is a value snapshot; freezing is meaningless (CRuby's is
    // unfrozen too) and a copy is just the same fields.
    fn is_frozen(&self) -> bool {
        false
    }
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(RStat { st: self.st })
    }
}

/// Wrap a `libc::stat` as a `File::Stat` value.
fn stat_value(st: libc::stat) -> RubyValue {
    RubyValue::Object(Arc::new(RStat { st }))
}

/// `File.stat(path)` (follows a final symlink) / `File.lstat(path)` (doesn't).
pub fn stat_from_path(path: &str, follow: bool) -> Result<RubyValue, Signal> {
    let c = std::ffi::CString::new(path).map_err(|_| arg_error!("string contains null byte"))?;
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path; `st` is a live `stat` buffer.
    let rc = unsafe {
        if follow {
            libc::stat(c.as_ptr(), &mut st)
        } else {
            libc::lstat(c.as_ptr(), &mut st)
        }
    };
    if rc != 0 {
        let syscall = if follow { "stat" } else { "lstat" };
        return Err(crate::builtins::file::raise_errno(
            &std::io::Error::last_os_error(),
            syscall,
            path,
        ));
    }
    Ok(stat_value(st))
}

/// `File#stat` -- a `fstat(2)` on the open descriptor.
pub fn stat_from_fd(fd: i32) -> Result<RubyValue, Signal> {
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `st` is a live `stat` buffer; a bad fd just yields an error rc.
    let rc = unsafe { libc::fstat(fd, &mut st) };
    if rc != 0 {
        return Err(crate::builtins::file::raise_errno(
            &std::io::Error::last_os_error(),
            "fstat",
            "",
        ));
    }
    Ok(stat_value(st))
}

fn recv_stat(recv: &RubyValue) -> Result<&RStat, Signal> {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RStat>()
            .ok_or_else(|| type_error!("not a File::Stat")),
        _ => Err(type_error!("not a File::Stat")),
    }
}

/// The `S_IFMT`-masked type bits -- the discriminant every `ftype`/predicate
/// reads.
fn fmt(st: &libc::stat) -> libc::mode_t {
    st.st_mode & libc::S_IFMT
}

/// Whether `mode` carries every bit of `mask` (the set-uid/gid/sticky checks).
fn mode_has(st: &libc::stat, mask: libc::mode_t) -> bool {
    (st.st_mode & mask) == mask
}

/// `mtime`/`atime`/`ctime` build a `Time` from the matching `st_*` seconds +
/// nanoseconds pair.
fn stat_time(secs: i64, nsec: i64) -> RubyValue {
    crate::builtins::time::time_from_parts(secs, nsec as u32)
}

ruby_class! {
    Stat = zeo_abi::FILE_STAT_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::COMPARABLE_CLASS;

    def "size"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_size))
    }
    // `size?` is nil for an empty file (the "is there content" predicate).
    def "size?"(recv, *args, &_blk) {
        arity!(args, 0);
        let n = recv_stat(recv)?.st.st_size;
        Ok(if n > 0 { RubyValue::Int(n) } else { RubyValue::Nil })
    }
    def "zero?"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv_stat(recv)?.st.st_size == 0))
    }
    def "mtime"(recv, *args, &_blk) {
        arity!(args, 0);
        let st = &recv_stat(recv)?.st;
        Ok(stat_time(st.st_mtime, st.st_mtime_nsec))
    }
    def "atime"(recv, *args, &_blk) {
        arity!(args, 0);
        let st = &recv_stat(recv)?.st;
        Ok(stat_time(st.st_atime, st.st_atime_nsec))
    }
    def "ctime"(recv, *args, &_blk) {
        arity!(args, 0);
        let st = &recv_stat(recv)?.st;
        Ok(stat_time(st.st_ctime, st.st_ctime_nsec))
    }
    def "birthtime"(recv, *args, &_blk) {
        arity!(args, 0);
        let st = &recv_stat(recv)?.st;
        Ok(stat_time(st.st_birthtime, st.st_birthtime_nsec))
    }
    def "mode"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_mode as i64))
    }
    def "uid"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_uid as i64))
    }
    def "gid"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_gid as i64))
    }
    def "ino"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_ino as i64))
    }
    def "dev"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_dev as i64))
    }
    def "rdev"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_rdev as i64))
    }
    def "nlink"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_nlink as i64))
    }
    def "blksize"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_blksize as i64))
    }
    def "blocks"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_blocks))
    }
    def "ftype"(recv, *args, &_blk) {
        arity!(args, 0);
        let t = fmt(&recv_stat(recv)?.st);
        let s = match t {
            libc::S_IFREG => "file",
            libc::S_IFDIR => "directory",
            libc::S_IFLNK => "link",
            libc::S_IFIFO => "fifo",
            libc::S_IFSOCK => "socket",
            libc::S_IFCHR => "characterSpecial",
            libc::S_IFBLK => "blockSpecial",
            _ => "unknown",
        };
        Ok(RubyValue::Str(crate::collections::string_new(s.to_string())))
    }
    def "file?"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Bool(fmt(&recv_stat(recv)?.st) == libc::S_IFREG))
    }
    def "directory?"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Bool(fmt(&recv_stat(recv)?.st) == libc::S_IFDIR))
    }
    def "symlink?"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Bool(fmt(&recv_stat(recv)?.st) == libc::S_IFLNK))
    }
    def "pipe?"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Bool(fmt(&recv_stat(recv)?.st) == libc::S_IFIFO))
    }
    def "socket?"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Bool(fmt(&recv_stat(recv)?.st) == libc::S_IFSOCK))
    }
    def "blockdev?"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Bool(fmt(&recv_stat(recv)?.st) == libc::S_IFBLK))
    }
    def "chardev?"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Bool(fmt(&recv_stat(recv)?.st) == libc::S_IFCHR))
    }
    def "setuid?"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Bool(mode_has(&recv_stat(recv)?.st, libc::S_ISUID)))
    }
    def "setgid?"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Bool(mode_has(&recv_stat(recv)?.st, libc::S_ISGID)))
    }
    def "sticky?"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Bool(mode_has(&recv_stat(recv)?.st, libc::S_ISVTX)))
    }
    def "owned?"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv_stat(recv)?.st.st_uid == unsafe { libc::geteuid() }))
    }
    def "grpowned?"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv_stat(recv)?.st.st_gid == unsafe { libc::getegid() }))
    }
    // Access predicates read the mode bits against the effective uid/gid --
    // owner bits when we own it, group bits when we share the group, else
    // other bits (the same rule CRuby's `Stat` uses, distinct from `access(2)`).
    def "readable?"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Bool(access_bits(&recv_stat(recv)?.st, 0o400, 0o040, 0o004)))
    }
    def "writable?"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Bool(access_bits(&recv_stat(recv)?.st, 0o200, 0o020, 0o002)))
    }
    def "executable?"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(RubyValue::Bool(access_bits(&recv_stat(recv)?.st, 0o100, 0o010, 0o001)))
    }
    def "world_readable?"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(world_perm(&recv_stat(recv)?.st, 0o004))
    }
    def "world_writable?"(recv, *args, &_blk) {
        arity!(args, 0);
        Ok(world_perm(&recv_stat(recv)?.st, 0o002))
    }
    // Ordered by mtime -- what `Comparable` drives `<`/`>`/`between?` from.
    def "<=>"(recv, *args, &_blk) {
        arity!(args, 1);
        let a = recv_stat(recv)?.st.st_mtime;
        let RubyValue::Object(o) = &args[0] else { return Ok(RubyValue::Nil) };
        let Some(other) = o.as_any().downcast_ref::<RStat>() else { return Ok(RubyValue::Nil) };
        Ok(RubyValue::Int(a.cmp(&other.st.st_mtime) as i64))
    }
    def "inspect" | "to_s"(recv, *args, &_blk) {
        arity!(args, 0);
        let st = &recv_stat(recv)?.st;
        Ok(RubyValue::Str(crate::collections::string_new(format!(
            "#<File::Stat dev=0x{:x}, ino={}, mode=0{:o}, nlink={}, uid={}, gid={}, size={}>",
            st.st_dev, st.st_ino, st.st_mode, st.st_nlink, st.st_uid, st.st_gid, st.st_size
        ))))
    }
}

/// Whether the effective user may access per the owner/group/other bit masks:
/// owner bits when the euid owns it, group bits when the egid matches, else
/// other bits. (Root -- euid 0 -- always reads/writes, mirroring the kernel.)
fn access_bits(st: &libc::stat, owner: u32, group: u32, other: u32) -> bool {
    let euid = unsafe { libc::geteuid() };
    let egid = unsafe { libc::getegid() };
    let mode = st.st_mode as u32;
    if euid == 0 {
        // Root bypasses read/write checks; execute still needs some x bit.
        return owner == 0o100 || (mode & 0o111) != 0 || (mode & (owner | group | other)) != 0;
    }
    let bit = if st.st_uid == euid {
        owner
    } else if st.st_gid == egid {
        group
    } else {
        other
    };
    (mode & bit) != 0
}

/// `world_readable?`/`world_writable?`: the low permission bits (`mode & 0777`)
/// when others hold the access, else nil -- CRuby returns the mask, not a bool.
fn world_perm(st: &libc::stat, bit: u32) -> RubyValue {
    let mode = st.st_mode as u32;
    if mode & bit != 0 {
        RubyValue::Int((mode & 0o777) as i64)
    } else {
        RubyValue::Nil
    }
}

#[cfg(test)]
mod tests {
    /// `File::Stat`'s `ruby_class!` table self-registers via linkme; this pins
    /// that its instance surface resolves through the registry -- the path real
    /// dispatch uses, since the DSL mangles the fn names.
    #[test]
    fn the_table_resolves_the_stat_surface() {
        let tbl = crate::builtins::registered_table(zeo_abi::FILE_STAT_CLASS)
            .expect("File::Stat is a registered builtin table")
            .instance
            .as_ref()
            .expect("File::Stat has instance methods");
        assert!((tbl.lookup)("size").is_some());
        assert!((tbl.lookup)("directory?").is_some());
        assert!((tbl.lookup)("<=>").is_some());
        assert!((tbl.lookup)("to_s").is_some()); // aliased with inspect
        assert!((tbl.lookup)("nope").is_none());
    }
}

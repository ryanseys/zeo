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
use crate::builtins::{arg_error, type_error};
use crate::dispatch::{RObj, RubyObject};
use crate::value::RubyValue;
use zeo_abi::{ClassId, FILE_STAT_CLASS};
use zeo_macros::ruby_class;

/// A captured `stat(2)` result. `libc::stat` is `Copy` and self-contained, so
/// the whole object is a plain value behind the `Arc` every `RObj` needs.
pub struct RStat {
    st: libc::stat,
    /// Creation time as `(secs, nanos)`. macOS reads it off the stat itself;
    /// Linux captures it separately via `statx(2)` at construction, and a
    /// filesystem without a recorded btime leaves it `None`.
    birth: Option<(i64, i64)>,
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
        Arc::new(RStat {
            st: self.st,
            birth: self.birth,
        })
    }
}

/// Wrap a `libc::stat` as a `File::Stat` value.
fn stat_value(st: libc::stat, birth: Option<(i64, i64)>) -> RubyValue {
    RubyValue::Object(Arc::new(RStat { st, birth }))
}

/// The creation time `stat(2)` itself cannot carry on Linux: `statx(2)` with
/// `STATX_BTIME`, `None` when the filesystem does not record one.
#[cfg(target_os = "linux")]
fn statx_birth(dirfd: libc::c_int, path: &std::ffi::CStr, flags: libc::c_int) -> Option<(i64, i64)> {
    let mut x: libc::statx = unsafe { std::mem::zeroed() };
    // SAFETY: `path` is a valid NUL-terminated string; `x` is a live buffer.
    let rc = unsafe { libc::statx(dirfd, path.as_ptr(), flags, libc::STATX_BTIME, &mut x) };
    if rc == 0 && (x.stx_mask & libc::STATX_BTIME) != 0 {
        Some((x.stx_btime.tv_sec, x.stx_btime.tv_nsec as i64))
    } else {
        None
    }
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
    #[cfg(target_vendor = "apple")]
    let birth = Some((st.st_birthtime, st.st_birthtime_nsec));
    #[cfg(target_os = "linux")]
    let birth = statx_birth(
        libc::AT_FDCWD,
        &c,
        if follow { 0 } else { libc::AT_SYMLINK_NOFOLLOW },
    );
    Ok(stat_value(st, birth))
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
    #[cfg(target_vendor = "apple")]
    let birth = Some((st.st_birthtime, st.st_birthtime_nsec));
    #[cfg(target_os = "linux")]
    let birth = statx_birth(fd, c"", libc::AT_EMPTY_PATH);
    Ok(stat_value(st, birth))
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

    def "size"(recv) {
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_size))
    }
    // `size?` is nil for an empty file (the "is there content" predicate).
    def "size?"(recv) {
        let n = recv_stat(recv)?.st.st_size;
        Ok(if n > 0 { RubyValue::Int(n) } else { RubyValue::Nil })
    }
    def "zero?"(recv) {
        Ok(RubyValue::Bool(recv_stat(recv)?.st.st_size == 0))
    }
    def "mtime"(recv) {
        let st = &recv_stat(recv)?.st;
        Ok(stat_time(st.st_mtime, st.st_mtime_nsec))
    }
    def "atime"(recv) {
        let st = &recv_stat(recv)?.st;
        Ok(stat_time(st.st_atime, st.st_atime_nsec))
    }
    def "ctime"(recv) {
        let st = &recv_stat(recv)?.st;
        Ok(stat_time(st.st_ctime, st.st_ctime_nsec))
    }
    def "birthtime"(recv) {
        match recv_stat(recv)?.birth {
            Some((secs, nsec)) => Ok(stat_time(secs, nsec)),
            None => Err(crate::dispatch::raise_error(
                "NotImplementedError",
                "birthtime() function is unimplemented on this machine".to_string(),
            )),
        }
    }
    def "mode"(recv) {
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_mode as i64))
    }
    def "uid"(recv) {
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_uid as i64))
    }
    def "gid"(recv) {
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_gid as i64))
    }
    def "ino"(recv) {
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_ino as i64))
    }
    def "dev"(recv) {
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_dev as i64))
    }
    def "rdev"(recv) {
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_rdev as i64))
    }
    def "nlink"(recv) {
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_nlink as i64))
    }
    def "blksize"(recv) {
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_blksize as i64))
    }
    def "blocks"(recv) {
        Ok(RubyValue::Int(recv_stat(recv)?.st.st_blocks))
    }
    def "ftype"(recv) {
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
    def "file?"(recv) {
        Ok(RubyValue::Bool(fmt(&recv_stat(recv)?.st) == libc::S_IFREG))
    }
    def "directory?"(recv) {
        Ok(RubyValue::Bool(fmt(&recv_stat(recv)?.st) == libc::S_IFDIR))
    }
    def "symlink?"(recv) {
        Ok(RubyValue::Bool(fmt(&recv_stat(recv)?.st) == libc::S_IFLNK))
    }
    def "pipe?"(recv) {
        Ok(RubyValue::Bool(fmt(&recv_stat(recv)?.st) == libc::S_IFIFO))
    }
    def "socket?"(recv) {
        Ok(RubyValue::Bool(fmt(&recv_stat(recv)?.st) == libc::S_IFSOCK))
    }
    def "blockdev?"(recv) {
        Ok(RubyValue::Bool(fmt(&recv_stat(recv)?.st) == libc::S_IFBLK))
    }
    def "chardev?"(recv) {
        Ok(RubyValue::Bool(fmt(&recv_stat(recv)?.st) == libc::S_IFCHR))
    }
    def "setuid?"(recv) {
        Ok(RubyValue::Bool(mode_has(&recv_stat(recv)?.st, libc::S_ISUID)))
    }
    def "setgid?"(recv) {
        Ok(RubyValue::Bool(mode_has(&recv_stat(recv)?.st, libc::S_ISGID)))
    }
    def "sticky?"(recv) {
        Ok(RubyValue::Bool(mode_has(&recv_stat(recv)?.st, libc::S_ISVTX)))
    }
    def "owned?"(recv) {
        Ok(RubyValue::Bool(recv_stat(recv)?.st.st_uid == unsafe { libc::geteuid() }))
    }
    def "grpowned?"(recv) {
        Ok(RubyValue::Bool(recv_stat(recv)?.st.st_gid == unsafe { libc::getegid() }))
    }
    // Access predicates read the mode bits against the effective uid/gid --
    // owner bits when we own it, group bits when we share the group, else
    // other bits (the same rule CRuby's `Stat` uses, distinct from `access(2)`).
    def "readable?"(recv) {
        Ok(RubyValue::Bool(access_bits(&recv_stat(recv)?.st, 0o400, 0o040, 0o004)))
    }
    def "writable?"(recv) {
        Ok(RubyValue::Bool(access_bits(&recv_stat(recv)?.st, 0o200, 0o020, 0o002)))
    }
    def "executable?"(recv) {
        Ok(RubyValue::Bool(access_bits(&recv_stat(recv)?.st, 0o100, 0o010, 0o001)))
    }
    // The `*_real?` trio asks the same question of the REAL uid/gid rather than
    // the effective one. They differ only under set-uid, which is exactly what
    // they exist to test.
    def "readable_real?"(recv) {
        Ok(RubyValue::Bool(real_access_bits(&recv_stat(recv)?.st, 0o400, 0o040, 0o004)))
    }
    def "writable_real?"(recv) {
        Ok(RubyValue::Bool(real_access_bits(&recv_stat(recv)?.st, 0o200, 0o020, 0o002)))
    }
    def "executable_real?"(recv) {
        Ok(RubyValue::Bool(real_access_bits(&recv_stat(recv)?.st, 0o100, 0o010, 0o001)))
    }
    // `st_dev`/`st_rdev` split into their major and minor halves.
    def "dev_major"(recv) {
        Ok(RubyValue::Int(dev_major(recv_stat(recv)?.st.st_dev)))
    }
    def "dev_minor"(recv) {
        Ok(RubyValue::Int(dev_minor(recv_stat(recv)?.st.st_dev)))
    }
    def "rdev_major"(recv) {
        Ok(RubyValue::Int(dev_major(recv_stat(recv)?.st.st_rdev)))
    }
    def "rdev_minor"(recv) {
        Ok(RubyValue::Int(dev_minor(recv_stat(recv)?.st.st_rdev)))
    }
    def "world_readable?"(recv) {
        Ok(world_perm(&recv_stat(recv)?.st, 0o004))
    }
    def "world_writable?"(recv) {
        Ok(world_perm(&recv_stat(recv)?.st, 0o002))
    }
    // Ordered by mtime -- what `Comparable` drives `<`/`>`/`between?` from.
    def "<=>"(recv, other) {
        let a = recv_stat(recv)?.st.st_mtime;
        let RubyValue::Object(o) = other else { return Ok(RubyValue::Nil) };
        let Some(other) = o.as_any().downcast_ref::<RStat>() else { return Ok(RubyValue::Nil) };
        Ok(RubyValue::Int(a.cmp(&other.st.st_mtime) as i64))
    }
    def "inspect" | "to_s"(recv) {
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
    // SAFETY: neither call takes an argument and neither can fail.
    let (euid, egid) = unsafe { (libc::geteuid(), libc::getegid()) };
    access_bits_for(st, euid, egid, owner, group, other)
}

/// The `*_real?` twin of [`access_bits`]: the same rule asked of the REAL
/// uid/gid, which differ from the effective pair only under set-uid.
fn real_access_bits(st: &libc::stat, owner: u32, group: u32, other: u32) -> bool {
    // SAFETY: neither call takes an argument and neither can fail.
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    access_bits_for(st, uid, gid, owner, group, other)
}

fn access_bits_for(
    st: &libc::stat,
    euid: libc::uid_t,
    egid: libc::gid_t,
    owner: u32,
    group: u32,
    other: u32,
) -> bool {
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

/// The major/minor split of a `dev_t`. The encoding is the platform's, not
/// Ruby's: macOS packs `major:minor` as 8+24 bits, Linux uses glibc's split
/// (12 bits of major around a 20-bit minor).
fn dev_major(dev: libc::dev_t) -> i64 {
    let dev = dev as u64;
    #[cfg(target_vendor = "apple")]
    {
        ((dev >> 24) & 0xff) as i64
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        (((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfffu64)) as i64
    }
}

fn dev_minor(dev: libc::dev_t) -> i64 {
    let dev = dev as u64;
    #[cfg(target_vendor = "apple")]
    {
        (dev & 0x00ff_ffff) as i64
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        ((dev & 0xff) | ((dev >> 12) & !0xffu64)) as i64
    }
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

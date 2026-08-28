//! `Etc` -- CRuby's `ext/etc` module, over `libc`. Access to the system
//! user/group databases (`getpwnam`/`getpwuid`/`getgrnam`/`getgrgid` and the
//! `getpwent`/`getgrent` cursors), `sysconf`/`confstr`, `uname`, `nprocessors`,
//! `getlogin`, and the install-path helpers (`sysconfdir`/`systmpdir`).
//!
//! `require`-gated on `"etc"`. The `Etc` module itself lives in `etc_module.rs`;
//! the user/group lookups return `Etc::Passwd` / `Etc::Group` value objects
//! (`passwd.rs` / `group.rs`) -- real `Struct` subclasses, as CRuby's
//! `rb_struct_define` makes them, so the whole struct protocol is inherited
//! and each file supplies only its members and accessors. The database calls
//! are NOT reentrant (they hand back pointers into a shared static buffer), so
//! every one runs under `PWDB_LOCK`, copying the fields out before releasing
//! it.

mod etc_module;
mod group;
mod passwd;

use std::ffi::{CStr, CString};

use parking_lot::Mutex;

use crate::builtins::arg_error;
use crate::dispatch::raise_error;
use crate::{RubyValue, Signal, Symbol};

/// Serializes access to the non-reentrant `getpw*`/`getgr*` family, whose
/// results alias a shared static buffer. Held only long enough to copy the
/// fields out.
static PWDB_LOCK: Mutex<()> = Mutex::new(());

fn str_val(s: String) -> RubyValue {
    RubyValue::Str(crate::string_new(s))
}

pub(crate) use crate::errno_ptr;
pub(crate) use group::GROUP_MEMBERS;
pub(crate) use passwd::PASSWD_MEMBERS;

/// A libc C string to an owned Rust `String` (empty for NULL).
unsafe fn cstr(p: *const libc::c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

/// `Etc::Passwd.each` / `Etc.passwd { }` -- walk the user database from the
/// current cursor, one `Etc::Passwd` per row. Without a block it is a single
/// `getpwent`, which is what CRuby answers too.
pub(crate) fn each_passwd(block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let Some(RubyValue::Proc(p)) = block else {
        let _g = PWDB_LOCK.lock();
        let pw = unsafe { libc::getpwent() };
        return if pw.is_null() {
            Ok(RubyValue::Nil)
        } else {
            Ok(unsafe { passwd::passwd_from(pw) })
        };
    };
    loop {
        let next = {
            let _g = PWDB_LOCK.lock();
            let pw = unsafe { libc::getpwent() };
            if pw.is_null() {
                None
            } else {
                Some(unsafe { passwd::passwd_from(pw) })
            }
        };
        match next {
            Some(v) => {
                p.call(&[v])?;
            }
            None => break,
        }
    }
    Ok(RubyValue::Nil)
}

/// [`each_passwd`]'s group twin.
pub(crate) fn each_group(block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let Some(RubyValue::Proc(p)) = block else {
        let _g = PWDB_LOCK.lock();
        let gr = unsafe { libc::getgrent() };
        return if gr.is_null() {
            Ok(RubyValue::Nil)
        } else {
            Ok(unsafe { group::group_from(gr) })
        };
    };
    loop {
        let next = {
            let _g = PWDB_LOCK.lock();
            let gr = unsafe { libc::getgrent() };
            if gr.is_null() {
                None
            } else {
                Some(unsafe { group::group_from(gr) })
            }
        };
        match next {
            Some(v) => {
                p.call(&[v])?;
            }
            None => break,
        }
    }
    Ok(RubyValue::Nil)
}

// ---------------------------------------------------------------------------
// Shared libc helpers used by the Etc module functions.
// ---------------------------------------------------------------------------

fn cstring_arg(v: &RubyValue) -> Result<CString, Signal> {
    let s = crate::builtins::convert::to_rstr(v)?
        .lock()
        .to_utf8_lossy()
        .into_owned();
    CString::new(s).map_err(|_| arg_error!("string contains null byte"))
}

/// The per-user temporary directory: `confstr(_CS_DARWIN_USER_TEMP_DIR)` on
/// macOS (what CRuby's `Etc.systmpdir` returns there), `P_tmpdir` elsewhere.
fn system_tmpdir() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(RubyValue::Str(s)) = confstr_value(libc::_CS_DARWIN_USER_TEMP_DIR) {
            let dir = s.lock().to_utf8_lossy().into_owned();
            if !dir.is_empty() {
                return dir.trim_end_matches('/').to_string();
            }
        }
    }
    "/tmp".to_string()
}

fn confstr_value(name: libc::c_int) -> Result<RubyValue, Signal> {
    // Two-call idiom: probe the length, then fill.
    let len = unsafe { libc::confstr(name, std::ptr::null_mut(), 0) };
    if len == 0 {
        return Ok(RubyValue::Nil);
    }
    let mut buf = vec![0u8; len];
    unsafe { libc::confstr(name, buf.as_mut_ptr() as *mut libc::c_char, len) };
    // Drop the trailing NUL confstr counts in `len`.
    if buf.last() == Some(&0) {
        buf.pop();
    }
    Ok(str_val(String::from_utf8_lossy(&buf).into_owned()))
}

fn uname_hash() -> Result<RubyValue, Signal> {
    let mut u: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut u) } != 0 {
        return Err(raise_error("Errno::EFAULT", "uname".to_string()));
    }
    let field = |a: &[libc::c_char]| -> RubyValue { str_val(unsafe { cstr(a.as_ptr()) }) };
    Ok(RubyValue::Hash(crate::collections::hash_new(vec![
        (
            RubyValue::Symbol(Symbol::intern("sysname")),
            field(&u.sysname),
        ),
        (
            RubyValue::Symbol(Symbol::intern("nodename")),
            field(&u.nodename),
        ),
        (
            RubyValue::Symbol(Symbol::intern("release")),
            field(&u.release),
        ),
        (
            RubyValue::Symbol(Symbol::intern("version")),
            field(&u.version),
        ),
        (
            RubyValue::Symbol(Symbol::intern("machine")),
            field(&u.machine),
        ),
    ])))
}

/// Seed `Etc`'s `SC_*`/`PC_*`/`CS_*` constants -- the portable subset `libc`
/// exposes on this build target, mapped from CRuby's `Etc::SC_FOO` name to
/// `libc::_SC_FOO`. Called from `bootstrap::install_core_constants`.
pub fn seed_etc() {
    let cid = zeo_abi::ETC_MODULE.0;
    let set = |name: &str, v: i64| crate::constants::const_set(cid, name, RubyValue::Int(v));
    macro_rules! c {
        ($($ruby:literal => $val:expr),* $(,)?) => { $( set($ruby, $val as i64); )* };
    }
    // sysconf(3) names.
    c! {
        "SC_ARG_MAX" => libc::_SC_ARG_MAX,
        "SC_CHILD_MAX" => libc::_SC_CHILD_MAX,
        "SC_CLK_TCK" => libc::_SC_CLK_TCK,
        "SC_NGROUPS_MAX" => libc::_SC_NGROUPS_MAX,
        "SC_OPEN_MAX" => libc::_SC_OPEN_MAX,
        "SC_STREAM_MAX" => libc::_SC_STREAM_MAX,
        "SC_TZNAME_MAX" => libc::_SC_TZNAME_MAX,
        "SC_JOB_CONTROL" => libc::_SC_JOB_CONTROL,
        "SC_SAVED_IDS" => libc::_SC_SAVED_IDS,
        "SC_VERSION" => libc::_SC_VERSION,
        "SC_BC_BASE_MAX" => libc::_SC_BC_BASE_MAX,
        "SC_BC_DIM_MAX" => libc::_SC_BC_DIM_MAX,
        "SC_BC_SCALE_MAX" => libc::_SC_BC_SCALE_MAX,
        "SC_BC_STRING_MAX" => libc::_SC_BC_STRING_MAX,
        "SC_COLL_WEIGHTS_MAX" => libc::_SC_COLL_WEIGHTS_MAX,
        "SC_EXPR_NEST_MAX" => libc::_SC_EXPR_NEST_MAX,
        "SC_LINE_MAX" => libc::_SC_LINE_MAX,
        "SC_RE_DUP_MAX" => libc::_SC_RE_DUP_MAX,
        "SC_2_VERSION" => libc::_SC_2_VERSION,
        "SC_2_C_BIND" => libc::_SC_2_C_BIND,
        "SC_2_C_DEV" => libc::_SC_2_C_DEV,
        "SC_2_CHAR_TERM" => libc::_SC_2_CHAR_TERM,
        "SC_2_FORT_DEV" => libc::_SC_2_FORT_DEV,
        "SC_2_FORT_RUN" => libc::_SC_2_FORT_RUN,
        "SC_2_LOCALEDEF" => libc::_SC_2_LOCALEDEF,
        "SC_2_SW_DEV" => libc::_SC_2_SW_DEV,
        "SC_2_UPE" => libc::_SC_2_UPE,
        "SC_PAGESIZE" => libc::_SC_PAGESIZE,
        "SC_PAGE_SIZE" => libc::_SC_PAGE_SIZE,
        "SC_NPROCESSORS_CONF" => libc::_SC_NPROCESSORS_CONF,
        "SC_NPROCESSORS_ONLN" => libc::_SC_NPROCESSORS_ONLN,
        "SC_GETGR_R_SIZE_MAX" => libc::_SC_GETGR_R_SIZE_MAX,
        "SC_GETPW_R_SIZE_MAX" => libc::_SC_GETPW_R_SIZE_MAX,
        "SC_LOGIN_NAME_MAX" => libc::_SC_LOGIN_NAME_MAX,
        "SC_TTY_NAME_MAX" => libc::_SC_TTY_NAME_MAX,
        "SC_SYMLOOP_MAX" => libc::_SC_SYMLOOP_MAX,
        "SC_HOST_NAME_MAX" => libc::_SC_HOST_NAME_MAX,
        "SC_ATEXIT_MAX" => libc::_SC_ATEXIT_MAX,
        "SC_IOV_MAX" => libc::_SC_IOV_MAX,
    }
    // pathconf(3) names.
    c! {
        "PC_LINK_MAX" => libc::_PC_LINK_MAX,
        "PC_MAX_CANON" => libc::_PC_MAX_CANON,
        "PC_MAX_INPUT" => libc::_PC_MAX_INPUT,
        "PC_NAME_MAX" => libc::_PC_NAME_MAX,
        "PC_PATH_MAX" => libc::_PC_PATH_MAX,
        "PC_PIPE_BUF" => libc::_PC_PIPE_BUF,
        "PC_CHOWN_RESTRICTED" => libc::_PC_CHOWN_RESTRICTED,
        "PC_NO_TRUNC" => libc::_PC_NO_TRUNC,
        "PC_VDISABLE" => libc::_PC_VDISABLE,
    }
    // confstr(3) names.
    c! {
        "CS_PATH" => libc::_CS_PATH,
    }
}

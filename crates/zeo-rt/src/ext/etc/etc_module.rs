//! The `Etc` module itself -- the system-database query functions
//! (`getpwnam`/`getgrgid`/`getpwent`/...), `sysconf`/`confstr`, `uname`,
//! `nprocessors`, `getlogin`, and the install-path helpers. The user/group rows
//! it returns are the `Etc::Passwd` / `Etc::Group` value objects from the
//! sibling files.

use super::group::group_from;
use super::passwd::passwd_from;
use super::{
    PWDB_LOCK, confstr_value, cstr, cstring_arg, errno_ptr, str_val, system_tmpdir, uname_hash,
};
use crate::RubyValue;
use crate::builtins::arg_error;
use crate::dispatch::raise_error;
use zeo_macros::ruby_module;

ruby_module! {
    Etc = zeo_abi::ETC_MODULE;

    // The install-path helpers.
    def self."sysconfdir"(_recv) {
        Ok(str_val("/etc".to_string()))
    }
    def self."systmpdir"(_recv) {
        Ok(str_val(system_tmpdir()))
    }

    // CPU count -- the affinity/online-processor count.
    def self."nprocessors"(_recv) {
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        Ok(RubyValue::Int(n as i64))
    }

    // The login name of the controlling terminal's user (nil if unavailable).
    def self."getlogin"(_recv) {
        let _g = PWDB_LOCK.lock();
        let p = unsafe { libc::getlogin() };
        if p.is_null() { Ok(RubyValue::Nil) } else { Ok(str_val(unsafe { cstr(p) })) }
    }

    // uname(2) as a Hash of the five portable fields.
    def self."uname"(_recv) {
        uname_hash()
    }

    // sysconf(3)/confstr(3): runtime system-configuration queries.
    def self."sysconf"(_recv, arg) {
        let name = crate::builtins::convert::to_index(arg)? as libc::c_int;
        // A -1 return with errno unchanged (0) means "no limit / indeterminate"
        // -> nil, matching CRuby; a real error (EINVAL: unknown name) raises.
        unsafe { *errno_ptr() = 0; }
        let v = unsafe { libc::sysconf(name) };
        if v == -1 {
            let e = unsafe { *errno_ptr() };
            if e == 0 { return Ok(RubyValue::Nil); }
            return Err(raise_error("Errno::EINVAL", format!("sysconf: {name}")));
        }
        Ok(RubyValue::Int(v))
    }
    def self."confstr"(_recv, arg) {
        let name = crate::builtins::convert::to_index(arg)? as libc::c_int;
        confstr_value(name)
    }

    // The user database: by name, by uid (default: the effective uid), and the
    // cursor form (getpwent / setpwent / endpwent, and the `passwd` iterator).
    def self."getpwnam"(_recv, arg) {
        let name = cstring_arg(arg)?;
        let _g = PWDB_LOCK.lock();
        let pw = unsafe { libc::getpwnam(name.as_ptr()) };
        if pw.is_null() {
            // The NAME, not its inspect: ruby prints `can't find user for
            // nobody`, without quotes around it.
            return Err(arg_error!(
                "can't find user for {}",
                name.to_string_lossy()
            ));
        }
        Ok(unsafe { passwd_from(pw) })
    }
    def self."getpwuid"(_recv, arg?) {
        let uid = match arg {
            Some(v) => crate::builtins::convert::to_index(v)? as libc::uid_t,
            None => unsafe { libc::geteuid() },
        };
        let _g = PWDB_LOCK.lock();
        let pw = unsafe { libc::getpwuid(uid) };
        if pw.is_null() {
            return Err(arg_error!("can't find user for {uid}"));
        }
        Ok(unsafe { passwd_from(pw) })
    }
    def self."setpwent"(_recv) {
        let _g = PWDB_LOCK.lock();
        unsafe { libc::setpwent() };
        Ok(RubyValue::Nil)
    }
    def self."endpwent"(_recv) {
        let _g = PWDB_LOCK.lock();
        unsafe { libc::endpwent() };
        Ok(RubyValue::Nil)
    }
    def self."getpwent"(_recv) {
        let _g = PWDB_LOCK.lock();
        let pw = unsafe { libc::getpwent() };
        if pw.is_null() { Ok(RubyValue::Nil) } else { Ok(unsafe { passwd_from(pw) }) }
    }
    // `Etc.passwd { |pw| ... }` iterates the whole database (from the current
    // cursor); without a block it is `getpwent`.
    def self."passwd"(_recv, &block) {
        let Some(RubyValue::Proc(p)) = block else {
            let _g = PWDB_LOCK.lock();
            let pw = unsafe { libc::getpwent() };
            return if pw.is_null() { Ok(RubyValue::Nil) } else { Ok(unsafe { passwd_from(pw) }) };
        };
        loop {
            let next = {
                let _g = PWDB_LOCK.lock();
                let pw = unsafe { libc::getpwent() };
                if pw.is_null() { None } else { Some(unsafe { passwd_from(pw) }) }
            };
            match next {
                Some(v) => { p.call(&[v])?; }
                None => break,
            }
        }
        Ok(RubyValue::Nil)
    }

    // The group database, mirroring the user one.
    def self."getgrnam"(_recv, arg) {
        let name = cstring_arg(arg)?;
        let _g = PWDB_LOCK.lock();
        let gr = unsafe { libc::getgrnam(name.as_ptr()) };
        if gr.is_null() {
            return Err(arg_error!("can't find group for {}", (*arg).inspect_string()));
        }
        Ok(unsafe { group_from(gr) })
    }
    def self."getgrgid"(_recv, arg?) {
        let gid = match arg {
            Some(v) => crate::builtins::convert::to_index(v)? as libc::gid_t,
            None => unsafe { libc::getegid() },
        };
        let _g = PWDB_LOCK.lock();
        let gr = unsafe { libc::getgrgid(gid) };
        if gr.is_null() {
            return Err(arg_error!("can't find group for {gid}"));
        }
        Ok(unsafe { group_from(gr) })
    }
    def self."setgrent"(_recv) {
        let _g = PWDB_LOCK.lock();
        unsafe { libc::setgrent() };
        Ok(RubyValue::Nil)
    }
    def self."endgrent"(_recv) {
        let _g = PWDB_LOCK.lock();
        unsafe { libc::endgrent() };
        Ok(RubyValue::Nil)
    }
    def self."getgrent"(_recv) {
        let _g = PWDB_LOCK.lock();
        let gr = unsafe { libc::getgrent() };
        if gr.is_null() { Ok(RubyValue::Nil) } else { Ok(unsafe { group_from(gr) }) }
    }
    def self."group"(_recv, &block) {
        let Some(RubyValue::Proc(p)) = block else {
            let _g = PWDB_LOCK.lock();
            let gr = unsafe { libc::getgrent() };
            return if gr.is_null() { Ok(RubyValue::Nil) } else { Ok(unsafe { group_from(gr) }) };
        };
        loop {
            let next = {
                let _g = PWDB_LOCK.lock();
                let gr = unsafe { libc::getgrent() };
                if gr.is_null() { None } else { Some(unsafe { group_from(gr) }) }
            };
            match next {
                Some(v) => { p.call(&[v])?; }
                None => break,
            }
        }
        Ok(RubyValue::Nil)
    }
}

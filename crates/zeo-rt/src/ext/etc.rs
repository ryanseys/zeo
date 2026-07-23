//! `Etc` -- CRuby's `ext/etc` module, over `libc`. Access to the system
//! user/group databases (`getpwnam`/`getpwuid`/`getgrnam`/`getgrgid` and the
//! `getpwent`/`getgrent` cursors), `sysconf`/`confstr`, `uname`, `nprocessors`,
//! `getlogin`, and the install-path helpers (`sysconfdir`/`systmpdir`).
//!
//! `require`-gated on `"etc"`. The user/group lookups return `Etc::Passwd` /
//! `Etc::Group` value objects (the `Process::Tms` pattern -- a small payload
//! struct with matching accessors, plus the Struct-like `to_a`/`to_h`/`members`/
//! `each`/`[]`). The database calls are NOT reentrant (they hand back pointers
//! into a shared static buffer), so every one runs under `PWDB_LOCK`, copying
//! the fields out before releasing it.

use std::ffi::{CStr, CString};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::builtins::{arg_error, arity, builtin_methods, type_error};
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::{RubyValue, Signal, Symbol};
use zeo_abi::{ClassId, ETC_GROUP_CLASS, ETC_PASSWD_CLASS};

/// Serializes access to the non-reentrant `getpw*`/`getgr*` family, whose
/// results alias a shared static buffer. Held only long enough to copy the
/// fields out.
static PWDB_LOCK: Mutex<()> = Mutex::new(());

fn str_val(s: String) -> RubyValue {
    RubyValue::Str(crate::string_new(s))
}

/// Portable pointer to the thread's `errno`.
unsafe fn errno_ptr() -> *mut libc::c_int {
    #[cfg(target_os = "macos")]
    {
        unsafe { libc::__error() }
    }
    #[cfg(not(target_os = "macos"))]
    {
        unsafe { libc::__errno_location() }
    }
}

/// A libc C string to an owned Rust `String` (empty for NULL).
unsafe fn cstr(p: *const libc::c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

// ---------------------------------------------------------------------------
// Etc::Passwd -- one password-database row.
// ---------------------------------------------------------------------------

pub struct RPasswd {
    name: String,
    passwd: String,
    uid: u32,
    gid: u32,
    gecos: String,
    dir: String,
    shell: String,
    // The BSD/Darwin `struct passwd` carries three extra fields CRuby exposes as
    // `change`/`uclass`/`expire`; glibc's does not, so they are Apple-gated.
    #[cfg(target_vendor = "apple")]
    change: i64,
    #[cfg(target_vendor = "apple")]
    uclass: String,
    #[cfg(target_vendor = "apple")]
    expire: i64,
}

#[cfg(target_vendor = "apple")]
const PASSWD_MEMBERS: &[&str] = &[
    "name", "passwd", "uid", "gid", "gecos", "dir", "shell", "change", "uclass", "expire",
];
#[cfg(not(target_vendor = "apple"))]
const PASSWD_MEMBERS: &[&str] = &["name", "passwd", "uid", "gid", "gecos", "dir", "shell"];

impl RPasswd {
    fn field(&self, name: &str) -> Option<RubyValue> {
        Some(match name {
            "name" => str_val(self.name.clone()),
            "passwd" => str_val(self.passwd.clone()),
            "uid" => RubyValue::Int(self.uid as i64),
            "gid" => RubyValue::Int(self.gid as i64),
            "gecos" => str_val(self.gecos.clone()),
            "dir" => str_val(self.dir.clone()),
            "shell" => str_val(self.shell.clone()),
            #[cfg(target_vendor = "apple")]
            "change" => RubyValue::Int(self.change),
            #[cfg(target_vendor = "apple")]
            "uclass" => str_val(self.uclass.clone()),
            #[cfg(target_vendor = "apple")]
            "expire" => RubyValue::Int(self.expire),
            _ => return None,
        })
    }
}

impl RubyObject for RPasswd {
    fn class_id(&self) -> ClassId {
        ETC_PASSWD_CLASS
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
        Arc::new(RPasswd {
            name: self.name.clone(),
            passwd: self.passwd.clone(),
            uid: self.uid,
            gid: self.gid,
            gecos: self.gecos.clone(),
            dir: self.dir.clone(),
            shell: self.shell.clone(),
            #[cfg(target_vendor = "apple")]
            change: self.change,
            #[cfg(target_vendor = "apple")]
            uclass: self.uclass.clone(),
            #[cfg(target_vendor = "apple")]
            expire: self.expire,
        })
    }
}

fn recv_passwd(recv: &RubyValue) -> &RPasswd {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RPasswd>()
            .expect("Etc::Passwd row on a non-Passwd receiver"),
        _ => panic!("Etc::Passwd row on a non-Object receiver"),
    }
}

builtin_methods! {
    pub(crate) fn lookup_passwd;

    "name" => fn pw_name(recv, args, _b) { arity!(args, 0); Ok(str_val(recv_passwd(recv).name.clone())) }
    "passwd" => fn pw_passwd(recv, args, _b) { arity!(args, 0); Ok(str_val(recv_passwd(recv).passwd.clone())) }
    "uid" => fn pw_uid(recv, args, _b) { arity!(args, 0); Ok(RubyValue::Int(recv_passwd(recv).uid as i64)) }
    "gid" => fn pw_gid(recv, args, _b) { arity!(args, 0); Ok(RubyValue::Int(recv_passwd(recv).gid as i64)) }
    "gecos" => fn pw_gecos(recv, args, _b) { arity!(args, 0); Ok(str_val(recv_passwd(recv).gecos.clone())) }
    "dir" => fn pw_dir(recv, args, _b) { arity!(args, 0); Ok(str_val(recv_passwd(recv).dir.clone())) }
    "shell" => fn pw_shell(recv, args, _b) { arity!(args, 0); Ok(str_val(recv_passwd(recv).shell.clone())) }
    "members" => fn pw_members(_recv, args, _b) { arity!(args, 0); Ok(members_array(PASSWD_MEMBERS)) }
    "to_a" => fn pw_to_a(recv, args, _b) { arity!(args, 0); Ok(struct_to_a(recv_passwd(recv), PASSWD_MEMBERS)) }
    "values" => fn pw_values(recv, args, _b) { arity!(args, 0); Ok(struct_to_a(recv_passwd(recv), PASSWD_MEMBERS)) }
    "to_h" => fn pw_to_h(recv, args, _b) { arity!(args, 0); Ok(struct_to_h(recv_passwd(recv), PASSWD_MEMBERS)) }
    "each" => fn pw_each(recv, args, block) { arity!(args, 0); struct_each(recv_passwd(recv), PASSWD_MEMBERS, block, recv) }
    "[]" => fn pw_index(recv, args, _b) { arity!(args, 1); struct_index(recv_passwd(recv), PASSWD_MEMBERS, &args[0]) }
    "to_s" | "inspect" => fn pw_inspect(recv, args, _b) { arity!(args, 0); Ok(str_val(struct_inspect("Etc::Passwd", recv_passwd(recv), PASSWD_MEMBERS))) }
}

// The BSD/Darwin-only `Passwd` accessors, kept in their own table because
// `builtin_methods!` can't `#[cfg]` an individual row.
#[cfg(target_vendor = "apple")]
builtin_methods! {
    pub(crate) fn lookup_passwd_bsd;

    "change" => fn pw_change(recv, args, _b) { arity!(args, 0); Ok(recv_passwd(recv).field("change").unwrap()) }
    "uclass" => fn pw_uclass(recv, args, _b) { arity!(args, 0); Ok(recv_passwd(recv).field("uclass").unwrap()) }
    "expire" => fn pw_expire(recv, args, _b) { arity!(args, 0); Ok(recv_passwd(recv).field("expire").unwrap()) }
}

/// The `Etc::Passwd` instance-method lookup: the portable accessors plus, on
/// BSD/Darwin, the `change`/`uclass`/`expire` fields.
pub(crate) fn passwd_lookup(name: &str) -> Option<crate::builtins::BuiltinMethodFn> {
    if let Some(f) = lookup_passwd(name) {
        return Some(f);
    }
    #[cfg(target_vendor = "apple")]
    {
        lookup_passwd_bsd(name)
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        None
    }
}

/// The reflection name list for `Etc::Passwd` -- every member (the BSD extras
/// included on Darwin) plus the Struct-like helpers.
pub(crate) fn passwd_names() -> &'static [&'static str] {
    #[cfg(target_vendor = "apple")]
    {
        &[
            "name", "passwd", "uid", "gid", "gecos", "dir", "shell", "change", "uclass", "expire",
            "members", "to_a", "values", "to_h", "each", "[]", "to_s", "inspect",
        ]
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        lookup_passwd_names()
    }
}

// ---------------------------------------------------------------------------
// Etc::Group -- one group-database row.
// ---------------------------------------------------------------------------

pub struct RGroup {
    name: String,
    passwd: String,
    gid: u32,
    mem: Vec<String>,
}

const GROUP_MEMBERS: &[&str] = &["name", "passwd", "gid", "mem"];

impl RGroup {
    fn field(&self, name: &str) -> Option<RubyValue> {
        Some(match name {
            "name" => str_val(self.name.clone()),
            "passwd" => str_val(self.passwd.clone()),
            "gid" => RubyValue::Int(self.gid as i64),
            "mem" => RubyValue::Array(crate::array_new(
                self.mem.iter().cloned().map(str_val).collect(),
            )),
            _ => return None,
        })
    }
}

impl RubyObject for RGroup {
    fn class_id(&self) -> ClassId {
        ETC_GROUP_CLASS
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
        Arc::new(RGroup {
            name: self.name.clone(),
            passwd: self.passwd.clone(),
            gid: self.gid,
            mem: self.mem.clone(),
        })
    }
}

fn recv_group(recv: &RubyValue) -> &RGroup {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RGroup>()
            .expect("Etc::Group row on a non-Group receiver"),
        _ => panic!("Etc::Group row on a non-Object receiver"),
    }
}

builtin_methods! {
    pub(crate) fn lookup_group;

    "name" => fn gr_name(recv, args, _b) { arity!(args, 0); Ok(str_val(recv_group(recv).name.clone())) }
    "passwd" => fn gr_passwd(recv, args, _b) { arity!(args, 0); Ok(str_val(recv_group(recv).passwd.clone())) }
    "gid" => fn gr_gid(recv, args, _b) { arity!(args, 0); Ok(RubyValue::Int(recv_group(recv).gid as i64)) }
    "mem" => fn gr_mem(recv, args, _b) { arity!(args, 0); Ok(recv_group(recv).field("mem").unwrap()) }
    "members" => fn gr_members(_recv, args, _b) { arity!(args, 0); Ok(members_array(GROUP_MEMBERS)) }
    "to_a" => fn gr_to_a(recv, args, _b) { arity!(args, 0); Ok(struct_to_a(recv_group(recv), GROUP_MEMBERS)) }
    "values" => fn gr_values(recv, args, _b) { arity!(args, 0); Ok(struct_to_a(recv_group(recv), GROUP_MEMBERS)) }
    "to_h" => fn gr_to_h(recv, args, _b) { arity!(args, 0); Ok(struct_to_h(recv_group(recv), GROUP_MEMBERS)) }
    "each" => fn gr_each(recv, args, block) { arity!(args, 0); struct_each(recv_group(recv), GROUP_MEMBERS, block, recv) }
    "[]" => fn gr_index(recv, args, _b) { arity!(args, 1); struct_index(recv_group(recv), GROUP_MEMBERS, &args[0]) }
    "to_s" | "inspect" => fn gr_inspect(recv, args, _b) { arity!(args, 0); Ok(str_val(struct_inspect("Etc::Group", recv_group(recv), GROUP_MEMBERS))) }
}

/// The shared Struct-like surface (`members`/`to_a`/`to_h`/`each`/`[]`/inspect)
/// over any payload exposing `field(name)`.
trait StructRow {
    fn field(&self, name: &str) -> Option<RubyValue>;
}
impl StructRow for RPasswd {
    fn field(&self, n: &str) -> Option<RubyValue> {
        RPasswd::field(self, n)
    }
}
impl StructRow for RGroup {
    fn field(&self, n: &str) -> Option<RubyValue> {
        RGroup::field(self, n)
    }
}

fn members_array(members: &[&str]) -> RubyValue {
    RubyValue::Array(crate::array_new(
        members
            .iter()
            .map(|m| RubyValue::Symbol(Symbol::intern(m)))
            .collect(),
    ))
}

fn struct_to_a(row: &dyn StructRow, members: &[&str]) -> RubyValue {
    RubyValue::Array(crate::array_new(
        members.iter().map(|m| row.field(m).unwrap()).collect(),
    ))
}

fn struct_to_h(row: &dyn StructRow, members: &[&str]) -> RubyValue {
    RubyValue::Hash(crate::collections::hash_new(
        members
            .iter()
            .map(|m| (RubyValue::Symbol(Symbol::intern(m)), row.field(m).unwrap()))
            .collect(),
    ))
}

fn struct_each(
    row: &dyn StructRow,
    members: &[&str],
    block: Option<RubyValue>,
    recv: &RubyValue,
) -> Result<RubyValue, Signal> {
    let Some(RubyValue::Proc(p)) = block else {
        // No block -> an Enumerator over the values, matching Struct#each.
        return Ok(crate::builtins::enumerator::enumerator_for(recv, "each", &[]));
    };
    for m in members {
        p.call(&[row.field(m).unwrap()])?;
    }
    Ok(recv.clone())
}

fn struct_index(row: &dyn StructRow, members: &[&str], key: &RubyValue) -> Result<RubyValue, Signal> {
    match key {
        RubyValue::Int(i) => {
            let idx = if *i < 0 { *i + members.len() as i64 } else { *i };
            let name = members
                .get(usize::try_from(idx).unwrap_or(usize::MAX))
                .copied()
                .ok_or_else(|| {
                    arg_error!("offset {i} too large for struct(size:{})", members.len())
                })?;
            Ok(row.field(name).unwrap())
        }
        RubyValue::Symbol(s) => {
            let n = s.name();
            row.field(&n).ok_or_else(|| name_error_no_member(&n))
        }
        RubyValue::Str(s) => {
            let n = s.lock().to_utf8_lossy().into_owned();
            row.field(&n).ok_or_else(|| name_error_no_member(&n))
        }
        _ => Err(type_error!("no implicit conversion into Integer")),
    }
}

fn name_error_no_member(name: &str) -> Signal {
    raise_error("NameError", format!("no member '{name}' in struct"))
}

fn struct_inspect(class: &str, row: &dyn StructRow, members: &[&str]) -> String {
    let body: Vec<String> = members
        .iter()
        .map(|m| format!("{m}={}", row.field(m).unwrap().inspect_string()))
        .collect();
    format!("#<struct {class} {}>", body.join(", "))
}

// ---------------------------------------------------------------------------
// Etc module functions.
// ---------------------------------------------------------------------------

unsafe fn passwd_from(pw: *const libc::passwd) -> RubyValue {
    let pw = unsafe { &*pw };
    RubyValue::Object(Arc::new(RPasswd {
        name: unsafe { cstr(pw.pw_name) },
        passwd: unsafe { cstr(pw.pw_passwd) },
        uid: pw.pw_uid,
        gid: pw.pw_gid,
        gecos: unsafe { cstr(pw.pw_gecos) },
        dir: unsafe { cstr(pw.pw_dir) },
        shell: unsafe { cstr(pw.pw_shell) },
        #[cfg(target_vendor = "apple")]
        change: pw.pw_change,
        #[cfg(target_vendor = "apple")]
        uclass: unsafe { cstr(pw.pw_class) },
        #[cfg(target_vendor = "apple")]
        expire: pw.pw_expire,
    }))
}

unsafe fn group_from(gr: *const libc::group) -> RubyValue {
    let gr = unsafe { &*gr };
    let mut mem = Vec::new();
    if !gr.gr_mem.is_null() {
        let mut p = gr.gr_mem;
        while !unsafe { *p }.is_null() {
            mem.push(unsafe { cstr(*p) });
            p = unsafe { p.add(1) };
        }
    }
    RubyValue::Object(Arc::new(RGroup {
        name: unsafe { cstr(gr.gr_name) },
        passwd: unsafe { cstr(gr.gr_passwd) },
        gid: gr.gr_gid,
        mem,
    }))
}

fn cstring_arg(v: &RubyValue) -> Result<CString, Signal> {
    let s = crate::builtins::convert::to_rstr(v)?
        .lock()
        .to_utf8_lossy()
        .into_owned();
    CString::new(s).map_err(|_| arg_error!("string contains null byte"))
}

builtin_methods! {
    pub(crate) fn lookup_class;

    // The install-path helpers.
    "sysconfdir" => fn sysconfdir(_recv, args, _b) {
        arity!(args, 0);
        Ok(str_val("/etc".to_string()))
    }
    "systmpdir" => fn systmpdir(_recv, args, _b) {
        arity!(args, 0);
        Ok(str_val(system_tmpdir()))
    }

    // CPU count -- the affinity/online-processor count.
    "nprocessors" => fn nprocessors(_recv, args, _b) {
        arity!(args, 0);
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        Ok(RubyValue::Int(n as i64))
    }

    // The login name of the controlling terminal's user (nil if unavailable).
    "getlogin" => fn getlogin(_recv, args, _b) {
        arity!(args, 0);
        let _g = PWDB_LOCK.lock();
        let p = unsafe { libc::getlogin() };
        if p.is_null() { Ok(RubyValue::Nil) } else { Ok(str_val(unsafe { cstr(p) })) }
    }

    // uname(2) as a Hash of the five portable fields.
    "uname" => fn uname(_recv, args, _b) {
        arity!(args, 0);
        uname_hash()
    }

    // sysconf(3)/confstr(3): runtime system-configuration queries.
    "sysconf" => fn sysconf(_recv, args, _b) {
        arity!(args, 1);
        let name = crate::builtins::convert::to_index(&args[0])? as libc::c_int;
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
    "confstr" => fn confstr(_recv, args, _b) {
        arity!(args, 1);
        let name = crate::builtins::convert::to_index(&args[0])? as libc::c_int;
        confstr_value(name)
    }

    // The user database: by name, by uid (default: the effective uid), and the
    // cursor form (getpwent / setpwent / endpwent, and the `passwd` iterator).
    "getpwnam" => fn getpwnam(_recv, args, _b) {
        arity!(args, 1);
        let name = cstring_arg(&args[0])?;
        let _g = PWDB_LOCK.lock();
        let pw = unsafe { libc::getpwnam(name.as_ptr()) };
        if pw.is_null() {
            return Err(arg_error!("can't find user for {}", args[0].inspect_string()));
        }
        Ok(unsafe { passwd_from(pw) })
    }
    "getpwuid" => fn getpwuid(_recv, args, _b) {
        arity!(args, 0..=1);
        let uid = match args.first() {
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
    "setpwent" => fn setpwent(_recv, args, _b) {
        arity!(args, 0);
        let _g = PWDB_LOCK.lock();
        unsafe { libc::setpwent() };
        Ok(RubyValue::Nil)
    }
    "endpwent" => fn endpwent(_recv, args, _b) {
        arity!(args, 0);
        let _g = PWDB_LOCK.lock();
        unsafe { libc::endpwent() };
        Ok(RubyValue::Nil)
    }
    "getpwent" => fn getpwent(_recv, args, _b) {
        arity!(args, 0);
        let _g = PWDB_LOCK.lock();
        let pw = unsafe { libc::getpwent() };
        if pw.is_null() { Ok(RubyValue::Nil) } else { Ok(unsafe { passwd_from(pw) }) }
    }
    // `Etc.passwd { |pw| ... }` iterates the whole database (from the current
    // cursor); without a block it is `getpwent`.
    "passwd" => fn passwd(_recv, args, block) {
        arity!(args, 0);
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
    "getgrnam" => fn getgrnam(_recv, args, _b) {
        arity!(args, 1);
        let name = cstring_arg(&args[0])?;
        let _g = PWDB_LOCK.lock();
        let gr = unsafe { libc::getgrnam(name.as_ptr()) };
        if gr.is_null() {
            return Err(arg_error!("can't find group for {}", args[0].inspect_string()));
        }
        Ok(unsafe { group_from(gr) })
    }
    "getgrgid" => fn getgrgid(_recv, args, _b) {
        arity!(args, 0..=1);
        let gid = match args.first() {
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
    "setgrent" => fn setgrent(_recv, args, _b) {
        arity!(args, 0);
        let _g = PWDB_LOCK.lock();
        unsafe { libc::setgrent() };
        Ok(RubyValue::Nil)
    }
    "endgrent" => fn endgrent(_recv, args, _b) {
        arity!(args, 0);
        let _g = PWDB_LOCK.lock();
        unsafe { libc::endgrent() };
        Ok(RubyValue::Nil)
    }
    "getgrent" => fn getgrent(_recv, args, _b) {
        arity!(args, 0);
        let _g = PWDB_LOCK.lock();
        let gr = unsafe { libc::getgrent() };
        if gr.is_null() { Ok(RubyValue::Nil) } else { Ok(unsafe { group_from(gr) }) }
    }
    "group" => fn group(_recv, args, block) {
        arity!(args, 0);
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
    let field = |a: &[libc::c_char]| -> RubyValue {
        str_val(unsafe { cstr(a.as_ptr()) })
    };
    Ok(RubyValue::Hash(crate::collections::hash_new(vec![
        (RubyValue::Symbol(Symbol::intern("sysname")), field(&u.sysname)),
        (RubyValue::Symbol(Symbol::intern("nodename")), field(&u.nodename)),
        (RubyValue::Symbol(Symbol::intern("release")), field(&u.release)),
        (RubyValue::Symbol(Symbol::intern("version")), field(&u.version)),
        (RubyValue::Symbol(Symbol::intern("machine")), field(&u.machine)),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn a_passwd() -> RPasswd {
        RPasswd {
            name: "alice".into(),
            passwd: "*".into(),
            uid: 1000,
            gid: 2000,
            gecos: "Alice".into(),
            dir: "/home/alice".into(),
            shell: "/bin/zsh".into(),
            #[cfg(target_vendor = "apple")]
            change: 0,
            #[cfg(target_vendor = "apple")]
            uclass: String::new(),
            #[cfg(target_vendor = "apple")]
            expire: 0,
        }
    }

    #[test]
    fn passwd_fields_map_to_the_right_ruby_types() {
        let pw = a_passwd();
        assert_eq!(pw.field("name").unwrap().inspect_string(), "\"alice\"");
        assert_eq!(pw.field("uid").unwrap().inspect_string(), "1000");
        assert_eq!(pw.field("gid").unwrap().inspect_string(), "2000");
        assert_eq!(pw.field("shell").unwrap().inspect_string(), "\"/bin/zsh\"");
        assert!(pw.field("nonexistent").is_none());
    }

    #[test]
    fn struct_to_a_and_to_h_cover_every_member_in_order() {
        let pw = a_passwd();
        let to_a = struct_to_a(&pw, PASSWD_MEMBERS);
        let RubyValue::Array(a) = to_a else { panic!("to_a is an Array") };
        assert_eq!(a.lock().to_vec().len(), PASSWD_MEMBERS.len());
        // First three values, in member order.
        assert_eq!(a.lock().to_vec()[0].inspect_string(), "\"alice\"");
        assert_eq!(a.lock().to_vec()[2].inspect_string(), "1000");

        let RubyValue::Hash(_) = struct_to_h(&pw, PASSWD_MEMBERS) else {
            panic!("to_h is a Hash")
        };
    }

    #[test]
    fn struct_index_accepts_int_symbol_and_string() {
        let pw = a_passwd();
        assert_eq!(struct_index(&pw, PASSWD_MEMBERS, &RubyValue::Int(0)).unwrap().inspect_string(), "\"alice\"");
        // Negative index counts from the end (shell is member index 6).
        assert_eq!(
            struct_index(&pw, PASSWD_MEMBERS, &RubyValue::Int(-(PASSWD_MEMBERS.len() as i64))).unwrap().inspect_string(),
            "\"alice\""
        );
        assert_eq!(
            struct_index(&pw, PASSWD_MEMBERS, &RubyValue::Symbol(Symbol::intern("uid"))).unwrap().inspect_string(),
            "1000"
        );
        // (Out-of-range / unknown-member RAISES are covered by the e2e tests,
        // which run with the full class registry; constructing an exception in
        // this registry-less unit context would panic.)
    }

    #[test]
    fn inspect_has_the_struct_shape() {
        let s = struct_inspect("Etc::Passwd", &a_passwd(), PASSWD_MEMBERS);
        assert!(s.starts_with("#<struct Etc::Passwd name=\"alice\", passwd=\"*\", uid=1000"), "{s}");
    }

    #[test]
    fn members_are_symbols() {
        let RubyValue::Array(a) = members_array(GROUP_MEMBERS) else { panic!() };
        assert_eq!(
            a.lock().to_vec().iter().map(|v| v.inspect_string()).collect::<Vec<_>>(),
            vec![":name", ":passwd", ":gid", ":mem"]
        );
    }
}

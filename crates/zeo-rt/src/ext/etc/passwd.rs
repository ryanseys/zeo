//! `Etc::Passwd` -- one password-database row, a Struct-like value object. The
//! BSD/Darwin `struct passwd` carries three extra fields CRuby exposes as
//! `change`/`uclass`/`expire`; glibc's does not, so both the payload fields and
//! their accessor `def`s are `#[cfg(target_vendor = "apple")]`-gated -- the
//! `#[cfg]` on a `def` drops the method from the lookup/names/arity surface as a
//! unit, exactly matching CRuby's platform-dependent method set.

use std::sync::Arc;

use super::{
    StructRow, members_array, str_val, struct_each, struct_index, struct_inspect, struct_to_a,
    struct_to_h,
};
use crate::RubyValue;
use crate::dispatch::{RObj, RubyObject};
use zeo_abi::{ClassId, ETC_PASSWD_CLASS};
use zeo_macros::ruby_class;

pub(crate) struct RPasswd {
    name: String,
    passwd: String,
    uid: u32,
    gid: u32,
    gecos: String,
    dir: String,
    shell: String,
    #[cfg(target_vendor = "apple")]
    change: i64,
    #[cfg(target_vendor = "apple")]
    uclass: String,
    #[cfg(target_vendor = "apple")]
    expire: i64,
}

#[cfg(target_vendor = "apple")]
pub(crate) const PASSWD_MEMBERS: &[&str] = &[
    "name", "passwd", "uid", "gid", "gecos", "dir", "shell", "change", "uclass", "expire",
];
#[cfg(not(target_vendor = "apple"))]
pub(crate) const PASSWD_MEMBERS: &[&str] =
    &["name", "passwd", "uid", "gid", "gecos", "dir", "shell"];

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

impl StructRow for RPasswd {
    fn field(&self, n: &str) -> Option<RubyValue> {
        RPasswd::field(self, n)
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

/// Copy a libc `struct passwd` into an owned `Etc::Passwd` value.
pub(crate) unsafe fn passwd_from(pw: *const libc::passwd) -> RubyValue {
    let pw = unsafe { &*pw };
    RubyValue::Object(Arc::new(RPasswd {
        name: unsafe { super::cstr(pw.pw_name) },
        passwd: unsafe { super::cstr(pw.pw_passwd) },
        uid: pw.pw_uid,
        gid: pw.pw_gid,
        gecos: unsafe { super::cstr(pw.pw_gecos) },
        dir: unsafe { super::cstr(pw.pw_dir) },
        shell: unsafe { super::cstr(pw.pw_shell) },
        #[cfg(target_vendor = "apple")]
        change: pw.pw_change,
        #[cfg(target_vendor = "apple")]
        uclass: unsafe { super::cstr(pw.pw_class) },
        #[cfg(target_vendor = "apple")]
        expire: pw.pw_expire,
    }))
}

ruby_class! {
    Passwd = zeo_abi::ETC_PASSWD_CLASS < zeo_abi::OBJECT_CLASS;

    def "name"(recv) { Ok(str_val(recv_passwd(recv).name.clone())) }
    def "passwd"(recv) { Ok(str_val(recv_passwd(recv).passwd.clone())) }
    def "uid"(recv) { Ok(RubyValue::Int(recv_passwd(recv).uid as i64)) }
    def "gid"(recv) { Ok(RubyValue::Int(recv_passwd(recv).gid as i64)) }
    def "gecos"(recv) { Ok(str_val(recv_passwd(recv).gecos.clone())) }
    def "dir"(recv) { Ok(str_val(recv_passwd(recv).dir.clone())) }
    def "shell"(recv) { Ok(str_val(recv_passwd(recv).shell.clone())) }

    // The BSD/Darwin-only accessors -- the `#[cfg]` gates the fn AND its
    // lookup/names/arity rows, so on glibc they simply don't exist.
    #[cfg(target_vendor = "apple")]
    def "change"(recv) { Ok(recv_passwd(recv).field("change").unwrap()) }
    #[cfg(target_vendor = "apple")]
    def "uclass"(recv) { Ok(recv_passwd(recv).field("uclass").unwrap()) }
    #[cfg(target_vendor = "apple")]
    def "expire"(recv) { Ok(recv_passwd(recv).field("expire").unwrap()) }

    def "members"(_recv) { Ok(members_array(PASSWD_MEMBERS)) }
    def "to_a" | "values"(recv) { Ok(struct_to_a(recv_passwd(recv), PASSWD_MEMBERS)) }
    def "to_h"(recv) { Ok(struct_to_h(recv_passwd(recv), PASSWD_MEMBERS)) }
    def "each"(recv, &block) { struct_each(recv_passwd(recv), PASSWD_MEMBERS, block, recv) }
    def "[]"(recv, arg) { struct_index(recv_passwd(recv), PASSWD_MEMBERS, arg) }
    def "to_s" | "inspect"(recv) { Ok(str_val(struct_inspect("Etc::Passwd", recv_passwd(recv), PASSWD_MEMBERS))) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Symbol;

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
        let RubyValue::Array(a) = to_a else {
            panic!("to_a is an Array")
        };
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
        assert_eq!(
            struct_index(&pw, PASSWD_MEMBERS, &RubyValue::Int(0))
                .unwrap()
                .inspect_string(),
            "\"alice\""
        );
        // Negative index counts from the end (shell is member index 6).
        assert_eq!(
            struct_index(
                &pw,
                PASSWD_MEMBERS,
                &RubyValue::Int(-(PASSWD_MEMBERS.len() as i64))
            )
            .unwrap()
            .inspect_string(),
            "\"alice\""
        );
        assert_eq!(
            struct_index(
                &pw,
                PASSWD_MEMBERS,
                &RubyValue::Symbol(Symbol::intern("uid"))
            )
            .unwrap()
            .inspect_string(),
            "1000"
        );
        // (Out-of-range / unknown-member RAISES are covered by the e2e tests,
        // which run with the full class registry; constructing an exception in
        // this registry-less unit context would panic.)
    }

    #[test]
    fn inspect_has_the_struct_shape() {
        let s = struct_inspect("Etc::Passwd", &a_passwd(), PASSWD_MEMBERS);
        assert!(
            s.starts_with("#<struct Etc::Passwd name=\"alice\", passwd=\"*\", uid=1000"),
            "{s}"
        );
    }

    // The `#[cfg]`-on-`def` DSL feature: the BSD-only accessors are in the
    // registered instance surface on Darwin and absent elsewhere -- proving the
    // cfg gates the fn AND its lookup/names rows together, per platform.
    #[test]
    fn bsd_accessors_track_the_platform_in_the_registered_surface() {
        let table = crate::builtins::registered_table(ETC_PASSWD_CLASS)
            .and_then(|t| t.instance.as_ref())
            .expect("Etc::Passwd registers an instance table");
        let present =
            |name: &str| (table.lookup)(name).is_some() && (table.names)().contains(&name);
        // Portable accessors are always there.
        assert!(present("name") && present("shell"));
        let expected = cfg!(target_vendor = "apple");
        assert_eq!(present("change"), expected);
        assert_eq!(present("uclass"), expected);
        assert_eq!(present("expire"), expected);
    }
}

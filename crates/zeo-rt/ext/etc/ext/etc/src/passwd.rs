//! `Etc::Passwd` -- one password-database row. A real `Struct` subclass, as
//! `rb_struct_define` makes it in CRuby, so `to_a`/`to_h`/`==`/`each`/`[]`/
//! `dig`/`size`/`deconstruct`/`Marshal` are all inherited: this file supplies
//! only the member list, the per-member accessors, and the two `hidden_ivar`
//! hooks the struct protocol reads a member by.
//!
//! The BSD/Darwin `struct passwd` carries three extra fields CRuby exposes as
//! `change`/`uclass`/`expire`; glibc's does not, so both the members and their
//! accessor `def`s are `#[cfg(target_vendor = "apple")]`-gated -- the `#[cfg]`
//! on a `def` drops the method from the lookup/names/arity surface as a unit,
//! exactly matching CRuby's platform-dependent method set.

use std::sync::Arc;

use parking_lot::Mutex;

use super::str_val;
use crate::RubyValue;
use crate::dispatch::{RObj, RubyObject};
use zeo_abi::{ClassId, ETC_PASSWD_CLASS};
use zeo_macros::ruby_class;

pub(crate) struct RPasswd {
    /// The members in `Struct` order, mutable behind the `Arc` every `RObj`
    /// lives in -- a Struct has writers.
    slots: Mutex<Vec<RubyValue>>,
}

#[cfg(target_vendor = "apple")]
pub(crate) const PASSWD_MEMBERS: &[&str] = &[
    "name", "passwd", "uid", "gid", "gecos", "dir", "shell", "change", "uclass", "expire",
];
#[cfg(not(target_vendor = "apple"))]
pub(crate) const PASSWD_MEMBERS: &[&str] =
    &["name", "passwd", "uid", "gid", "gecos", "dir", "shell"];

impl RPasswd {
    fn get(&self, at: usize) -> RubyValue {
        self.slots.lock()[at].clone()
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
    // The struct protocol reaches a member by INDEX through these two -- see
    // `RubyObject::hidden_ivar_get`. Implementing them is the whole of what
    // makes this a real Struct.
    fn hidden_ivar_get(&self, i: usize) -> Option<RubyValue> {
        (i < PASSWD_MEMBERS.len()).then(|| self.get(i))
    }
    fn hidden_ivar_set(&self, i: usize, v: RubyValue) -> bool {
        let ok = i < PASSWD_MEMBERS.len();
        if ok {
            self.slots.lock()[i] = v;
        }
        ok
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(RPasswd {
            slots: Mutex::new(self.slots.lock().clone()),
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

fn set(recv: &RubyValue, at: usize, v: &RubyValue) -> Result<RubyValue, crate::Signal> {
    recv_passwd(recv).slots.lock()[at] = v.clone();
    Ok(v.clone())
}

/// Copy a libc `struct passwd` into an owned `Etc::Passwd` value.
///
/// # Safety
///
/// `pw` is a non-NULL `struct passwd` libc filled, with its string fields
/// still valid -- so before the next call into the passwd database.
pub(crate) unsafe fn passwd_from(pw: *const libc::passwd) -> RubyValue {
    let pw = unsafe { &*pw };
    #[allow(unused_mut)] // pushed to under cfg(target_vendor = "apple") only
    let mut slots = vec![
        str_val(unsafe { super::cstr(pw.pw_name) }),
        str_val(unsafe { super::cstr(pw.pw_passwd) }),
        RubyValue::Int(i64::from(pw.pw_uid)),
        RubyValue::Int(i64::from(pw.pw_gid)),
        str_val(unsafe { super::cstr(pw.pw_gecos) }),
        str_val(unsafe { super::cstr(pw.pw_dir) }),
        str_val(unsafe { super::cstr(pw.pw_shell) }),
    ];
    #[cfg(target_vendor = "apple")]
    slots.extend([
        RubyValue::Int(pw.pw_change),
        str_val(unsafe { super::cstr(pw.pw_class) }),
        RubyValue::Int(pw.pw_expire),
    ]);
    RubyValue::Object(Arc::new(RPasswd {
        slots: Mutex::new(slots),
    }))
}

/// `Etc::Passwd[..]` / `.new(..)` -- the Struct constructor, which CRuby
/// leaves on the class because `rb_struct_define` put it there. Fewer
/// arguments than members leaves the rest nil.
pub(crate) fn passwd_construct(args: &[RubyValue]) -> Result<RubyValue, crate::Signal> {
    if args.len() > PASSWD_MEMBERS.len() {
        return Err(crate::builtins::arg_error!("struct size differs"));
    }
    let mut slots = vec![RubyValue::Nil; PASSWD_MEMBERS.len()];
    for (slot, v) in slots.iter_mut().zip(args) {
        *slot = v.clone();
    }
    Ok(RubyValue::Object(Arc::new(RPasswd {
        slots: Mutex::new(slots),
    })))
}

/// A blank `Etc::Passwd` -- every member nil, which is what ruby's own
/// `allocate` answers for a Struct subclass.
fn passwd_allocate() -> RubyValue {
    passwd_construct(&[]).expect("no arguments is always within the member count")
}

ruby_class! {
    Passwd = zeo_abi::ETC_PASSWD_CLASS < zeo_abi::STRUCT_CLASS;

    allocate passwd_allocate;

    // The class methods a `Struct` subclass carries in its OWN singleton --
    // `rb_struct_define` installs them there, so reflection reports them as
    // this class's rather than `Struct`'s.
    def self."[]" cfunc (_recv, *args, &_block) {
        passwd_construct(args)
    }
    def self."new" cfunc (_recv, *args, &_block) {
        passwd_construct(args)
    }
    // Never keyword-initialized: `rb_struct_define` builds it positionally.
    def self."keyword_init?"(_recv) {
        Ok(RubyValue::Nil)
    }
    // A Struct subclass re-declares `inspect` on its own singleton, where it
    // answers exactly what `Module#inspect` answers. Declaring it here is what
    // puts the name in `singleton_methods(false)`.
    def self."inspect"(recv) {
        crate::builtins::inherited_row!(rmodule, "inspect", recv, __args, None)
    }
    // `Etc::Passwd.each` -- Etc's own cursor over the whole database, the
    // same walk `Etc.passwd` runs with a block.
    def self."each"(_recv, &block) {
        super::each_passwd(block)
    }

    def "name"(recv) { Ok(recv_passwd(recv).get(0)) }
    def "name=" params "_"(recv, v) { set(recv, 0, v) }
    def "passwd"(recv) { Ok(recv_passwd(recv).get(1)) }
    def "passwd=" params "_"(recv, v) { set(recv, 1, v) }
    def "uid"(recv) { Ok(recv_passwd(recv).get(2)) }
    def "uid=" params "_"(recv, v) { set(recv, 2, v) }
    def "gid"(recv) { Ok(recv_passwd(recv).get(3)) }
    def "gid=" params "_"(recv, v) { set(recv, 3, v) }
    def "gecos"(recv) { Ok(recv_passwd(recv).get(4)) }
    def "gecos=" params "_"(recv, v) { set(recv, 4, v) }
    def "dir"(recv) { Ok(recv_passwd(recv).get(5)) }
    def "dir=" params "_"(recv, v) { set(recv, 5, v) }
    def "shell"(recv) { Ok(recv_passwd(recv).get(6)) }
    def "shell=" params "_"(recv, v) { set(recv, 6, v) }

    // The BSD/Darwin-only accessors -- the `#[cfg]` gates the fn AND its
    // lookup/names/arity rows, so on glibc they simply don't exist.
    #[cfg(target_vendor = "apple")]
    def "change"(recv) { Ok(recv_passwd(recv).get(7)) }
    #[cfg(target_vendor = "apple")]
    def "change=" params "_"(recv, v) { set(recv, 7, v) }
    #[cfg(target_vendor = "apple")]
    def "uclass"(recv) { Ok(recv_passwd(recv).get(8)) }
    #[cfg(target_vendor = "apple")]
    def "uclass=" params "_"(recv, v) { set(recv, 8, v) }
    #[cfg(target_vendor = "apple")]
    def "expire"(recv) { Ok(recv_passwd(recv).get(9)) }
    #[cfg(target_vendor = "apple")]
    def "expire=" params "_"(recv, v) { set(recv, 9, v) }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl RPasswd {
        fn field(&self, name: &str) -> Option<RubyValue> {
            PASSWD_MEMBERS
                .iter()
                .position(|&m| m == name)
                .map(|i| self.get(i))
        }
    }

    fn a_passwd() -> RPasswd {
        #[allow(unused_mut)] // pushed to under cfg(target_vendor = "apple") only
        let mut slots = vec![
            str_val("alice".into()),
            str_val("*".into()),
            RubyValue::Int(1000),
            RubyValue::Int(2000),
            str_val("Alice".into()),
            str_val("/home/alice".into()),
            str_val("/bin/zsh".into()),
        ];
        #[cfg(target_vendor = "apple")]
        slots.extend([RubyValue::Int(0), str_val(String::new()), RubyValue::Int(0)]);
        RPasswd {
            slots: Mutex::new(slots),
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

    /// The two hooks the whole inherited protocol reads a member by: index in
    /// range answers, out of range declines, and a write is visible to the
    /// next read.
    #[test]
    fn hidden_slots_are_the_members_in_order() {
        let pw = a_passwd();
        assert_eq!(pw.hidden_ivar_get(0).unwrap().inspect_string(), "\"alice\"");
        assert_eq!(pw.hidden_ivar_get(2).unwrap().inspect_string(), "1000");
        assert!(pw.hidden_ivar_get(PASSWD_MEMBERS.len()).is_none());
        assert!(pw.hidden_ivar_set(2, RubyValue::Int(7)));
        assert_eq!(pw.hidden_ivar_get(2).unwrap().inspect_string(), "7");
        assert!(!pw.hidden_ivar_set(PASSWD_MEMBERS.len(), RubyValue::Nil));
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
        assert!(present("name=") && present("shell="));
        let expected = cfg!(target_vendor = "apple");
        assert_eq!(present("change"), expected);
        assert_eq!(present("uclass"), expected);
        assert_eq!(present("expire"), expected);
        assert_eq!(present("expire="), expected);
    }
}

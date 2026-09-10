//! `Etc::Group` -- one group-database row. A real `Struct` subclass, as
//! `rb_struct_define` makes it in CRuby: the protocol rows are inherited and
//! this file supplies the member list, the accessors, and the two
//! `hidden_ivar` hooks the protocol reads a member by. See `passwd.rs`, whose
//! shape this follows exactly.

use std::sync::Arc;

use parking_lot::Mutex;

use super::str_val;
use crate::RubyValue;
use crate::dispatch::{RObj, RubyObject};
use zeo_abi::{ClassId, ETC_GROUP_CLASS};
use zeo_macros::ruby_class;

pub(crate) struct RGroup {
    slots: Mutex<Vec<RubyValue>>,
}

pub(crate) const GROUP_MEMBERS: &[&str] = &["name", "passwd", "gid", "mem"];

impl RGroup {
    fn get(&self, at: usize) -> RubyValue {
        self.slots.lock()[at].clone()
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
    fn hidden_ivar_get(&self, i: usize) -> Option<RubyValue> {
        (i < GROUP_MEMBERS.len()).then(|| self.get(i))
    }
    fn hidden_ivar_set(&self, i: usize, v: RubyValue) -> bool {
        let ok = i < GROUP_MEMBERS.len();
        if ok {
            self.slots.lock()[i] = v;
        }
        ok
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(RGroup {
            slots: Mutex::new(self.slots.lock().clone()),
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

fn set(recv: &RubyValue, at: usize, v: &RubyValue) -> Result<RubyValue, crate::Signal> {
    recv_group(recv).slots.lock()[at] = v.clone();
    Ok(v.clone())
}

/// Copy a libc `struct group` into an owned `Etc::Group` value.
///
/// # Safety
///
/// `gr` is a non-NULL `struct group` libc filled, with its string fields and
/// its NULL-terminated `gr_mem` still valid -- so before the next call into
/// the group database.
pub(crate) unsafe fn group_from(gr: *const libc::group) -> RubyValue {
    let gr = unsafe { &*gr };
    let mut mem = Vec::new();
    if !gr.gr_mem.is_null() {
        let mut p = gr.gr_mem;
        while !unsafe { *p }.is_null() {
            mem.push(str_val(unsafe { super::cstr(*p) }));
            p = unsafe { p.add(1) };
        }
    }
    RubyValue::Object(Arc::new(RGroup {
        slots: Mutex::new(vec![
            str_val(unsafe { super::cstr(gr.gr_name) }),
            str_val(unsafe { super::cstr(gr.gr_passwd) }),
            RubyValue::Int(i64::from(gr.gr_gid)),
            RubyValue::Array(crate::array_new(mem)),
        ]),
    }))
}

/// `Etc::Group[..]` / `.new(..)` -- see `passwd::passwd_construct`.
pub(crate) fn group_construct(args: &[RubyValue]) -> Result<RubyValue, crate::Signal> {
    if args.len() > GROUP_MEMBERS.len() {
        return Err(crate::builtins::arg_error!("struct size differs"));
    }
    let mut slots = vec![RubyValue::Nil; GROUP_MEMBERS.len()];
    for (slot, v) in slots.iter_mut().zip(args) {
        *slot = v.clone();
    }
    Ok(RubyValue::Object(Arc::new(RGroup {
        slots: Mutex::new(slots),
    })))
}

ruby_class! {
    Group = zeo_abi::ETC_GROUP_CLASS < zeo_abi::STRUCT_CLASS;

    // The Struct class methods, on this class's own singleton -- see
    // `passwd.rs` for why they are declared rather than inherited.
    def self."[]" cfunc (_recv, *args, &_block) {
        group_construct(args)
    }
    def self."new" cfunc (_recv, *args, &_block) {
        group_construct(args)
    }
    def self."keyword_init?"(_recv) {
        Ok(RubyValue::Nil)
    }
    def self."inspect"(recv) {
        crate::builtins::inherited_row!(rmodule, "inspect", recv, __args, None)
    }
    // `Etc::Group.each` -- the cursor over the whole group database.
    def self."each"(_recv, &block) {
        super::each_group(block)
    }

    def "name"(recv) { Ok(recv_group(recv).get(0)) }
    def "name=" params "_"(recv, v) { set(recv, 0, v) }
    def "passwd"(recv) { Ok(recv_group(recv).get(1)) }
    def "passwd=" params "_"(recv, v) { set(recv, 1, v) }
    def "gid"(recv) { Ok(recv_group(recv).get(2)) }
    def "gid=" params "_"(recv, v) { set(recv, 2, v) }
    def "mem"(recv) { Ok(recv_group(recv).get(3)) }
    def "mem=" params "_"(recv, v) { set(recv, 3, v) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_slots_are_the_members_in_order() {
        let g = RGroup {
            slots: Mutex::new(vec![
                str_val("staff".into()),
                str_val("*".into()),
                RubyValue::Int(20),
                RubyValue::Array(crate::array_new(vec![str_val("alice".into())])),
            ]),
        };
        assert_eq!(g.hidden_ivar_get(0).unwrap().inspect_string(), "\"staff\"");
        assert_eq!(g.hidden_ivar_get(2).unwrap().inspect_string(), "20");
        assert!(g.hidden_ivar_get(GROUP_MEMBERS.len()).is_none());
        assert!(g.hidden_ivar_set(2, RubyValue::Int(21)));
        assert_eq!(g.hidden_ivar_get(2).unwrap().inspect_string(), "21");
    }
}

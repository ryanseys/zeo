//! `Etc::Group` -- one group-database row, a Struct-like value object mirroring
//! `Etc::Passwd` (`passwd.rs`). Its member set is platform-independent.

use std::sync::Arc;

use super::{
    StructRow, members_array, str_val, struct_each, struct_index, struct_inspect, struct_to_a,
    struct_to_h,
};
use crate::RubyValue;
use crate::dispatch::{RObj, RubyObject};
use zeo_abi::{ClassId, ETC_GROUP_CLASS};
use zeo_macros::ruby_class;

pub(crate) struct RGroup {
    name: String,
    passwd: String,
    gid: u32,
    mem: Vec<String>,
}

pub(crate) const GROUP_MEMBERS: &[&str] = &["name", "passwd", "gid", "mem"];

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

impl StructRow for RGroup {
    fn field(&self, n: &str) -> Option<RubyValue> {
        RGroup::field(self, n)
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

/// Copy a libc `struct group` into an owned `Etc::Group` value.
pub(crate) unsafe fn group_from(gr: *const libc::group) -> RubyValue {
    let gr = unsafe { &*gr };
    let mut mem = Vec::new();
    if !gr.gr_mem.is_null() {
        let mut p = gr.gr_mem;
        while !unsafe { *p }.is_null() {
            mem.push(unsafe { super::cstr(*p) });
            p = unsafe { p.add(1) };
        }
    }
    RubyValue::Object(Arc::new(RGroup {
        name: unsafe { super::cstr(gr.gr_name) },
        passwd: unsafe { super::cstr(gr.gr_passwd) },
        gid: gr.gr_gid,
        mem,
    }))
}

ruby_class! {
    Group = zeo_abi::ETC_GROUP_CLASS < zeo_abi::OBJECT_CLASS;

    def "name"(recv) { Ok(str_val(recv_group(recv).name.clone())) }
    def "passwd"(recv) { Ok(str_val(recv_group(recv).passwd.clone())) }
    def "gid"(recv) { Ok(RubyValue::Int(recv_group(recv).gid as i64)) }
    def "mem"(recv) { Ok(recv_group(recv).field("mem").unwrap()) }
    def "members"(_recv) { Ok(members_array(GROUP_MEMBERS)) }
    def "to_a" | "values"(recv) { Ok(struct_to_a(recv_group(recv), GROUP_MEMBERS)) }
    def "to_h"(recv) { Ok(struct_to_h(recv_group(recv), GROUP_MEMBERS)) }
    def "each"(recv, &block) { struct_each(recv_group(recv), GROUP_MEMBERS, block, recv) }
    def "[]"(recv, arg) { struct_index(recv_group(recv), GROUP_MEMBERS, arg) }
    def "to_s" | "inspect"(recv) { Ok(str_val(struct_inspect("Etc::Group", recv_group(recv), GROUP_MEMBERS))) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn members_are_symbols() {
        let RubyValue::Array(a) = members_array(GROUP_MEMBERS) else {
            panic!()
        };
        assert_eq!(
            a.lock()
                .to_vec()
                .iter()
                .map(|v| v.inspect_string())
                .collect::<Vec<_>>(),
            vec![":name", ":passwd", ":gid", ":mem"]
        );
    }
}

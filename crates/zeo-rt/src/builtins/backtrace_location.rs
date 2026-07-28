//! `Thread::Backtrace::Location` -- one backtrace entry as an OBJECT rather
//! than a formatted string, which is what `Kernel#caller_locations` answers.
//!
//! Nothing here is computed: a `Frame` already carries the file, line and
//! method label a location reports, so this is that triple boxed as a Ruby
//! object with CRuby's accessor names. `forwardable` reads
//! `caller_locations(2, 1).first.path` to build its deprecation message, which
//! is the shape that made the string approximation insufficient.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::builtins::arity;
use crate::dispatch::{RObj, RubyObject};
use crate::{RubyValue, string_new};
use zeo_abi::BACKTRACE_LOCATION_CLASS;
use zeo_macros::ruby_class;

pub struct BacktraceLocation {
    path: String,
    lineno: i64,
    label: String,
    frozen: AtomicBool,
}

impl RubyObject for BacktraceLocation {
    fn class_id(&self) -> crate::ClassId {
        BACKTRACE_LOCATION_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Relaxed)
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        Arc::new(BacktraceLocation {
            path: self.path.clone(),
            lineno: self.lineno,
            label: self.label.clone(),
            frozen: AtomicBool::new(copy_frozen && self.is_frozen()),
        })
    }
}

/// One location from a frame triple -- the only way these are minted (there is
/// no `Location.new` in Ruby either).
pub fn location_new(path: &str, lineno: u32, label: &str) -> RubyValue {
    RubyValue::Object(Arc::new(BacktraceLocation {
        path: path.to_string(),
        lineno: i64::from(lineno),
        label: label.to_string(),
        frozen: AtomicBool::new(false),
    }))
}

fn loc_of(recv: &RubyValue) -> &BacktraceLocation {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<BacktraceLocation>()
            .expect("the Location table only dispatches on Location receivers"),
        _ => unreachable!("the Location table only dispatches on Location receivers"),
    }
}

/// `FILE:LINE:in 'LABEL'` -- the same rendering `caller` gives as a string, so
/// a location and its string form always agree.
fn rendered(loc: &BacktraceLocation) -> String {
    format!("{}:{}:in '{}'", loc.path, loc.lineno, loc.label)
}

ruby_class! {
    BacktraceLocationClass = zeo_abi::BACKTRACE_LOCATION_CLASS < zeo_abi::OBJECT_CLASS;

    def "path"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(string_new(loc_of(recv).path.clone())))
    }
    // zeo's frames record the path the compiler saw, which is already absolute
    // for every spliced file -- so the two answers coincide here.
    def "absolute_path"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(string_new(loc_of(recv).path.clone())))
    }
    def "lineno"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(loc_of(recv).lineno))
    }
    def "label"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(string_new(loc_of(recv).label.clone())))
    }
    // `base_label` is the bare method NAME: `label` minus the `block in` /
    // `block (2 levels) in` prefix a nested block carries, and minus the
    // `Klass#` / `Klass.` owner qualification. A synthetic label
    // (`<main>`, `<class:Foo>`) is already bare and passes through.
    def "base_label"(recv, args, _block) {
        arity!(args, 0);
        let label = &loc_of(recv).label;
        let base = label.rsplit_once(" in ").map_or(label.as_str(), |(_, m)| m);
        let base = match base.starts_with('<') {
            true => base,
            false => base.rsplit(['#', '.']).next().unwrap_or(base),
        };
        Ok(RubyValue::Str(string_new(base.to_string())))
    }
    def "to_s"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(string_new(rendered(loc_of(recv)))))
    }
    def "inspect"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(string_new(format!("{:?}", rendered(loc_of(recv))))))
    }
}

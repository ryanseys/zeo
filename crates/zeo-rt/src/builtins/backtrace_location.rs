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

use crate::dispatch::{RObj, RubyObject};
use crate::{RubyValue, string_new};
use zeo_abi::BACKTRACE_LOCATION_CLASS;
use zeo_macros::ruby_class;

pub struct BacktraceLocation {
    path: String,
    lineno: i64,
    label: String,
    /// The method this frame INVOKED, when the builder could determine it.
    ///
    /// No Ruby row reads it, and CRuby's `Location` has none either -- a
    /// public row here would be a surface divergence. It exists for
    /// `RubyVM::AbstractSyntaxTree.node_id_for_backtrace_location`, whose
    /// rule needs it: a location's node is the CALL at (path, lineno) whose
    /// NAME is the method that location invoked, and a location alone cannot
    /// say which call on its line that is.
    ///
    /// `None` for a builder that cannot answer, and the primitive raises
    /// `ArgumentError` there rather than guessing.
    callee: Option<String>,
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
            callee: self.callee.clone(),
            frozen: AtomicBool::new(copy_frozen && self.is_frozen()),
        })
    }
}

/// One location from a frame triple -- the only way these are minted (there is
/// no `Location.new` in Ruby either).
pub fn location_new(path: &str, lineno: u32, label: &str) -> RubyValue {
    location_with_callee(path, lineno, label, None)
}

/// The same, carrying the method this frame invoked -- see
/// [`BacktraceLocation::callee`].
pub fn location_with_callee(
    path: &str,
    lineno: u32,
    label: &str,
    callee: Option<String>,
) -> RubyValue {
    RubyValue::Object(Arc::new(BacktraceLocation {
        path: path.to_string(),
        lineno: i64::from(lineno),
        label: label.to_string(),
        callee,
        frozen: AtomicBool::new(false),
    }))
}

/// The method name inside a frame LABEL: `Object#inner` and `Foo.bar` name
/// `inner` and `bar`, `block in outer` names `outer`, and `<main>` or
/// `<class:Foo>` name nothing a call site can be written as.
pub fn label_method(label: &str) -> Option<String> {
    let label = label.rsplit("block in ").next().unwrap_or(label);
    let name = label
        .rsplit_once(['#', '.'])
        .map_or(label, |(_, name)| name)
        .trim();
    (!name.is_empty() && !name.starts_with('<')).then(|| name.to_string())
}

/// The callee a location carries, if its builder knew one.
pub fn callee_of(v: &RubyValue) -> Option<String> {
    match v {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<BacktraceLocation>()
            .and_then(|l| l.callee.clone()),
        _ => None,
    }
}

/// `(path, lineno)` of a location, for a caller that is not a Ruby row.
pub fn place_of(v: &RubyValue) -> Option<(String, i64)> {
    match v {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<BacktraceLocation>()
            .map(|l| (l.path.clone(), l.lineno)),
        _ => None,
    }
}

/// Fills in each location's callee from the list itself: location `i`'s
/// callee is the method named by location `i-1`'s label -- the frame it
/// called into. `first` is location 0's, which the list cannot supply.
///
/// Oracle-verified against ruby 4.0.6 over `def outer = inner` /
/// `def inner = raise`, `[1,2].fetch(9)` (whose cfunc location and its
/// caller name the same call) and `Integer("zz")`.
pub fn thread_callees(rows: &[(String, u32, String)], first: Option<String>) -> Vec<RubyValue> {
    rows.iter()
        .enumerate()
        .map(|(i, (path, lineno, label))| {
            let callee = if i == 0 {
                first.clone()
            } else {
                label_method(&rows[i - 1].2)
            };
            location_with_callee(path, *lineno, label, callee)
        })
        .collect()
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
    // `Thread::Backtrace` -- the namespace `Location` nests under, and the
    // one class method CRuby puts on it. It holds no instances: a backtrace
    // is an Array of locations, not an object of this class.
    Backtrace = zeo_abi::BACKTRACE_CLASS < zeo_abi::OBJECT_CLASS;

    // `--backtrace-limit`, which zeo does not accept, so the answer is
    // CRuby's own default: unlimited.
    def self."limit"(_recv) {
        Ok(RubyValue::Int(-1))
    }
}

ruby_class! {
    Location = zeo_abi::BACKTRACE_LOCATION_CLASS < zeo_abi::OBJECT_CLASS;

    def "path"(recv) {
        Ok(RubyValue::Str(string_new(loc_of(recv).path.clone())))
    }
    // zeo's frames record the path the compiler saw, which is already absolute
    // for every spliced file -- so the two answers coincide here.
    def "absolute_path"(recv) {
        Ok(RubyValue::Str(string_new(loc_of(recv).path.clone())))
    }
    def "lineno"(recv) {
        Ok(RubyValue::Int(loc_of(recv).lineno))
    }
    def "label"(recv) {
        Ok(RubyValue::Str(string_new(loc_of(recv).label.clone())))
    }
    // `base_label` is the bare method NAME: `label` minus the `block in` /
    // `block (2 levels) in` prefix a nested block carries, and minus the
    // `Klass#` / `Klass.` owner qualification. A synthetic label
    // (`<main>`, `<class:Foo>`) is already bare and passes through.
    def "base_label"(recv) {
        let label = &loc_of(recv).label;
        let base = label.rsplit_once(" in ").map_or(label.as_str(), |(_, m)| m);
        let base = match base.starts_with('<') {
            true => base,
            false => base.rsplit(['#', '.']).next().unwrap_or(base),
        };
        Ok(RubyValue::Str(string_new(base.to_string())))
    }
    def "to_s"(recv) {
        Ok(RubyValue::Str(string_new(rendered(loc_of(recv)))))
    }
    def "inspect"(recv) {
        Ok(RubyValue::Str(string_new(format!("{:?}", rendered(loc_of(recv))))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One Location row, called straight off the builtin table -- the rows
    /// are pure accessors, so no registry is needed.
    fn row(recv: &RubyValue, name: &str) -> String {
        let lookup =
            crate::builtins::class_table(BACKTRACE_LOCATION_CLASS).expect("Location has a table");
        let f = lookup(name).expect("the row exists");
        match f(recv, &[], None) {
            Ok(RubyValue::Str(s)) => s.lock().to_utf8_lossy().into_owned(),
            Ok(RubyValue::Int(n)) => n.to_string(),
            _ => panic!("expected a Str or Int answer"),
        }
    }

    #[test]
    fn label_method_names_the_method_inside_a_frame_label() {
        assert_eq!(label_method("Object#inner").as_deref(), Some("inner"));
        assert_eq!(label_method("Foo.bar").as_deref(), Some("bar"));
        assert_eq!(label_method("block in outer").as_deref(), Some("outer"));
        assert_eq!(label_method("<main>"), None);
        assert_eq!(label_method("<class:Foo>"), None);
    }

    #[test]
    fn a_location_reports_its_triple_and_renders_like_caller() {
        let loc = location_new("/a.rb", 3, "Object#inner");
        assert_eq!(row(&loc, "path"), "/a.rb");
        assert_eq!(row(&loc, "absolute_path"), "/a.rb");
        assert_eq!(row(&loc, "lineno"), "3");
        assert_eq!(row(&loc, "label"), "Object#inner");
        assert_eq!(row(&loc, "to_s"), "/a.rb:3:in 'Object#inner'");
    }

    #[test]
    fn base_label_strips_the_block_prefix_and_the_owner() {
        let cases = [
            ("Object#inner", "inner"),
            ("Foo.bar", "bar"),
            ("block (2 levels) in Object#outer", "outer"),
            ("<main>", "<main>"),
        ];
        for (label, want) in cases {
            let loc = location_new("/a.rb", 1, label);
            assert_eq!(row(&loc, "base_label"), want, "for label {label:?}");
        }
    }

    #[test]
    fn thread_callees_take_the_previous_frames_label() {
        let rows = vec![
            ("/a.rb".to_string(), 1, "Object#inner".to_string()),
            ("/a.rb".to_string(), 5, "Object#outer".to_string()),
        ];
        let locs = thread_callees(&rows, Some("raise".to_string()));
        // Location 0's callee is handed in; location 1's is what location 0
        // invoked -- the method its label names.
        assert_eq!(callee_of(&locs[0]).as_deref(), Some("raise"));
        assert_eq!(callee_of(&locs[1]).as_deref(), Some("inner"));
    }
}

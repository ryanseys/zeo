//! `@x` written inside a class method (`def self.x`, a `class << self` body,
//! or a module function) -- an instance variable of the CLASS OBJECT itself,
//! which is a different thing from both `@@x` and an instance's own `@x`.
//!
//! Keyed by `(class_id, name)`, exactly like `cvars`' table -- but the class
//! id means something DIFFERENT here, and the difference is the entire
//! reason this is a separate module rather than another `cvars` caller:
//!
//!   - a `@@x` reference resolves to a single OWNER at compile time
//!     (`analyze::mro::resolve_cvars` walks the ancestors for whoever
//!     claimed the name first), so a subclass and its superclass SHARE one
//!     slot;
//!   - a class-level `@x` is per-class-object storage and is NOT inherited,
//!     so the key is the RECEIVER class, and a subclass gets its own,
//!     independent slot.
//!
//! Oracle-verified (ruby 4.0.5), which is what pinned the distinction down:
//!
//! ```ruby
//! class Base
//!   @reg = "base-ivar"
//!   @@cv = "base-cvar"
//!   def self.reg = @reg
//!   def self.cv  = @@cv
//! end
//! class Sub < Base; end
//! Sub.reg  # => nil            -- NOT inherited: its own empty slot
//! Sub.cv   # => "base-cvar"    -- inherited: Base owns the one slot
//! ```
//!
//! Getting the receiver class right costs nothing at runtime: zeo
//! materializes an inherited class method as a separate copy per subclass
//! (`analyze::mro::materialize_class_methods`), so each emitted copy knows
//! its own receiver statically and passes a literal class id in --
//! `Sub::reg`'s body is compiled with `Sub`'s id, `Base::reg`'s with
//! `Base`'s, and no runtime receiver lookup is needed to keep them apart.
//!
//! Process-wide-shared, not per-thread, for the same reason `cvars` is (see
//! that module's docs): a class-level `@count` bumped by one `Thread` must
//! be visible to another.

use crate::RubyValue;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::LazyLock;

static CIVARS: LazyLock<Mutex<HashMap<(u32, String), RubyValue>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// `nil` for a class-level `@x` never yet written -- and here that is real
/// Ruby's ACTUAL behavior, not this runtime's usual approximation of it:
/// reading a never-assigned instance variable genuinely answers `nil` (only
/// `@@x` raises a NameError, and only warns for an ivar under `-w`).
/// Oracle-checked: `class C; def self.probe = @never_written; end; C.probe`
/// is `nil`.
pub fn class_ivar_get(class_id: u32, name: &str) -> RubyValue {
    CIVARS
        .lock()
        .get(&(class_id, name.to_string()))
        .cloned()
        .unwrap_or(RubyValue::Nil)
}

/// Fallible because a FROZEN class refuses the write (`can't modify frozen
/// Class: Foo`, CRuby's `rb_check_frozen` on the receiver class -- checked
/// here so every path in, static or reflective, gets the same guard).
pub fn class_ivar_set(class_id: u32, name: &str, value: RubyValue) -> Result<(), crate::Signal> {
    if crate::dispatch::class_frozen(crate::ClassId(class_id)) {
        return Err(crate::dispatch::frozen_class_error(crate::ClassId(
            class_id,
        )));
    }
    CIVARS.lock().insert((class_id, name.to_string()), value);
    Ok(())
}

/// The class-level ivar names with a value, in sorted order -- backs
/// `Class#instance_variables`. Sorted rather than definition-ordered: the
/// `HashMap` has no insertion order to report, and a stable answer beats a
/// nondeterministic one. Real Ruby answers in first-assignment order, so a
/// class assigning `@b` before `@a` reports `[:@b, :@a]` where this reports
/// `[:@a, :@b]` -- a documented divergence, not worth a second side table
/// until something needs it.
pub fn class_ivar_names(class_id: u32) -> Vec<String> {
    let mut names: Vec<String> = CIVARS
        .lock()
        .keys()
        .filter(|(cid, _)| *cid == class_id)
        .map(|(_, n)| n.clone())
        .collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    // `RubyValue` has no `PartialEq` (Ruby equality is `rb_eq`, fallible and
    // dispatch-driven), so these read the Int back out rather than comparing
    // values directly.
    fn int_of(v: RubyValue) -> Option<i64> {
        match v {
            RubyValue::Int(i) => Some(i),
            _ => None,
        }
    }

    // The property the whole module exists for: same name, two classes, two
    // slots. `cvars`' table would answer 1 for both.
    #[test]
    fn storage_is_per_class_not_inherited() {
        class_ivar_set(900, "reg", RubyValue::Int(1)).unwrap();
        assert_eq!(int_of(class_ivar_get(900, "reg")), Some(1));
        assert!(matches!(class_ivar_get(901, "reg"), RubyValue::Nil));
        class_ivar_set(901, "reg", RubyValue::Int(2)).unwrap();
        assert_eq!(int_of(class_ivar_get(900, "reg")), Some(1));
        assert_eq!(int_of(class_ivar_get(901, "reg")), Some(2));
    }

    #[test]
    fn unwritten_reads_nil() {
        assert!(matches!(class_ivar_get(902, "never"), RubyValue::Nil));
    }

    #[test]
    fn names_are_scoped_to_their_class() {
        class_ivar_set(903, "b", RubyValue::Int(1)).unwrap();
        class_ivar_set(903, "a", RubyValue::Int(2)).unwrap();
        class_ivar_set(904, "z", RubyValue::Int(3)).unwrap();
        assert_eq!(class_ivar_names(903), vec!["a", "b"]);
        assert_eq!(class_ivar_names(904), vec!["z"]);
    }
}

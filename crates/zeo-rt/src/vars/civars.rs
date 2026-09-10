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
//! Oracle-verified (ruby 4.0.6), which is what pinned the distinction down:
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

use crate::FMap;
use crate::RubyValue;
use parking_lot::Mutex;
use std::sync::{LazyLock, OnceLock};

/// One class-level `@x`'s storage, shaped exactly like an instance ivar's
/// field in a `ruby_class!` struct: `Option` so `instance_variables` can tell
/// "never assigned" from "assigned nil", behind its own lock so a write
/// contends with nothing but the same name on the same class.
///
/// Slots are LEAKED (see [`intern`]) and therefore `&'static`. Nothing ever
/// removes a class-level ivar -- `remove_instance_variable` on a class writes
/// `nil` rather than unbinding -- so the leak is bounded by the number of
/// distinct `(class, name)` pairs the program mentions.
pub struct CivarSlot {
    value: Mutex<Option<RubyValue>>,
    /// When this slot was FIRST written, in program-wide order; `0` = never.
    ///
    /// `Class#instance_variables` reports first-assignment order, and the
    /// intern table cannot supply it: a name is interned the first time any
    /// emitted site MENTIONS it, which for a read-only `@x` happens before --
    /// possibly instead of -- a write. One relaxed load on the write path
    /// buys the order; the counter itself only ever moves on a first write.
    first_write: std::sync::atomic::AtomicU64,
}

/// Hands out [`CivarSlot::first_write`] stamps. Starts at 1 so `0` can mean
/// "never written".
static CIVAR_WRITE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl CivarSlot {
    #[inline]
    pub fn get(&self) -> RubyValue {
        if crate::gvl::sole_thread() {
            // SAFETY: the same argument [`crate::IvarCell`] makes for an
            // instance's slots. No other thread can reach this one while
            // `sole_thread` holds, and the borrow ends before the clone
            // returns -- nothing here calls Ruby, so nothing re-enters.
            return unsafe { (*self.value.data_ptr()).clone().unwrap_or(RubyValue::Nil) };
        }
        self.value.lock().clone().unwrap_or(RubyValue::Nil)
    }

    #[inline]
    fn put(&self, value: RubyValue) {
        use std::sync::atomic::Ordering;
        if self.first_write.load(Ordering::Relaxed) == 0 {
            let seq = CIVAR_WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
            // A racing second writer keeps the stamp the first one won.
            let _ = self
                .first_write
                .compare_exchange(0, seq, Ordering::Relaxed, Ordering::Relaxed);
        }
        if crate::gvl::sole_thread() {
            // SAFETY: as in `get`.
            unsafe { *self.value.data_ptr() = Some(value) };
            return;
        }
        *self.value.lock() = Some(value);
    }

    /// Clear the slot, answering what it held. The write stamp goes with the
    /// value, so `class_ivar_names` stops reporting a removed name -- an
    /// emptied slot is indistinguishable from one that was never assigned,
    /// which is what ruby reports after `remove_instance_variable`.
    fn take(&self) -> Option<RubyValue> {
        let held = self.value.lock().take();
        if held.is_some() {
            self.first_write
                .store(0, std::sync::atomic::Ordering::Relaxed);
        }
        held
    }

    /// The write stamp, or `None` for a slot interned by a READ and never
    /// assigned -- ruby reports only assigned names.
    fn write_seq(&self) -> Option<u64> {
        match self.first_write.load(std::sync::atomic::Ordering::Relaxed) {
            0 => None,
            n => Some(n),
        }
    }
}

/// Two-level (class -> name -> slot). Consulted once per emitted site, then
/// never again: the site caches the `&'static CivarSlot` it resolved.
static CIVARS: LazyLock<Mutex<crate::ClassScopedMap<&'static CivarSlot>>> =
    LazyLock::new(|| Mutex::new(FMap::default()));

fn intern(class_id: u32, name: &str) -> &'static CivarSlot {
    let mut table = CIVARS.lock();
    if let Some(slot) = table.get(&class_id).and_then(|m| m.get(name)) {
        return slot;
    }
    let slot: &'static CivarSlot = Box::leak(Box::new(CivarSlot {
        value: Mutex::new(None),
        first_write: std::sync::atomic::AtomicU64::new(0),
    }));
    table
        .entry(class_id)
        .or_default()
        .insert(Box::from(name), slot);
    slot
}

/// One emitted `@x` reference inside a class-method or class body, holding the
/// slot it resolves to. The compiler emits one `static` of this per site, so a
/// read costs a `OnceLock` load and the slot's own lock -- no global table, no
/// hashing, and no contention with an unrelated class's ivars.
pub struct CivarSite {
    slot: OnceLock<&'static CivarSlot>,
    class_id: u32,
    name: &'static str,
}

impl CivarSite {
    pub const fn new(class_id: u32, name: &'static str) -> Self {
        Self {
            slot: OnceLock::new(),
            class_id,
            name,
        }
    }

    #[inline]
    fn slot(&self) -> &'static CivarSlot {
        self.slot.get_or_init(|| intern(self.class_id, self.name))
    }

    #[inline]
    pub fn get(&self) -> RubyValue {
        self.slot().get()
    }

    #[inline]
    pub fn set(&self, value: RubyValue) -> Result<(), crate::Signal> {
        let class = crate::ClassId(self.class_id);
        if crate::dispatch::class_frozen(class) {
            return Err(crate::dispatch::frozen_class_error(class));
        }
        self.slot().put(value);
        Ok(())
    }
}

/// `nil` for a class-level `@x` never yet written -- and here that is real
/// Ruby's ACTUAL behavior, not this runtime's usual approximation of it:
/// reading a never-assigned instance variable genuinely answers `nil` (only
/// `@@x` raises a NameError, and only warns for an ivar under `-w`).
/// Oracle-checked: `class C; def self.probe = @never_written; end; C.probe`
/// is `nil`.
pub fn class_ivar_get(class_id: u32, name: &str) -> RubyValue {
    let class_id = crate::boxes::overlay_root(class_id);
    if let Some(mine) = crate::boxes::box_record_for_read(crate::boxes::current_box(), class_id)
        && let Some(slot) = CIVARS.lock().get(&mine).and_then(|m| m.get(name)).copied()
    {
        return slot.get();
    }
    let slot = CIVARS
        .lock()
        .get(&class_id)
        .and_then(|m| m.get(name))
        .copied();
    slot.map_or(RubyValue::Nil, |slot| slot.get())
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
    let class_id = crate::boxes::box_record_for_write(
        crate::boxes::current_box(),
        crate::boxes::overlay_root(class_id),
    );
    intern(class_id, name).put(value);
    Ok(())
}

/// `remove_instance_variable`'s half: clear the slot and answer what it held,
/// or `None` when it held nothing.
///
/// The slot itself stays interned (the intern table is append-only and an
/// emitted site may still name it), but its stamp is cleared too, so
/// [`class_ivar_names`] stops reporting it -- the two have to agree, and they
/// did not: `instance_variables` listed `@sources` while
/// `remove_instance_variable(:@sources)` said it was not defined, which is
/// the pair `Bundler::Plugin.reset!` runs one after the other.
pub fn class_ivar_remove(class_id: u32, name: &str) -> Result<Option<RubyValue>, crate::Signal> {
    if crate::dispatch::class_frozen(crate::ClassId(class_id)) {
        return Err(crate::dispatch::frozen_class_error(crate::ClassId(
            class_id,
        )));
    }
    Ok(CIVARS
        .lock()
        .get(&class_id)
        .and_then(|m| m.get(name))
        .and_then(|slot| slot.take()))
}

/// The class-level ivar names with a value, in FIRST-ASSIGNMENT order --
/// backs `Class#instance_variables`, which is what ruby reports.
///
/// The intern table cannot supply that order, so each slot stamps its own
/// first write ([`CivarSlot::first_write`]) and this sorts on the stamp. A
/// slot interned by a READ and never assigned has no stamp and is skipped,
/// which is also ruby's rule.
pub fn class_ivar_names(class_id: u32) -> Vec<String> {
    let mut rows: Vec<(u64, String)> = CIVARS
        .lock()
        .get(&class_id)
        .map(|m| {
            m.iter()
                .filter_map(|(name, slot)| Some((slot.write_seq()?, name.to_string())))
                .collect()
        })
        .unwrap_or_default();
    rows.sort();
    rows.into_iter().map(|(_, name)| name).collect()
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
        // First-assignment order, which is what ruby reports -- `b` was
        // written first, so it comes first however the names sort.
        assert_eq!(class_ivar_names(903), vec!["b", "a"]);
        assert_eq!(class_ivar_names(904), vec!["z"]);
    }
}

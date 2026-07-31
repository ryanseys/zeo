//! One lock per OBJECT for its instance variables, replacing one lock per
//! ivar.
//!
//! Two things follow from that, and the second matters more than the first.
//!
//! **Size.** A `parking_lot::Mutex<Option<RubyValue>>` is 32 bytes, so a
//! four-ivar object paid 128 bytes of slots plus an always-present 56-byte
//! `HashMap` for the ivars a runtime path might invent. Here the slots are a
//! plain array behind one lock and the invented-ivar storage is a `None`
//! pointer until something actually invents one.
//!
//! **No accessor returns a borrow.** [`IvarCell::get`] answers an owned
//! `RubyValue`; nothing hands out a guard. The rule that a lock must not be
//! held across a Ruby call -- which every ivar read site used to satisfy by
//! convention, through a `{ let __g = ...; __g.clone() }` shape and a comment
//! explaining the temporary-lifetime hazard -- is now a property of the type.
//! A `parking_lot::Mutex` is not re-entrant, so breaking that rule is a silent
//! permanent hang; under `debug_assertions` this module turns it into a named
//! panic instead (see [`IvarCell::held`]).
//!
//! Assignment ORDER is recorded, which the old `Option`-per-slot could not do.
//! `Option` distinguished "never assigned" from "assigned nil" -- and it was
//! free, because the niche made it the same 24 bytes -- but Ruby reports
//! `instance_variables` in FIRST-ASSIGNMENT order, not declaration order, and
//! only a stamp can say that. The stamp carries the definedness too, so the
//! read path loses a branch.

use crate::RubyValue;
use parking_lot::Mutex;

/// `0` means "never assigned"; every real stamp is 1-based.
type Seq = u8;

/// An ivar the class body never declared, assigned by `instance_variable_set`
/// or by a method a runtime path added. Carries its own stamp so it interleaves
/// with the declared slots in assignment order.
struct Invented {
    name: String,
    value: RubyValue,
    seq: Seq,
}

struct Inner<const N: usize> {
    /// `Nil` until assigned -- so a read needs no branch. `seq` is what says
    /// whether the `Nil` is real.
    vals: [RubyValue; N],
    seq: [Seq; N],
    next_seq: Seq,
    /// `None` for the overwhelmingly common object that invents nothing. A
    /// `Vec` rather than a map: invented ivars are few, a linear scan over a
    /// handful of names beats hashing, and insertion order is free.
    ///
    /// `Box`ed deliberately, against `clippy::box_collection`: this whole
    /// module is about object footprint, and `Option<Vec<_>>` is 24 bytes on
    /// EVERY object where `Option<Box<Vec<_>>>` is 8. The extra allocation is
    /// paid only by an object that actually invents an ivar.
    #[allow(clippy::box_collection)]
    invented: Option<Box<Vec<Invented>>>,
}

impl<const N: usize> Inner<N> {
    /// The next stamp, renumbering everything live if the counter is about to
    /// wrap. Renumbering preserves relative order, so what callers observe
    /// never changes -- an object would need 255 assignments to DISTINCT ivars
    /// to reach this at all.
    fn stamp(&mut self) -> Seq {
        if self.next_seq == Seq::MAX {
            self.compact();
        }
        let s = self.next_seq;
        self.next_seq += 1;
        s
    }

    fn compact(&mut self) {
        let mut live: Vec<Seq> = self.seq.iter().copied().filter(|s| *s != 0).collect();
        if let Some(inv) = &self.invented {
            live.extend(inv.iter().map(|i| i.seq));
        }
        live.sort_unstable();
        let rank = |s: Seq| (live.partition_point(|l| *l < s) as Seq) + 1;
        for s in self.seq.iter_mut().filter(|s| **s != 0) {
            *s = rank(*s);
        }
        if let Some(inv) = &mut self.invented {
            for i in inv.iter_mut() {
                i.seq = rank(i.seq);
            }
        }
        self.next_seq = live.len() as Seq + 1;
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.invented
            .as_ref()?
            .iter()
            .position(|i| i.name == name)
    }
}

pub struct IvarCell<const N: usize> {
    inner: Mutex<Inner<N>>,
    /// The thread currently inside an accessor, or `0`. Only a diagnostic: it
    /// converts the hang a re-entrant access would otherwise cause into a
    /// panic that names the slot. Debug builds only, which is where it earns
    /// its keep -- the whole golden corpus runs against a Debug runtime.
    #[cfg(debug_assertions)]
    holder: std::sync::atomic::AtomicU64,
}

impl<const N: usize> Default for IvarCell<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Pairs the real guard with clearing [`IvarCell::holder`] on the way out, so
/// an unwinding accessor cannot leave a stale owner behind.
#[cfg(debug_assertions)]
struct Held<'a, const N: usize> {
    cell: &'a IvarCell<N>,
    guard: parking_lot::MutexGuard<'a, Inner<N>>,
}

#[cfg(debug_assertions)]
impl<const N: usize> std::ops::Deref for Held<'_, N> {
    type Target = Inner<N>;
    fn deref(&self) -> &Inner<N> {
        &self.guard
    }
}

#[cfg(debug_assertions)]
impl<const N: usize> std::ops::DerefMut for Held<'_, N> {
    fn deref_mut(&mut self) -> &mut Inner<N> {
        &mut self.guard
    }
}

#[cfg(debug_assertions)]
impl<const N: usize> Drop for Held<'_, N> {
    fn drop(&mut self) {
        self.cell
            .holder
            .store(0, std::sync::atomic::Ordering::Release);
    }
}

impl<const N: usize> IvarCell<N> {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                vals: std::array::from_fn(|_| RubyValue::Nil),
                seq: [0; N],
                next_seq: 1,
                invented: None,
            }),
            #[cfg(debug_assertions)]
            holder: std::sync::atomic::AtomicU64::new(0),
        }
    }

    #[cfg(debug_assertions)]
    fn held(&self) -> Held<'_, N> {
        use std::sync::atomic::Ordering;
        // `ThreadId` has no stable numeric form, so this hashes it. A
        // collision would only ever cost a false panic in a debug build, and
        // the hash is over a value that is unique per live thread.
        let me = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            std::thread::current().id().hash(&mut h);
            h.finish() | 1
        };
        assert_ne!(
            self.holder.load(Ordering::Acquire),
            me,
            "re-entrant instance-variable access: this thread is already inside \
             this object's ivars. A method reached from an ivar read or write \
             touched the same object -- in a release build that is a permanent \
             hang, not a panic."
        );
        let guard = self.inner.lock();
        self.holder.store(me, Ordering::Release);
        Held { cell: self, guard }
    }

    #[cfg(not(debug_assertions))]
    #[inline]
    fn held(&self) -> parking_lot::MutexGuard<'_, Inner<N>> {
        self.inner.lock()
    }

    /// A declared slot's value -- `nil` for one never assigned, as in Ruby.
    #[inline]
    pub fn get(&self, index: usize) -> RubyValue {
        self.held().vals[index].clone()
    }

    #[inline]
    pub fn set(&self, index: usize, value: RubyValue) {
        let mut inner = self.held();
        if inner.seq[index] == 0 {
            inner.seq[index] = inner.stamp();
        }
        let old = std::mem::replace(&mut inner.vals[index], value);
        // Released before the replaced value drops. No zeo `Drop` runs Ruby
        // code today -- finalizers are queued and run at `GC.start`/exit -- and
        // ordering it this way keeps that from being load-bearing.
        drop(inner);
        drop(old);
    }

    #[inline]
    pub fn defined(&self, index: usize) -> bool {
        self.held().seq[index] != 0
    }

    /// `remove_instance_variable`: empties the slot and answers the old value,
    /// or `None` for a slot never assigned (the caller raises `NameError`).
    pub fn take(&self, index: usize) -> Option<RubyValue> {
        let mut inner = self.held();
        if inner.seq[index] == 0 {
            return None;
        }
        inner.seq[index] = 0;
        let old = std::mem::replace(&mut inner.vals[index], RubyValue::Nil);
        drop(inner);
        Some(old)
    }

    /// By-NAME access, for a receiver whose concrete class the compiler could
    /// not know. `names` is the class's declared ivar list, positionally
    /// matching the slots; a name not in it is an invented ivar.
    ///
    /// `Some(Nil)` for a declared-but-unassigned slot, `None` only for a name
    /// this object has no storage for at all -- the distinction a probe needs.
    pub fn get_named(&self, names: &[&str], name: &str) -> Option<RubyValue> {
        if let Some(index) = names.iter().position(|n| *n == name) {
            return Some(self.get(index));
        }
        let inner = self.held();
        let at = inner.find(name)?;
        Some(inner.invented.as_ref()?[at].value.clone())
    }

    pub fn set_named(&self, names: &[&str], name: &str, value: RubyValue) {
        if let Some(index) = names.iter().position(|n| *n == name) {
            self.set(index, value);
            return;
        }
        let mut inner = self.held();
        match inner.find(name) {
            Some(at) => {
                let inv = inner.invented.as_mut().expect("found an entry in it");
                let old = std::mem::replace(&mut inv[at].value, value);
                drop(inner);
                drop(old);
            }
            None => {
                let seq = inner.stamp();
                inner
                    .invented
                    .get_or_insert_with(Default::default)
                    .push(Invented {
                        name: name.to_string(),
                        value,
                        seq,
                    });
            }
        }
    }

    pub fn remove_named(&self, names: &[&str], name: &str) -> Option<RubyValue> {
        if let Some(index) = names.iter().position(|n| *n == name) {
            return self.take(index);
        }
        let mut inner = self.held();
        let at = inner.find(name)?;
        // An invented ivar genuinely vanishes, name and all.
        let gone = inner.invented.as_mut()?.remove(at);
        drop(inner);
        Some(gone.value)
    }

    /// Every ASSIGNED ivar as `("@name", value)`, in first-assignment order --
    /// what `instance_variables` and the default `Object#inspect` report, and
    /// what CRuby reports.
    pub fn pairs(&self, names: &[&str]) -> Vec<(String, RubyValue)> {
        let inner = self.held();
        let mut out: Vec<(Seq, String, RubyValue)> = names
            .iter()
            .enumerate()
            .filter(|(i, _)| inner.seq[*i] != 0)
            .map(|(i, n)| (inner.seq[i], format!("@{n}"), inner.vals[i].clone()))
            .collect();
        if let Some(inv) = &inner.invented {
            out.extend(
                inv.iter()
                    .map(|i| (i.seq, format!("@{}", i.name), i.value.clone())),
            );
        }
        drop(inner);
        out.sort_by_key(|(s, _, _)| *s);
        out.into_iter().map(|(_, n, v)| (n, v)).collect()
    }

    /// The assigned values alone, in the same order [`Self::pairs`] uses.
    pub fn values(&self) -> Vec<RubyValue> {
        let inner = self.held();
        let mut out: Vec<(Seq, RubyValue)> = (0..N)
            .filter(|i| inner.seq[*i] != 0)
            .map(|i| (inner.seq[i], inner.vals[i].clone()))
            .collect();
        if let Some(inv) = &inner.invented {
            out.extend(inv.iter().map(|i| (i.seq, i.value.clone())));
        }
        drop(inner);
        out.sort_by_key(|(s, _)| *s);
        out.into_iter().map(|(_, v)| v).collect()
    }

    /// `dup`/`clone`'s shallow copy: every value cloned as a HANDLE, so nested
    /// objects stay shared, and the assignment order carried over with them.
    pub fn duplicate(&self) -> Self {
        let inner = self.held();
        let copy = Inner {
            vals: std::array::from_fn(|i| inner.vals[i].clone()),
            seq: inner.seq,
            next_seq: inner.next_seq,
            invented: inner.invented.as_ref().map(|inv| {
                Box::new(
                    inv.iter()
                        .map(|i| Invented {
                            name: i.name.clone(),
                            value: i.value.clone(),
                            seq: i.seq,
                        })
                        .collect(),
                )
            }),
        };
        drop(inner);
        Self {
            inner: Mutex::new(copy),
            #[cfg(debug_assertions)]
            holder: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

/// The footprint this module exists for, pinned so a field added here has to
/// be an explicit decision. Add 16 bytes of `Arc` header and 8 for the frozen
/// flag to get the object: a four-ivar object is 144 bytes, against 208 when
/// each ivar carried its own 32-byte `Mutex` alongside an always-present
/// 56-byte overflow map. CRuby's is about 40 -- the rest of that gap is
/// `RubyValue` being 24 bytes wide.
///
/// Release only: a debug build carries the extra re-entrancy `holder` word.
#[cfg(not(debug_assertions))]
const _: () = {
    assert!(size_of::<IvarCell<0>>() == 24);
    assert!(size_of::<IvarCell<3>>() == 96);
    assert!(size_of::<IvarCell<4>>() == 120);
    assert!(size_of::<IvarCell<5>>() == 144);
};

#[cfg(test)]
mod tests {
    use super::*;

    const NAMES: &[&str] = &["a", "b", "c"];

    fn int(v: &RubyValue) -> Option<i64> {
        match v {
            RubyValue::Int(i) => Some(*i),
            _ => None,
        }
    }

    fn names_of(cell: &IvarCell<3>) -> Vec<String> {
        cell.pairs(NAMES).into_iter().map(|(n, _)| n).collect()
    }

    #[test]
    fn unassigned_reads_nil_but_is_not_reported() {
        let cell = IvarCell::<3>::new();
        assert!(matches!(cell.get(1), RubyValue::Nil));
        assert!(!cell.defined(1));
        assert!(names_of(&cell).is_empty());
    }

    #[test]
    fn assigned_nil_differs_from_never_assigned() {
        let cell = IvarCell::<3>::new();
        cell.set(0, RubyValue::Nil);
        assert!(cell.defined(0));
        assert!(!cell.defined(1));
        assert_eq!(names_of(&cell), vec!["@a"]);
    }

    // The property `Option`-per-slot could not express: Ruby reports
    // first-ASSIGNMENT order, not declaration order.
    #[test]
    fn order_is_first_assignment_not_declaration() {
        let cell = IvarCell::<3>::new();
        cell.set(2, RubyValue::Int(3));
        cell.set(0, RubyValue::Int(1));
        assert_eq!(names_of(&cell), vec!["@c", "@a"]);
        // Reassigning does not move a name.
        cell.set(2, RubyValue::Int(30));
        assert_eq!(names_of(&cell), vec!["@c", "@a"]);
    }

    #[test]
    fn removing_then_reassigning_moves_to_the_end() {
        let cell = IvarCell::<3>::new();
        cell.set(0, RubyValue::Int(1));
        cell.set(1, RubyValue::Int(2));
        assert_eq!(int(&cell.take(0).unwrap()), Some(1));
        assert!(cell.take(0).is_none());
        cell.set(0, RubyValue::Int(9));
        assert_eq!(names_of(&cell), vec!["@b", "@a"]);
    }

    #[test]
    fn invented_ivars_interleave_by_assignment_order() {
        let cell = IvarCell::<3>::new();
        cell.set_named(NAMES, "z", RubyValue::Int(26));
        cell.set(1, RubyValue::Int(2));
        cell.set_named(NAMES, "y", RubyValue::Int(25));
        assert_eq!(names_of(&cell), vec!["@z", "@b", "@y"]);
        assert_eq!(
            int(&cell.get_named(NAMES, "z").unwrap()),
            Some(26),
            "an invented name reads back"
        );
        // A declared name routes to its slot, not the invented list.
        cell.set_named(NAMES, "a", RubyValue::Int(1));
        assert_eq!(int(&cell.get(0)), Some(1));
        assert_eq!(names_of(&cell), vec!["@z", "@b", "@y", "@a"]);
    }

    #[test]
    fn removing_an_invented_ivar_takes_its_name_with_it() {
        let cell = IvarCell::<3>::new();
        cell.set_named(NAMES, "z", RubyValue::Int(26));
        assert_eq!(int(&cell.remove_named(NAMES, "z").unwrap()), Some(26));
        assert!(cell.remove_named(NAMES, "z").is_none());
        assert!(cell.get_named(NAMES, "z").is_none());
        // A DECLARED name never answers `None`, assigned or not.
        assert!(cell.get_named(NAMES, "a").is_some());
    }

    #[test]
    fn duplicate_copies_values_and_order_but_not_identity() {
        let cell = IvarCell::<3>::new();
        cell.set(2, RubyValue::Int(3));
        cell.set_named(NAMES, "z", RubyValue::Int(26));
        let copy = cell.duplicate();
        assert_eq!(names_of(&copy), vec!["@c", "@z"]);
        copy.set(0, RubyValue::Int(1));
        assert_eq!(names_of(&cell), vec!["@c", "@z"]);
    }

    // 255 distinct assignments force a renumbering; relative order must
    // survive it, which is the only thing callers can observe.
    #[test]
    fn compaction_preserves_relative_order() {
        let cell = IvarCell::<3>::new();
        cell.set(1, RubyValue::Int(2));
        cell.set(0, RubyValue::Int(1));
        for i in 0..300 {
            cell.set_named(NAMES, &format!("inv{i}"), RubyValue::Int(i));
            cell.remove_named(NAMES, &format!("inv{i}"));
        }
        assert_eq!(names_of(&cell), vec!["@b", "@a"]);
        cell.set(2, RubyValue::Int(3));
        assert_eq!(names_of(&cell), vec!["@b", "@a", "@c"]);
    }

    #[test]
    fn values_follow_the_same_order_as_pairs() {
        let cell = IvarCell::<3>::new();
        cell.set(2, RubyValue::Int(3));
        cell.set(0, RubyValue::Int(1));
        let vals: Vec<Option<i64>> = cell.values().iter().map(int).collect();
        assert_eq!(vals, vec![Some(3), Some(1)]);
    }
}


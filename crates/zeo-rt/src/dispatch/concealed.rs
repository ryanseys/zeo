//! The rows a compiled-in UNIT wrote, hidden until that unit's file runs.
//!
//! A unit is a load-path file nothing has required yet. Its `def`s register
//! at startup -- the static MRO needs a shape, and the walk must find the
//! real body once the unit loads -- but CRuby has no such method until the
//! `require` reaches the file, so answering from the row before then invents
//! a method the program never defined. `rbs`' `lib/rbs/test/setup.rb` is a
//! SCRIPT with a top-level `def match(filter, name)`; through rdoc it sat in
//! an rspec program's compiled-in load path, and `expect("hello").to
//! match(/ell/)` raised "wrong number of arguments (given 1, expected 2)".
//!
//! The row stays where it is and is CONCEALED instead -- the method twin of
//! `constants::conceal_class`, which hides the same unit's class NAMES for
//! the same reason. `zeo_rt_reveal_unit_methods`, emitted at the head of
//! every unit function, lifts the whole unit's set at once.
//!
//! A concealed name never terminates a lookup: the walk falls THROUGH to the
//! ancestors, so an inherited definition still answers, which is what ruby
//! does with a method that has not been defined yet.

use super::*;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// `(class, name, class_side)` -> the unit that reveals it. Filled once from
/// the program's `REG_CONCEAL_METHOD` rows, then read-only.
static ROWS: OnceLock<crate::FMap<(u32, Symbol, bool), u32>> = OnceLock::new();

/// One flag per unit: false while its rows are still concealed.
static REVEALED: OnceLock<Vec<AtomicBool>> = OnceLock::new();

/// How many units still hold rows back. At zero the gate clears and every
/// probe below is a single load again.
static PENDING_UNITS: AtomicUsize = AtomicUsize::new(0);

/// Installs the program's concealed set. Called once, from
/// `register_program`, with every `REG_CONCEAL_METHOD` row.
pub(crate) fn install(rows: Vec<(u32, Symbol, bool, u32)>) {
    if rows.is_empty() {
        return;
    }
    let mut units: crate::FSet<u32> = Default::default();
    let mut map: crate::FMap<(u32, Symbol, bool), u32> = Default::default();
    let mut widest = 0;
    for (class, name, class_side, unit) in rows {
        units.insert(unit);
        widest = widest.max(unit + 1);
        map.insert((class, name, class_side), unit);
    }
    let _ = ROWS.set(map);
    let _ = REVEALED.set((0..widest).map(|_| AtomicBool::new(false)).collect());
    PENDING_UNITS.store(units.len(), Ordering::Release);
    crate::runtime_meta::arm_concealed();
}

/// Lifts unit `unit`'s concealment. Idempotent: a feature reachable under
/// two spellings runs one function, and a unit that concealed nothing is
/// simply not in the table.
pub(crate) fn reveal_unit(unit: u32) {
    let Some(flags) = REVEALED.get() else {
        return;
    };
    let Some(flag) = flags.get(unit as usize) else {
        return;
    };
    if flag.swap(true, Ordering::AcqRel) {
        return;
    }
    // The window closes when the last concealing unit has run. Until then a
    // program that never requires a swept file keeps the gate armed, which
    // is correct: those methods genuinely do not exist yet.
    if PENDING_UNITS.fetch_sub(1, Ordering::AcqRel) == 1 {
        crate::runtime_meta::disarm_concealed();
    }
    tracing::debug!(unit, "unit methods revealed");
}

/// Whether `class`'s row for `name` belongs to a unit that has not run.
///
/// The gate makes this free for every program with no swept package: one
/// masked load of the word dispatch already reads.
#[inline]
pub(crate) fn is_concealed(class: u32, name: Symbol, class_side: bool) -> bool {
    crate::runtime_meta::any_concealed() && concealed_slow(class, name, class_side)
}

#[cold]
fn concealed_slow(class: u32, name: Symbol, class_side: bool) -> bool {
    let Some(rows) = ROWS.get() else {
        return false;
    };
    let Some(&unit) = rows.get(&(class, name, class_side)) else {
        return false;
    };
    REVEALED
        .get()
        .and_then(|f| f.get(unit as usize))
        .is_some_and(|f| !f.load(Ordering::Acquire))
}

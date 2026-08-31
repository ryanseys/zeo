//! `Ruby::Box` runtime support.
//!
//! A box's top-level constants live on a SURROGATE class -- the compiler
//! mints one per `Ruby::Box.new` it sees (`#<Ruby::Box:N>`), and a run-time
//! `Ruby::Box.new` mints one here. The handle value a program holds IS that
//! class: `Ruby::Box < Module` in CRuby too, which is why `box::X`
//! resolves.
//!
//! Three boxes have no surrogate and cannot have one. `main`'s top level is
//! `Object` itself, and `master`/`root` are boxes zeo has no equivalent for
//! (there is no boot box, and saying so is more honest than faking one), so
//! their handles are minted modules tagged as instances of `Ruby::Box`.
//! Minting reaches the `runtime_meta` overlay and so deoptimizes dispatch
//! process-wide -- which is why it happens LAZILY, on the first program to
//! ask for one of the three by name.
//!
//! Two id spaces are kept apart deliberately. The INTERNAL id is what
//! codegen bakes at every site and what every map is keyed by, with
//! `box_id == 0` meaning main at some twenty places. The DISPLAY id is
//! CRuby's -- master 1, root 2, main 3, user boxes from 4 in creation
//! order -- and is derived (`internal + 3`), never stored in a key.

use crate::RubyValue;
use std::sync::OnceLock;
use std::sync::RwLock;

/// The internal id of the MASTER box -- an inert handle, out of the range
/// codegen ever bakes.
pub const MASTER: u32 = u32::MAX;
/// The internal id of the ROOT box. See [`MASTER`].
pub const ROOT: u32 = u32::MAX - 1;
/// The internal id of the MAIN box, which is the one codegen means by 0.
pub const MAIN: u32 = 0;

/// CRuby's gate: boxes exist only under `RUBY_BOX=1`.
pub fn boxes_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("RUBY_BOX").is_ok_and(|v| v == "1"))
}

/// CRuby's exact refusal for a disabled-mode `Ruby::Box.new`.
pub fn disabled_error() -> crate::Signal {
    crate::builtins::runtime_error!(
        "Ruby Box is disabled. Set RUBY_BOX=1 environment variable to use Ruby::Box."
    )
}

/// The surrogates a RUN-TIME `Ruby::Box.new` minted, and the inert handles
/// for main/master/root: `(internal id -> class, class -> internal id)`.
/// The compile-time surrogates are derived from the registry's own name
/// table (`dispatch::box_surrogate_class`) and are not repeated here.
type Side = (crate::FMap<u32, u32>, crate::FMap<u32, u32>);

static RUNTIME: OnceLock<RwLock<Side>> = OnceLock::new();

fn runtime() -> &'static RwLock<Side> {
    RUNTIME.get_or_init(Default::default)
}

/// The next internal id a run-time `Ruby::Box.new` takes. Compile-time
/// boxes occupy `1..=hir.boxes`, so run-time ones start above the highest
/// id either half has used.
fn next_internal() -> u32 {
    let side = runtime().read().expect("no poisoned box readers");
    let mine = side
        .0
        .keys()
        .filter(|&&b| b != MAIN && b != ROOT && b != MASTER)
        .copied()
        .max()
        .unwrap_or(0);
    let compiled = crate::dispatch::highest_compile_time_box();
    mine.max(compiled) + 1
}

/// The class id owning box `box_id`'s top-level constants -- the surrogate,
/// or 0 (`Object`) for the main box and for a box id no surrogate was
/// registered for.
pub fn surrogate_of(box_id: u32) -> u32 {
    if box_id == MAIN || box_id == ROOT || box_id == MASTER {
        return 0;
    }
    if let Some(cid) = crate::dispatch::box_surrogate_class(box_id) {
        return cid.0;
    }
    runtime()
        .read()
        .expect("no poisoned box readers")
        .0
        .get(&box_id)
        .copied()
        .unwrap_or(0)
}

/// The box a surrogate CLASS ID belongs to -- [`surrogate_of`]'s reverse,
/// asked of the id rather than of a value. Answers for the three inert
/// handles too, which is what makes `Ruby::Box.main.main?` work.
pub fn box_of_surrogate_class(cid: crate::ClassId) -> Option<u32> {
    if let Some(b) = crate::dispatch::box_of_surrogate_class(cid) {
        return Some(b);
    }
    let side = runtime().read().expect("no poisoned box readers");
    if side.1.is_empty() {
        return None;
    }
    side.1.get(&cid.0).copied()
}

/// The box a surrogate class belongs to -- the reverse of [`surrogate_of`].
pub fn box_of_surrogate(v: &RubyValue) -> Option<u32> {
    let RubyValue::Class(cid) = v else {
        return None;
    };
    box_of_surrogate_class(*cid)
}

/// The HANDLE value for box `box_id` -- what `Ruby::Box.new` answered and
/// what `Ruby::Box.current` answers. Raises when boxes are disabled: the
/// gate is CRuby's, and it is a run-time question, so a compile-time box
/// asks it here rather than at the compile that allocated it.
pub fn handle_of(box_id: u32) -> Result<RubyValue, crate::Signal> {
    if !boxes_enabled() {
        return Err(disabled_error());
    }
    if matches!(box_id, MAIN | ROOT | MASTER) {
        return inert_handle(box_id);
    }
    match surrogate_of(box_id) {
        0 => Err(crate::builtins::runtime_error!(
            "box {box_id} has no top level"
        )),
        cid => Ok(RubyValue::Class(crate::ClassId(cid))),
    }
}

/// The minted module standing in for a box with no surrogate. See the
/// module docs for why this is lazy.
fn inert_handle(box_id: u32) -> Result<RubyValue, crate::Signal> {
    if let Some(&cid) = runtime()
        .read()
        .expect("no poisoned box readers")
        .0
        .get(&box_id)
    {
        return Ok(RubyValue::Class(crate::ClassId(cid)));
    }
    let val = crate::runtime_meta::runtime_module_new_owned(None, Some(zeo_abi::RUBY_BOX_CLASS))?;
    let RubyValue::Class(cid) = val else {
        unreachable!("runtime_module_new_owned answers a Class")
    };
    let mut side = runtime().write().expect("no poisoned box writers");
    // Another thread may have won the race; its handle is the one.
    if let Some(&winner) = side.0.get(&box_id) {
        return Ok(RubyValue::Class(crate::ClassId(winner)));
    }
    side.0.insert(box_id, cid.0);
    side.1.insert(cid.0, box_id);
    Ok(RubyValue::Class(cid))
}

/// Mint a box at RUN time: a fresh surrogate module to own its top-level
/// constants, and the handle that names it.
pub fn new_box() -> Result<RubyValue, crate::Signal> {
    if !boxes_enabled() {
        return Err(disabled_error());
    }
    let internal = next_internal();
    let val = crate::runtime_meta::runtime_module_new_owned(None, Some(zeo_abi::RUBY_BOX_CLASS))?;
    let RubyValue::Class(cid) = val else {
        unreachable!("runtime_module_new_owned answers a Class")
    };
    crate::runtime_meta::name_runtime_class_if_anonymous(cid, &format!("#<Ruby::Box:{internal}>"));
    let mut side = runtime().write().expect("no poisoned box writers");
    side.0.insert(internal, cid.0);
    side.1.insert(cid.0, internal);
    drop(side);
    crate::globals::seed_box_globals(internal);
    Ok(RubyValue::Class(cid))
}

/// Seed every COMPILE-TIME box's globals, once, at program start. A box
/// declared in the source is minted by the compiler and never runs
/// [`new_box`], so this is where it gets its `$LOAD_PATH` copy and its
/// empty `$LOADED_FEATURES`.
pub fn seed_compile_time_boxes() {
    for box_id in 1..=crate::dispatch::highest_compile_time_box() {
        crate::globals::seed_box_globals(box_id);
    }
}

/// CRuby's own id for a box: master 1, root 2, main 3, and every other box
/// from 4 in creation order.
pub fn display_id(box_id: u32) -> u32 {
    match box_id {
        MASTER => 1,
        ROOT => 2,
        _ => box_id + 3,
    }
}

/// The `#<Ruby::Box:4,user,optional>` form `inspect` prints.
pub fn describe(box_id: u32) -> String {
    let kind = match box_id {
        MASTER => "master",
        ROOT => "root",
        MAIN => "user,main",
        _ => "user,optional",
    };
    format!("#<Ruby::Box:{},{kind}>", display_id(box_id))
}

/// Every class a BOX owns, by class id. Empty for a program that declares
/// no box, which is why every reader short-circuits on `is_empty` -- box 0
/// pays one read and nothing else.
///
/// A box's top-level class keeps its bare ruby name (`Escapee` written in
/// a box is called `Escapee`), and the registry's name table is what
/// `Object.constants` and a bare constant read scan. Without this mark
/// main would reach a class the box wrote.
static BOX_CLASSES: OnceLock<RwLock<crate::FMap<u32, u32>>> = OnceLock::new();

fn box_classes() -> &'static RwLock<crate::FMap<u32, u32>> {
    BOX_CLASSES.get_or_init(Default::default)
}

/// Records that `cid` belongs to box `box_id` -- `REG_MARK_BOX_CLASS`, and
/// a class a run-time box mints.
pub fn mark_box_class(cid: crate::ClassId, box_id: u32) {
    box_classes()
        .write()
        .expect("no poisoned box-class writers")
        .insert(cid.0, box_id);
}

/// The box `cid` belongs to: 0 for main, which is every class in a program
/// that declares no box.
pub fn class_box(cid: crate::ClassId) -> u32 {
    let table = box_classes().read().expect("no poisoned box-class readers");
    if table.is_empty() {
        return 0;
    }
    table.get(&cid.0).copied().unwrap_or(0)
}

thread_local! {
    /// The box whose code is RUNNING, as opposed to the box a class was
    /// defined in. Every send out of a boxed body carries its `box_id`
    /// already, but the reflection rows those sends reach
    /// (`respond_to?`, `methods`, `method_missing` probing) take no such
    /// argument and would otherwise answer main's view from inside a box.
    ///
    /// `0` (main) at rest, and per-thread; it rides the `Ec` so a fiber
    /// switch does not inherit another fiber's box.
    static CURRENT: std::cell::Cell<u32> = const { std::cell::Cell::new(MAIN) };
}

/// The box whose code is running. See [`CURRENT`].
pub fn current_box() -> u32 {
    CURRENT.with(std::cell::Cell::get)
}

/// Install `b` as the running box, answering the one it replaced.
pub fn swap_current_box(b: u32) -> u32 {
    CURRENT.with(|c| c.replace(b))
}

/// Runs `f` with `b` as the running box, restoring the previous one however
/// `f` leaves -- a raise included, which a bare set/restore pair around a
/// fallible call would leak.
///
/// Box 0 is the overwhelming case and does no work at all: a program with no
/// box never touches the cell.
pub fn in_box<T>(b: u32, f: impl FnOnce() -> T) -> T {
    if b == MAIN {
        return f();
    }
    struct Restore(u32);
    impl Drop for Restore {
        fn drop(&mut self) {
            swap_current_box(self.0);
        }
    }
    let _restore = Restore(swap_current_box(b));
    f()
}

/// Where a per-(box, class) record is keyed.
///
/// CRuby gives a class a PRIME `rb_classext_t` and, on a box's first write,
/// copies it into a per-box table (`box_classext_tbl`, `internal/class.h`).
/// zeo keeps the same one-record-per-(box, class) shape but names the record
/// with a SHADOW class id, so a table already keyed by class id gains the box
/// axis without widening its key type.
///
/// The shadow ids live above every real class id and are never handed to Ruby:
/// they are table keys only. Asking `ancestors_of_value` or `class_name` about
/// one is a bug, so nothing may return a shadow as a class.
const SHADOW_BASE: u32 = 0xF000_0000;

static SHADOWS: OnceLock<RwLock<(crate::FMap<(u32, u32), u32>, u32)>> = OnceLock::new();

fn shadows() -> &'static RwLock<(crate::FMap<(u32, u32), u32>, u32)> {
    SHADOWS.get_or_init(|| RwLock::new((crate::FMap::default(), SHADOW_BASE)))
}

/// The record key `box_id` writes `owner`'s state into, minting one if this
/// is the box's first write to that class.
///
/// Box 0 writes the shared record, and so does a box writing to a class it
/// OWNS -- such a class has no shared version to protect, and giving it a
/// shadow would hide its state from main (the `box::Thrower` case).
pub fn box_record_for_write(box_id: u32, owner: u32) -> u32 {
    if box_id == MAIN || class_box(crate::ClassId(owner)) != 0 {
        return owner;
    }
    let key = (box_id, owner);
    if let Some(&s) = shadows().read().expect("no poisoned shadow readers").0.get(&key) {
        return s;
    }
    let mut w = shadows().write().expect("no poisoned shadow writers");
    if let Some(&s) = w.0.get(&key) {
        return s;
    }
    w.1 += 1;
    let s = w.1;
    w.0.insert(key, s);
    s
}

/// The record key `box_id` READS `owner`'s state from, or `None` when the box
/// has never written to that class and only the shared record applies.
pub fn box_record_for_read(box_id: u32, owner: u32) -> Option<u32> {
    if box_id == MAIN {
        return None;
    }
    shadows()
        .read()
        .expect("no poisoned shadow readers")
        .0
        .get(&(box_id, owner))
        .copied()
}

/// The (box, class) a shadow key stands for, or `None` when `key` is a real
/// class id. A table keyed by class id holds both kinds side by side, so a
/// walk over its keys needs this to tell them apart.
pub fn shadow_owner(key: u32) -> Option<(u32, u32)> {
    if key < SHADOW_BASE {
        return None;
    }
    shadows()
        .read()
        .expect("no poisoned shadow readers")
        .0
        .iter()
        .find_map(|(&(b, owner), &s)| (s == key).then_some((b, owner)))
}

/// A per-box builtin OVERLAY class -> the shared class it patches.
static OVERLAY_ROOTS: OnceLock<RwLock<crate::FMap<u32, u32>>> = OnceLock::new();

fn overlay_roots() -> &'static RwLock<crate::FMap<u32, u32>> {
    OVERLAY_ROOTS.get_or_init(Default::default)
}

/// Records that `cid` is box `box_id`'s overlay of `root` (`root == cid` when
/// it overlays nothing).
pub fn mark_overlay_root(cid: crate::ClassId, root: crate::ClassId) {
    if cid != root {
        overlay_roots()
            .write()
            .expect("no poisoned overlay-root writers")
            .insert(cid.0, root.0);
    }
}

/// The SHARED class `cid` patches, or `cid` itself.
///
/// A per-box builtin overlay is a patch container, never a class of its own:
/// its methods register on the root keyed by box, so any other state written
/// against it -- a class ivar is the case that showed this -- has to land on
/// the root too, or the write and the read name different classes.
pub fn overlay_root(cid: u32) -> u32 {
    let table = overlay_roots()
        .read()
        .expect("no poisoned overlay-root readers");
    if table.is_empty() {
        return cid;
    }
    table.get(&cid).copied().unwrap_or(cid)
}

/// A scoped ambient-box change for code that is not a send, and so gets no
/// box published for it -- a class body's constant write, a `Scope::NAME`
/// read. Restores on drop, an early return or a raise included.
pub struct BoxGuard(u32);

impl BoxGuard {
    pub fn enter(b: u32) -> Option<BoxGuard> {
        (b != MAIN).then(|| BoxGuard(swap_current_box(b)))
    }
}

impl Drop for BoxGuard {
    fn drop(&mut self) {
        swap_current_box(self.0);
    }
}

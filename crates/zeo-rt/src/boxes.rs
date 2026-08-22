//! `Ruby::Box` runtime support -- the RUNTIME half of zeo's compile-time box
//! model. Boxes are allocated at compile time (`Ctx.box_id` is baked into
//! every emit site; each box's top-level constants live on a surrogate class
//! named `#<Ruby::Box:N>`), so what the runtime owns is: the env gate, the
//! surrogate lookup that gives dynamic `eval` the right top-level owner, and
//! the disabled-mode refusals.

use crate::RubyValue;
use std::sync::OnceLock;

/// CRuby's gate: boxes exist only under `RUBY_BOX=1`.
pub fn boxes_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("RUBY_BOX").is_ok_and(|v| v == "1"))
}

/// CRuby's exact refusal for a disabled-mode `Ruby::Box.new`.
pub fn disabled_error() -> crate::Signal {
    crate::dispatch::raise_error(
        "RuntimeError",
        "Ruby Box is disabled. Set RUBY_BOX=1 environment variable to use Ruby::Box.".to_string(),
    )
}

/// The class id owning box `box_id`'s top-level constants -- the compiler's
/// surrogate (`#<Ruby::Box:N>`), or 0 (`Object`) for the main box and for a
/// box id no surrogate was registered for. Answered from the registry-derived
/// map (`dispatch::box_surrogate_class`), not by formatting and parsing the
/// name per ask.
pub fn surrogate_of(box_id: u32) -> u32 {
    if box_id == 0 {
        return 0;
    }
    crate::dispatch::box_surrogate_class(box_id)
        .map(|c| c.0)
        .unwrap_or(0)
}

/// Every class a BOX owns, by class id. Empty for a program that declares
/// no box, which is why every reader short-circuits on `is_empty` -- box 0
/// pays one atomic-free read and nothing else.
///
/// A box's top-level class keeps its bare ruby name (`Escapee` written in
/// a box is called `Escapee`), and the registry's name table is what
/// `Object.constants` and a bare constant read scan. Without this mark
/// main would reach a class the box wrote.
static BOX_CLASSES: std::sync::OnceLock<std::sync::RwLock<crate::FMap<u32, u32>>> =
    std::sync::OnceLock::new();

fn box_classes() -> &'static std::sync::RwLock<crate::FMap<u32, u32>> {
    BOX_CLASSES.get_or_init(Default::default)
}

/// Records that `cid` belongs to box `box_id` -- `REG_MARK_BOX_CLASS`, and
/// (later) a class a run-time box mints.
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

/// Whether any box owns a class at all -- the short-circuit every scan
/// over the registry's name table takes first.
pub fn any_box_classes() -> bool {
    !box_classes()
        .read()
        .expect("no poisoned box-class readers")
        .is_empty()
}

/// The box a surrogate CLASS ID belongs to -- the reverse of
/// [`surrogate_of`], asked of the id rather than of a value.
pub fn box_of_surrogate_class(cid: crate::ClassId) -> Option<u32> {
    crate::dispatch::box_of_surrogate_class(cid)
}

/// The box a surrogate class belongs to -- the reverse of [`surrogate_of`].
pub fn box_of_surrogate(v: &RubyValue) -> Option<u32> {
    let RubyValue::Class(cid) = v else {
        return None;
    };
    crate::dispatch::box_of_surrogate_class(*cid)
}

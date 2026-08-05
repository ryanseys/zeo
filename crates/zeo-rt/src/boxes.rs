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
/// box id no surrogate was registered for.
pub fn surrogate_of(box_id: u32) -> u32 {
    if box_id == 0 {
        return 0;
    }
    crate::dispatch::class_id_by_name(&format!("#<Ruby::Box:{box_id}>"))
        .map(|c| c.0)
        .unwrap_or(0)
}

/// The box a surrogate class belongs to -- the reverse of [`surrogate_of`],
/// answered from the class NAME (`#<Ruby::Box:N>`).
pub fn box_of_surrogate(v: &RubyValue) -> Option<u32> {
    let RubyValue::Class(cid) = v else {
        return None;
    };
    let name = crate::dispatch::class_name(*cid)?;
    name.strip_prefix("#<Ruby::Box:")?
        .strip_suffix('>')?
        .parse()
        .ok()
}

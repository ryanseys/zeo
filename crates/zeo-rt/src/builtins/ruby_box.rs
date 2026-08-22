//! `Ruby::Box` -- namespace isolation, env-gated exactly like CRuby's
//! (`RUBY_BOX=1`).
//!
//! A box's top-level constants live on a SURROGATE class. `box =
//! Ruby::Box.new` as a top-level statement allocates one at COMPILE time
//! (so `box.eval`/`box.require` splice AOT, which is the fast, statically
//! typed path); anywhere else it mints one at run time. Either way the
//! handle IS the surrogate class -- `Ruby::Box < Module` in CRuby too --
//! and `crate::boxes` owns the table both halves share.
//!
//! `Ruby` itself is the identity namespace: the `RUBY_*` constants under
//! their modern spellings, seeded by bootstrap from the same one source.

use crate::RubyValue;
use crate::boxes;
use crate::dispatch::raise_error;
use zeo_macros::{ruby_class, ruby_module};

mod ruby_ns {
    use super::*;

    ruby_module! {
        Ruby = zeo_abi::RUBY_MODULE;
    }
}

/// The RUN-TIME allocation path -- an expression-position `.new`, or one
/// the compile-time box loader did not claim. A `ConstructorFn` rather
/// than a table row, so `Ruby::Box.singleton_methods(false)` matches
/// CRuby's (the Pathname precedent).
fn box_construct(
    _id: crate::ClassId,
    _args: &[RubyValue],
    _block: Option<RubyValue>,
) -> Result<RubyValue, crate::Signal> {
    boxes::new_box()
}

/// `Ruby::Box#require`/`#load`: the box's own load path, and its own
/// once-only table.
fn box_load(box_id: u32, feature: &RubyValue, reload: bool) -> Result<RubyValue, crate::Signal> {
    let path = crate::builtins::convert::to_rstr(feature)?
        .lock()
        .to_utf8_lossy()
        .into_owned();
    match crate::features::load_from_disk(&path, box_id, reload) {
        Some(result) => result.map(RubyValue::Bool),
        None => Err(crate::builtins::kernel::missing_feature_error(&path)),
    }
}

/// Whether `recv` is the handle for exactly box `want`.
fn box_kind_is(recv: &RubyValue, want: u32) -> bool {
    boxes::box_of_surrogate(recv) == Some(want)
}

pub fn register_ruby_box(registry: &mut crate::dispatch::ClassRegistry) {
    registry.register(
        zeo_abi::RUBY_BOX_CLASS,
        "Ruby::Box",
        false,
        zeo_abi::declared_ancestors(zeo_abi::RUBY_BOX_CLASS),
        Some(box_construct as crate::dispatch::ConstructorFn),
    );
}

mod loader {
    use super::*;

    ruby_module! {
        Loader = zeo_abi::RUBY_BOX_LOADER_MODULE;

        // The box-aware load path. Outside an enabled box these are the
        // ordinary dynamic require/load (Kernel's own body) -- CRuby's
        // Loader methods degrade the same way.
        module_function def "require" | "require_relative" | "load" (_recv, *args) {
            crate::builtins::check_arity(args.len(), 1, Some(2))?;
            crate::builtins::kernel::dynamic_require(&args[0])
        }
    }
}

mod box_class {
    use super::*;

    ruby_class! {
        Box = zeo_abi::RUBY_BOX_CLASS < zeo_abi::MODULE_CLASS;

        def self."enabled?"(_recv) {
            Ok(RubyValue::Bool(boxes::boxes_enabled()))
        }
        // The box the CALLER runs in. A method row cannot see that -- a
        // `MethodFn` takes no box -- so the emitter FOLDS every literal
        // `Ruby::Box.current` to its own site's box, and this row is what a
        // computed `Ruby::Box.send(:current)` reaches: main, which is where
        // all but a box's own code runs.
        def self."current"(_recv) {
            if !boxes::boxes_enabled() {
                return Ok(RubyValue::Nil);
            }
            boxes::handle_of(boxes::MAIN)
        }
        def self."main"(_recv) { boxes::handle_of(boxes::MAIN) }
        def self."root"(_recv) { boxes::handle_of(boxes::ROOT) }
        def self."master"(_recv) { boxes::handle_of(boxes::MASTER) }
        // `Ruby::Box.new` runs the CONSTRUCTOR (`box_construct`), which
        // mints; `initialize` exists so the row set matches CRuby's.
        private def "initialize"(_recv) {
            Ok(RubyValue::Nil)
        }
        def "main?"(recv) { Ok(RubyValue::Bool(box_kind_is(recv, boxes::MAIN))) }
        def "root?"(recv) { Ok(RubyValue::Bool(box_kind_is(recv, boxes::ROOT))) }
        def "master?"(recv) { Ok(RubyValue::Bool(box_kind_is(recv, boxes::MASTER))) }
        def "eval"(recv, src) {
            let source = match src {
                RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
                other => {
                    return Err(crate::builtins::type_error!(
                        "wrong argument type {} (expected String)",
                        crate::builtins::check_type_name(other)
                    ));
                }
            };
            let box_id = boxes::box_of_surrogate(recv).unwrap_or(0);
            crate::eval_string(&source, crate::dispatch::main_object(), box_id)
        }
        def "load_path"(recv) {
            let box_id = boxes::box_of_surrogate(recv).unwrap_or(0);
            Ok(crate::globals::global_get(box_id, "$LOAD_PATH"))
        }
        // A box's own load path, searched in the box's own
        // `$LOADED_FEATURES` -- so a file already loaded in MAIN loads
        // again here, which is CRuby's rule and the whole point of a box.
        //
        // A feature the compiler SPLICED into main has no file to re-read
        // and no unit of its own, so it raises the ordinary `LoadError`.
        // See `tests/gaps/a_box_cannot_require_a_spliced_feature.rb`.
        def "require" | "require_relative" (recv, feature) {
            let box_id = boxes::box_of_surrogate(recv).unwrap_or(0);
            box_load(box_id, feature, false)
        }
        def "load"(recv, *args) {
            crate::builtins::check_arity(args.len(), 1, Some(2))?;
            if matches!(args.get(1), Some(v) if v.truthy()) {
                return Err(crate::builtins::not_impl_error!(
                    "`Ruby::Box#load`'s `wrap:` isn't supported (CRuby wraps the file in an \
                     anonymous module; zeo has no equivalent definee)"
                ));
            }
            let box_id = boxes::box_of_surrogate(recv).unwrap_or(0);
            box_load(box_id, &args[0], true)
        }
        // CRuby's own form: `#<Ruby::Box:4,user,optional>`. The number is
        // the DISPLAY id (master 1, root 2, main 3, users from 4), never
        // the internal one codegen bakes.
        def "inspect"(recv) {
            let Some(bx) = boxes::box_of_surrogate(recv) else {
                return Err(raise_error("TypeError", "not a box".to_string()));
            };
            Ok(RubyValue::Str(crate::string_new(boxes::describe(bx))))
        }
    }
}

//! `Ruby::Box` -- namespace isolation, env-gated exactly like CRuby's
//! (`RUBY_BOX=1`). zeo's boxes are COMPILE-TIME: `box = Ruby::Box.new` as a
//! top-level statement allocates one in the compiler, `box.eval`/`box.require`
//! splice AOT, and each box's top-level constants live on a surrogate class
//! (`#<Ruby::Box:N>` -- see `crate::boxes`). The rows here carry the dynamic
//! surface: the disabled-mode refusals with CRuby's messages, and `eval` on a
//! box value routed through the runtime `eval` entry with the box's OWN top-level owner
//! (so a dynamic `box.eval("X = 1")` lands in the box, not on `Object`).
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

/// The DYNAMIC allocation path (an expression-position `.new` the
/// compile-time box loader did not claim): CRuby's disabled refusal when
/// ungated, the compile-time-model refusal otherwise. A `ConstructorFn`
/// rather than a table row, so `Ruby::Box.singleton_methods(false)` stays
/// `[:current, :enabled?]` like CRuby's (the Pathname precedent).
fn box_construct(
    _id: crate::ClassId,
    _args: &[RubyValue],
    _block: Option<RubyValue>,
) -> Result<RubyValue, crate::Signal> {
    if !boxes::boxes_enabled() {
        return Err(boxes::disabled_error());
    }
    Err(crate::builtins::not_impl_error!(
        "dynamic `Ruby::Box.new` isn't supported (zeo boxes are compile-time; write `box = Ruby::Box.new` as a top-level statement)"
    ))
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
        // The current box as a VALUE. zeo bakes the enclosing box into each
        // emit site at compile time (there is no runtime current-box), so the
        // dynamic row answers what CRuby's disabled mode answers; box-scoped
        // code sees its own box through the compile-time handle instead.
        def self."current"(_recv) {
            Ok(RubyValue::Nil)
        }
        private def "initialize"(_recv) {
            if !boxes::boxes_enabled() {
                return Err(boxes::disabled_error());
            }
            Err(crate::builtins::not_impl_error!(
                "dynamic `Ruby::Box.new` isn't supported (zeo boxes are compile-time; write `box = Ruby::Box.new` as a top-level statement)"
            ))
        }
        def "eval"(recv, src) {
            let source = match src {
                RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
                other => {
                    return Err(crate::builtins::type_error!(
                        "wrong argument type {} (expected String)",
                        crate::builtins::class_name_of(other)
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
        // zeo requires are compile-time splices (`box.require` at the top
        // level already works); a DYNAMIC require has no source to splice.
        def "require" | "require_relative" (_recv, _feature) {
            Err(crate::builtins::not_impl_error!(
                "dynamic `Ruby::Box#require`/`#load` isn't supported (zeo requires splice at compile time; write `box.require \"feature\"` as a top-level statement)"
            ))
        }
        def "load"(_recv, *_args) {
            Err(crate::builtins::not_impl_error!(
                "dynamic `Ruby::Box#require`/`#load` isn't supported (zeo requires splice at compile time; write `box.require \"feature\"` as a top-level statement)"
            ))
        }
        def "inspect"(recv) {
            let RubyValue::Class(cid) = recv else {
                return Err(raise_error("TypeError", "not a box".to_string()));
            };
            let name = crate::dispatch::class_name(*cid)
                .unwrap_or_else(|| "#<Ruby::Box>".to_string());
            Ok(RubyValue::Str(crate::string_new(name)))
        }
    }
}

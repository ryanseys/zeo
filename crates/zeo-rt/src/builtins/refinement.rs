//! `Refinement` -- what a `refine Target do ... end` block answers.
//!
//! The object itself is an ORDINARY registered module holding the refined
//! methods; the compiler mints one per `refine` block under a name the source
//! could never write. What makes it a `Refinement` is the `(module, target)`
//! pair codegen marks it with (`ClassRegistry::mark_refinement`), which is
//! also what `RubyValue::class_id` reads to answer `Refinement` rather than
//! `Module`, and what renders `#<refinement:String@M>`.
//!
//! Two rows, because ruby declares exactly two: `#target` and the private
//! `#import_methods`. Everything else it answers comes from `Module` through
//! the ancestry walk.
//!
//! `import_methods` copies bodies, not references, like CRuby -- but zeo's
//! bodies are Rust fns with no cref, so an imported body does NOT see the
//! refinement's own refinements the way a re-compiled CRuby iseq would.

use crate::RubyValue;
use crate::builtins::arg_error;
use crate::builtins::rmodule::recv_cid;
use zeo_macros::ruby_class;

ruby_class! {
    Refinement = zeo_abi::REFINEMENT_CLASS < zeo_abi::MODULE_CLASS;

    // `Refinement#target` -- the class the block refines. A receiver that is
    // not a holder cannot reach this row: `Refinement` has no instances but
    // the holders themselves.
    def "target"(recv) {
        Ok(match crate::dispatch::refinement_of(recv_cid(recv)) {
            Some((_, target)) => RubyValue::Class(target),
            None => RubyValue::Nil,
        })
    }

    // Private `Refinement#import_methods(*modules)` -- each module's OWN
    // methods, copied into this refinement at their declared visibility.
    private def "import_methods" cfunc (recv, *modules) {
        if modules.is_empty() {
            return Err(arg_error!("wrong number of arguments (given 0, expected 1+)"));
        }
        crate::runtime_meta::refinement_import_methods(recv, modules)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RProc, Symbol};

    // Each test runs in its own nextest process -- see the crate README for
    // the with_core() bootstrap pattern.
    fn install_core() {
        crate::dispatch::install_class_registry(crate::dispatch::ClassRegistry::with_core());
    }

    /// A runtime `refine(String) { }` holder minted on a fresh module.
    fn string_refinement() -> RubyValue {
        let module = crate::runtime_meta::runtime_module_new(None).unwrap();
        let RubyValue::Class(mid) = module else {
            panic!("Module.new answers a module");
        };
        let body = RProc::with_self(|_, _| Ok(RubyValue::Nil), RubyValue::Nil, 0, false);
        crate::runtime_meta::runtime_refine(mid, zeo_abi::STRING_CLASS, &body).unwrap()
    }

    #[test]
    fn a_holder_answers_its_target_and_reads_as_a_refinement() {
        install_core();
        let holder = string_refinement();
        assert_eq!(holder.class_id(), zeo_abi::REFINEMENT_CLASS);
        let target =
            crate::dispatch::send_value(&holder, Symbol::intern("target"), &[], None).unwrap();
        assert!(matches!(target, RubyValue::Class(c) if c == zeo_abi::STRING_CLASS));
    }

    #[test]
    fn import_methods_refuses_an_empty_argument_list() {
        install_core();
        let holder = string_refinement();
        let err = crate::dispatch::send_value(&holder, Symbol::intern("import_methods"), &[], None);
        let Err(crate::Signal::Raise(exc)) = err else {
            panic!("expected an ArgumentError raise");
        };
        assert_eq!(
            exc.as_object_unchecked().class_id(),
            zeo_abi::ARGUMENT_ERROR_CLASS
        );
    }
}

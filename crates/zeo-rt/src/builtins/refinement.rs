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

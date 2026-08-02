//! `Refinement` -- what a `refine Target do ... end` block answers.
//!
//! The object itself is an ORDINARY registered module holding the refined
//! methods; the compiler mints one per `refine` block under a name the source
//! could never write. What makes it a `Refinement` is the `(module, target)`
//! pair codegen marks it with (`ClassRegistry::mark_refinement`), which is
//! also what `RubyValue::class_id` reads to answer `Refinement` rather than
//! `Module`, and what renders `#<refinement:String@M>`.
//!
//! One row, because ruby declares exactly one: `#target`. Everything else it
//! answers comes from `Module` through the ancestry walk.

use crate::RubyValue;
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
}

//! The Ruby files CRuby compiles INTO its own interpreter, compiled into a
//! zeo program the same way.
//!
//! CRuby writes 24 of its core files in Ruby (`BUILTIN_RB_SRCS` in
//! `common.mk`) and compiles them into the binary; a method defined in one
//! reports `<internal:nilclass>` as its source. zeo vendors those files
//! VERBATIM from the same `ruby/ruby` pin the C API headers ride, so the rows
//! a program dispatches to are CRuby's own code rather than a Rust
//! reimplementation of it -- which is the only way the signatures, the
//! `source_location`s and the corner cases can agree by construction.
//!
//! # What is eligible
//!
//! Only a file with NO `Primitive.`, `__builtin` or `cexpr!` in it. Those are
//! a C-level escape hatch zeo has no answer for, and a file carrying one
//! would compile and then fail at the call. `tools/zeo-dev corelib census`
//! prints the eligibility of all 24; at v4.0.6 five qualify and this module
//! carries the ones zeo has measured.
//!
//! # Provenance
//!
//! `crates/zeo/corelib/` is upstream's bytes and carries NO patch series --
//! unlike the C headers next door, which describe zeo's object layout rather
//! than ruby's behaviour. Three checks hold the claim up, and each covers what
//! the others cannot:
//!
//! * `build.rs` hashes each file against `upstream.lock` and stages it into
//!   `OUT_DIR`. The `include_str!`s below read the STAGED copy, so bytes that
//!   were not hashed cannot reach a compiled program.
//! * `tools/zeo-dev corelib verify` re-hashes offline -- in CI, in a release
//!   tarball, on a machine with no network.
//! * `tools/zeo-dev corelib sync --check` fetches the pinned rev and proves
//!   the locked bytes ARE upstream's.
//!
//! The lock also records each file's git blob OID, which is what GitHub's
//! contents API answers, so a reader can verify one against github.com with a
//! single request and no clone.

use crate::hir::{Hir, NodeId};
use crate::lower_error::LowerError;

/// One vendored file: the name CRuby reports for it, and its bytes.
struct Segment {
    /// What `#source_location` and a frame answer. CRuby's own spelling --
    /// `<internal:nilclass>`, not a path -- because that is what a program
    /// reading either one gets from ruby.
    internal_name: &'static str,
    source: &'static str,
}

/// The vendored corelib, in the order it is compiled in. Every entry is
/// unconditional today; a file bound to a `require` would gate here.
const SEGMENTS: &[Segment] = &[Segment {
    internal_name: "<internal:nilclass>",
    source: include_str!(concat!(env!("OUT_DIR"), "/corelib_nilclass.rb")),
}];

/// Lower every segment into `hir`, appending its statements.
///
/// Always called: the prefix arena is built once and cloned per compile, so
/// `ZEO_CORELIB=rust` and an `eval` snippet TRUNCATE the cloned statement
/// list instead of building a second arena. See `parse_and_lower_with`.
///
/// Each file is registered in the source table under its `<internal:>` name
/// and lowered with `lowering_file` pointing at it, so a `def` inside one
/// carries that name and the file's own line -- which is what makes
/// `NilClass.instance_method(:to_i).source_location` answer
/// `["<internal:nilclass>", 36]` the way ruby's does.
pub(super) fn lower_into(hir: &mut Hir, statements: &mut Vec<NodeId>) -> Result<(), LowerError> {
    for seg in SEGMENTS {
        let file = hir.add_file(seg.internal_name, seg.source);
        // Not part of the program in any sense a program can observe --
        // see `Hir::internal_files`.
        hir.internal_files.insert(file);
        let saved = hir.lowering_file.replace(file);
        let lowered = crate::lower::parse_and_lower_into(hir, seg.source).map_err(|e| LowerError {
            message: format!(
                "internal error in zeo's vendored corelib {} (this is a zeo bug): {}",
                seg.internal_name, e.message
            ),
            ..e
        });
        hir.lowering_file = saved;
        statements.extend(lowered?);
    }
    Ok(())
}

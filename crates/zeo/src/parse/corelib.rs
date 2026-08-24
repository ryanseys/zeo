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

/// One vendored file: what it is called, what CRuby reports for it, and
/// whether it is compiled in unless asked otherwise.
struct Segment {
    /// The name `ZEO_CORELIB` selects it by.
    name: &'static str,
    /// What `#source_location` and a frame answer. CRuby's own spelling --
    /// `<internal:nilclass>`, not a path -- because that is what a program
    /// reading either one gets from ruby.
    internal_name: &'static str,
    source: &'static str,
    /// Whether it is on with no `ZEO_CORELIB` set.
    ///
    /// This is a SIZE decision, and the only one. A Ruby row is emitted into
    /// every binary that can reach it, while a Rust row already lives in the
    /// shared archive -- so `nilclass` costs 34 KB and is on, and `pathname`
    /// costs 8.7 MB and is not. Nothing about correctness differs: both are
    /// CRuby's own bytes and both pass the corpus.
    ///
    /// Gating `pathname` on `require "pathname"` is NOT available, and that
    /// is a fact about ruby rather than a limitation here: ruby 4.0 loads
    /// `pathname.so` before the first line, so `Pathname.new` answers with no
    /// require at all and only `#find`/`#rmtree` arrive with one.
    on_by_default: bool,
}

impl Segment {
    /// Whether this compile carries the segment.
    fn selected(&self, policy: &crate::Corelib) -> bool {
        match policy {
            crate::Corelib::Default => self.on_by_default,
            crate::Corelib::Rust => false,
            crate::Corelib::Only(names) => names.iter().any(|n| n == self.name),
        }
    }
}

/// The vendored corelib, in the order it is compiled in.
const SEGMENTS: &[Segment] = &[
    Segment {
        name: "nilclass",
        internal_name: "<internal:nilclass>",
        source: include_str!(concat!(env!("OUT_DIR"), "/corelib_nilclass.rb")),
        on_by_default: true,
    },
    Segment {
        name: "pathname",
        internal_name: "<internal:pathname_builtin>",
        source: include_str!(concat!(env!("OUT_DIR"), "/corelib_pathname_builtin.rb")),
        on_by_default: false,
    },
];

/// Lower the SELECTED segments into `hir`, appending their statements.
///
/// Per compile, not once: the exception prelude is cached in a template arena
/// and cloned, and an unselected corelib segment must not be lowered into it
/// at all. Leaving one there as dead nodes is not free -- `uses_runtime_eval`
/// and every other whole-arena scan reads `Hir::nodes` directly, so a
/// segment nobody selected still linked the compiler into every binary.
///
/// Each file is registered in the source table under its `<internal:>` name
/// and lowered with `lowering_file` pointing at it, so a `def` inside one
/// carries that name and the file's own line -- which is what makes
/// `NilClass.instance_method(:to_i).source_location` answer
/// `["<internal:nilclass>", 36]` the way ruby's does.
pub(super) fn lower_into(
    hir: &mut Hir,
    statements: &mut Vec<NodeId>,
    policy: &crate::Corelib,
) -> Result<(), LowerError> {
    for seg in SEGMENTS.iter().filter(|s| s.selected(policy)) {
        lower_as_internal(hir, statements, seg.internal_name, seg.source)?;
    }
    Ok(())
}

/// Lower `source` as an INTERNAL file named `internal_name`, appending its
/// statements.
///
/// One place registers the file and marks it internal, so every carve-out
/// keyed on `Hir::internal_files` -- the `method_added` hook, `Coverage`, and
/// the fused-iterator guard -- covers a segment by construction rather than
/// by each remembering to.
pub(crate) fn lower_as_internal(
    hir: &mut Hir,
    statements: &mut Vec<NodeId>,
    internal_name: &str,
    source: &'static str,
) -> Result<(), LowerError> {
    let file = hir.add_file(internal_name, source);
    // Not part of the program in any sense a program can observe --
    // see `Hir::internal_files`.
    hir.internal_files.insert(file);
    let saved = hir.lowering_file.replace(file);
    let lowered = crate::lower::parse_and_lower_into(hir, source).map_err(|e| LowerError {
        message: format!(
            "internal error in zeo's vendored corelib {internal_name} (this is a zeo bug): {}",
            e.message
        ),
        ..e
    });
    hir.lowering_file = saved;
    statements.extend(lowered?);
    Ok(())
}

/// Every segment name, for the CLI's error when `ZEO_CORELIB` names one that
/// does not exist -- a typo there is otherwise a silent no-op.
pub(crate) fn names() -> Vec<&'static str> {
    SEGMENTS.iter().map(|s| s.name).collect()
}

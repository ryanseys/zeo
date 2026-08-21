//! Where a HIR node was written -- the `(file, line)` pair every emitted
//! frame, backtrace row and `#source_location` is built from.
//!
//! Backend-neutral: it reads spans and the arena's prebuilt newline index and
//! knows nothing about how a frame is emitted. It lived in `codegen/` until
//! the Cranelift emitter became the product and wanted the same answers.

use crate::compiler::Compiler;

/// The `(file name, 1-based line)` of `node`'s span start -- `None` for a
/// synthetic node (the exception prelude, `eval` bodies). Backing for
/// backtrace-frame emission: the file string is baked into the binary and
/// the line comes from the file's prebuilt newline index (`line_at`).
pub(crate) fn source_location(
    compiler: &Compiler,
    node: crate::hir::NodeId,
) -> Option<(&str, u32)> {
    let span = compiler.hir.span(node)?;
    let file = compiler.hir.files.get(span.file.0 as usize)?;
    let upto = (span.start as usize).min(file.source.len());
    // Borrowed, not cloned. This is asked once per emitted statement and once
    // per call site, and `pooled_file` dedups the answer anyway -- so a clone
    // here was a fresh allocation per statement for a string that already
    // lives in the arena and outlives every caller.
    Some((file.name.as_str(), file.line_at(upto as u32)))
}

/// The line `node`'s span ENDS on -- a `def`/`class` node's `end` keyword
/// line, which is what `TracePoint` reports for `:return`/`:end` (0, the
/// no-trace-events marker, when the node is span-less).
pub(crate) fn source_end_line(compiler: &Compiler, node: crate::hir::NodeId) -> u32 {
    let Some(span) = compiler.hir.span(node) else {
        return 0;
    };
    let Some(file) = compiler.hir.files.get(span.file.0 as usize) else {
        return 0;
    };
    let upto = (span.end as usize).min(file.source.len());
    // `line_at`, not a newline count from byte 0 -- the same quadratic its
    // own docs describe, left behind here when `source_location` was
    // converted. Every emitted method asks this once, and the file it scans
    // is the whole SPLICED require graph: 96% of a gem-scale compile's
    // samples landed in this one `filter().count()`.
    file.line_at(upto as u32)
}

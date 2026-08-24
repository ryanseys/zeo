//! Line coverage's compile-time half: which lines a coverage-activated
//! program reports, and which of them no emitted statement ever stamps.
//!
//! Both backends read this; the collection of STATEMENT lines stays with
//! each emitter (it happens as the statement stream is written).

use crate::compiler::Compiler;

/// Whether this program activated line coverage -- it `require`d
/// `coverage`. A program without the require collects and emits NOTHING,
/// which is the whole cost model: the instrumentation is not a checked
/// branch per line, it is absent.
pub(crate) fn active(compiler: &Compiler) -> bool {
    compiler.hir.activated_features.contains("coverage")
}

/// `def` lines per file: definitions the emitted statement stream never
/// stamps -- top-level and class-body `def`s are intercepted at analyze
/// (`process_top_stmt` / `register_body_def_method`) and compiled statically,
/// so no runtime statement passes their line. CRuby reports a definition
/// line's execution count, which for these is exactly once, when the file
/// loads -- `Coverage.result` adds the static 1 for a covered file.
pub(crate) fn def_lines(
    compiler: &Compiler,
) -> std::collections::BTreeMap<String, std::collections::BTreeSet<u32>> {
    fn walk(
        compiler: &Compiler,
        body: &[crate::hir::NodeId],
        out: &mut std::collections::BTreeMap<String, std::collections::BTreeSet<u32>>,
    ) {
        for &s in body {
            match &compiler.hir[s] {
                crate::hir::HirNode::DefMethod { .. } => {
                    if let Some((file, line)) = crate::analyze::source::source_location(compiler, s)
                    {
                        out.entry(file.to_string()).or_default().insert(line);
                    }
                }
                crate::hir::HirNode::ClassDef { body, .. } => {
                    let body = body.clone();
                    walk(compiler, &body, out);
                }
                _ => {}
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    let programs: Vec<Vec<crate::hir::NodeId>> = compiler
        .hir
        .iter()
        .filter_map(|n| match n {
            crate::hir::HirNode::Program(stmts) => Some(stmts.clone()),
            _ => None,
        })
        .collect();
    for stmts in &programs {
        walk(compiler, stmts, &mut out);
    }
    out
}

//! Pins the CLI's miette rendering of compile errors -- the no-color unicode
//! theme, so the snapshots are what a piped/NO_COLOR invocation prints (the
//! themed graphical form differs only in ANSI styling). The `-e` (pathless)
//! compile shape keeps file names deterministic: the main file registers as
//! `"-e"`.

use miette::{GraphicalReportHandler, GraphicalTheme};

fn render(err: zeo::CompileError) -> String {
    let mut out = String::new();
    GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor())
        .render_report(&mut out, &err)
        .expect("renders");
    out
}

/// The headline case: valid-but-unsupported Ruby gets an annotated excerpt
/// pointing at exactly the offending construct, with the "this is zeo's
/// gap, not your syntax" framing in code + help.
#[test]
fn an_unsupported_construct_renders_a_located_excerpt() {
    let err = zeo::check_program_with("x = 1\nputs(1) if /foo/\n", &Default::default())
        .expect_err("a bare condition regexp is rejected");
    insta::assert_snapshot!(render(err));
}

/// Prism rejects the source before any lowering frame exists, so this
/// renders span-less: message, code, and help only.
#[test]
fn a_parse_failure_renders_without_an_excerpt() {
    let err = zeo::check_program_with("def foo(\n", &Default::default())
        .expect_err("malformed source is rejected");
    insta::assert_snapshot!(render(err));
}

/// Every construct the Cranelift emitter used to refuse now compiles.
///
/// This test replaced `an_unsupported_construct_is_an_error_not_a_panic`,
/// which asserted that a refusal renders as a typed diagnostic rather than a
/// panic. That test had to be pointed at a LIVE refusal, and it was re-pointed
/// four times on 2026-08-21 as each one was fixed -- at which point no
/// reachable user-facing emitter refusal was left to point it at. Every
/// `Fx::unsupported` site that remains guards an internal invariant (a
/// non-literal block where the walk proves one is literal, a marker shape
/// `lower::ffi` always writes) or a construct ruby itself rejects.
///
/// So the coverage moves from "a refusal reports well" to "these do not
/// refuse". The lowering stage still proves the first, in the two tests
/// below.
#[test]
fn every_shape_the_emitter_once_refused_now_compiles() {
    for (what, src) in [
        // Refused by the retired rustc emitter only; CLIF always lowered it.
        (
            "safe navigation on a class method",
            "class F\n  def self.b = 1\nend\np F&.b\n",
        ),
        (
            "a refined call through `&.`",
            "module R\n  refine String do\n    def sh = upcase\n  end\nend\nusing R\np \"h\"&.sh\n",
        ),
        (
            "a splat at a refined call",
            "module R\n  refine Array do\n    def pick(*n) = n\n  end\nend\nusing R\ni = [0]\np [1].pick(*i)\n",
        ),
        (
            "a fused loop with a multi-parameter block",
            "3.times { |a, b| p [a, b] }\n",
        ),
        (
            "a multi-parameter fused block capturing an outer local",
            "seen = []\n3.times { |a, b| seen << a }\np seen\n",
        ),
    ] {
        zeo::compile_to_clif_text(src, &Default::default())
            .unwrap_or_else(|e| panic!("{what} should compile: {}", String::from(e)));
    }
}

/// A failure past lowering carries its stage as the diagnostic code, and the
/// statement that provoked it: analyze refuses definitions, and the walk stamps
/// the one it was handed (see `analyze_error`). Without that, a rejection names
/// a construct rather than a place -- which for a gem is a search across every
/// file that spells it.
#[test]
fn an_analyze_error_is_coded_with_its_stage() {
    // A singleton-class `prepend` of a module carrying the hook. Two probes
    // have retired from this slot: a kind collision (ruby RAISES on one, so
    // it compiles into that raise now) and subclassing a built-in class
    // (every built-in class is subclassable now -- see
    // `zeo_abi::NOT_PAYLOAD_ROOTS`).
    let err = zeo::check_program_with(
        "module M\n  def self.prepend_features(base)\n    super\n  end\nend\nclass K\n  class << self\n    prepend M\n  end\nend\n",
        &Default::default(),
    )
    .expect_err("a hook-carrying singleton prepend fails analyze");
    insta::assert_snapshot!(render(err));
}

/// A class-body statement gets a frame of its OWN, so a rejection raised while
/// lowering it names that line rather than the enclosing `class`/`module`
/// header. The FFI directives are the case that made it necessary: they never
/// reach `lower_node` (nor `lower_class_body_statement`), so every one of the
/// ledger's FFI rows used to point at `module Native`, and triaging them
/// meant re-parsing the files to recover what they said. (A computed
/// `ffi_lib` no longer rejects -- it defers to class-body time -- so the
/// probe is an `attach_function` naming an undeclared type.)
#[test]
fn a_class_body_directive_names_its_own_line() {
    let err = zeo::check_program_with(
        "require 'ffi'\nmodule Native\n  extend FFI::Library\n  ffi_lib 'm'\n  attach_function :f, [:nope_type], :int\nend\n",
        &Default::default(),
    )
    .expect_err("an undeclared argument type is rejected");
    insta::assert_snapshot!(render(err));
}

/// A codegen error whose span survived to the CLI boundary renders the
/// same located excerpt the other stages do -- `zeo::codegen` code, the
/// `╭─[file:line:col]` header, and the position label. Constructed
/// directly (a `CodegenError` + a registered source file) because every
/// reachable user-facing emitter refusal compiles today -- see
/// `every_shape_the_emitter_once_refused_now_compiles`.
#[test]
fn a_codegen_error_renders_a_located_excerpt() {
    let mut hir = zeo::hir::Hir::default();
    hir.add_file("-e", "x = 1\nlist.compile_me\n");
    let err = zeo::CompileError::from_codegen(
        zeo::codegen_error::CodegenError::unsupported(
            "the CLIF backend cannot lower this construct yet",
            Some(zeo::hir::Span {
                file: zeo::hir::FileId(0),
                start: 6,
                end: 21,
            }),
        ),
        &hir.files,
    );
    insta::assert_snapshot!(render(err));
}

/// The same for an FFI `layout`, which takes its own dispatch arm.
#[test]
fn an_ffi_layout_names_its_own_line() {
    let err = zeo::check_program_with(
        "require 'ffi'\nclass Rec < FFI::Struct\n  layout :a, :int,\n         :b, :nope_type\nend\n",
        &Default::default(),
    )
    .expect_err("an undeclared field type is rejected");
    insta::assert_snapshot!(render(err));
}

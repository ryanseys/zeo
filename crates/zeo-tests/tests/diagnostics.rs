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

/// A construct the emitter cannot lower reports as an error rather than
/// unwinding, which is what lets an unsupported repro be checked in as an
/// XFAIL gap.
///
/// **This test is about the REPORT, not the construct.** The probe has moved
/// three times as refusals closed: `Foo&.bar` (refused only by the retired
/// rustc emitter), a refined call through `&.`, then a splat at a refined
/// call. Re-point it at another live refusal rather than deleting the test.
#[test]
fn an_unsupported_construct_is_an_error_not_a_panic() {
    // Through EMISSION, not just the front end: `fx.unsupported` is the
    // emitter's own refusal channel, and `--emit-clif` is the cheapest way
    // to reach it (no object file, no linker).
    let err = zeo::compile_to_clif_text("3.times { |a, b| p [a, b] }\n", &Default::default())
        .expect_err("a fused loop with a multi-parameter block is rejected");
    insta::assert_snapshot!(render(err));
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

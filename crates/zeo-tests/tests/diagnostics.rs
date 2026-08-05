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
    let err = zeo::compile_to_rust_with("x = 1\np(/foo/e)\n", &Default::default())
        .expect_err("the e-flag regexp is rejected");
    insta::assert_snapshot!(render(err));
}

/// Prism rejects the source before any lowering frame exists, so this
/// renders span-less: message, code, and help only.
#[test]
fn a_parse_failure_renders_without_an_excerpt() {
    let err = zeo::compile_to_rust_with("def foo(\n", &Default::default())
        .expect_err("malformed source is rejected");
    insta::assert_snapshot!(render(err));
}

/// A construct codegen can't emit reports as an error rather than unwinding,
/// which is what lets an unsupported repro be checked in as an XFAIL gap.
#[test]
fn an_unsupported_codegen_construct_is_an_error_not_a_panic() {
    let err = zeo::compile_to_rust_with(
        "class Foo\n  def self.bar = 1\nend\np Foo&.bar\n",
        &Default::default(),
    )
    .expect_err("safe navigation on a class-method call is rejected");
    insta::assert_snapshot!(render(err));
}

/// A failure past lowering carries its stage as the diagnostic code
/// (message-only until analyze sites gain spans).
#[test]
fn an_analyze_error_is_coded_with_its_stage() {
    let err = zeo::compile_to_rust_with("class Foo\nend\nmodule Foo\nend\n", &Default::default())
        .expect_err("reopening a class as a module fails analyze");
    insta::assert_snapshot!(render(err));
}

//! A run-time `eval` through the REAL compiler (`ZEO_EVAL=compiler`,
//! plan G6) against the same program through the interpreter.
//!
//! Both must answer what CRuby answers, so each case carries the oracle's
//! own output and both evaluators are held to it. That is the whole
//! burn-down discipline: the interpreter is the differential oracle for
//! every shape until nothing falls back to it (G6-4).

use crate::support::{RunResult, run_ruby, run_ruby_configured};

/// The program under both evaluators. A compiled program installs the
/// compiler through `ProgramDesc.eval_install`, so the switch is all that
/// differs between the two runs.
fn both(source: &str) -> (RunResult, RunResult) {
    let vm = run_ruby(source);
    let compiled = run_ruby_configured(source, &[("ZEO_EVAL", "compiler")], &[]);
    (vm, compiled)
}

fn agree(source: &str, expected: &str) {
    let (vm, compiled) = both(source);
    assert_eq!(vm.stdout, expected, "the interpreter diverges from CRuby");
    assert_eq!(
        compiled.stdout, expected,
        "the compiler diverges from CRuby (stderr: {})",
        compiled.stderr
    );
}

#[test]
fn an_expression_evaluates() {
    agree(
        r#"
        src = "1 + 1"
        p eval(src)
        p eval("2 * 3" + "")
        "#,
        "2\n6\n",
    );
}

#[test]
fn the_callers_locals_are_the_evals_locals() {
    agree(
        r#"
        n = 20
        src = "n * 3"
        p eval(src)
        write = "n = 7"
        eval(write)
        p n
        "#,
        "60\n7\n",
    );
}

#[test]
fn a_block_runs_inside_the_source() {
    agree(
        r#"
        src = "[3,1,2].sort.map { |x| x * x }"
        p eval(src)
        "#,
        "[1, 4, 9]\n",
    );
}

#[test]
fn a_rescue_clause_binds_inside_the_source() {
    // The compiler answers CRuby here where the INTERPRETER cannot: a
    // `rescue => e` inside an eval is one of the nodes the eval VM never
    // covered, and it raises `NotImplementedError`. The compiled path
    // lowers it like any other body, so this is a shape the burn-down
    // gains rather than merely keeps.
    let source = r#"
        rescued = "begin; Integer('x'); rescue ArgumentError => e; e.class; end"
        p eval(rescued)
        "#;
    let compiled = run_ruby_configured(source, &[("ZEO_EVAL", "compiler")], &[]);
    assert_eq!(
        compiled.stdout, "ArgumentError\n",
        "stderr: {}",
        compiled.stderr
    );
    assert!(
        run_ruby(source).stderr.contains("NotImplementedError"),
        "the eval VM was expected to decline this shape"
    );
}

#[test]
fn a_raise_names_the_snippet_and_the_eval_site() {
    agree(
        r#"
        src = "raise 'boom'"
        begin
          eval(src)
        rescue => e
          puts e.backtrace.first(2)
        end
        "#,
        "(eval at -e:4):1:in '<main>'\n-e:4:in 'Kernel#eval'\n",
    );
}

#[test]
fn file_and_line_report_the_eval_site() {
    agree(
        r#"
        f = "__FILE__"
        p eval(f)
        l = "__LINE__"
        p eval(l)
        "#,
        "\"(eval at -e:3)\"\n1\n",
    );
}

#[test]
fn a_shape_the_compiler_declines_still_runs() {
    // A `def` in the source installs on the runtime's overlay, which this
    // compiler does not emit yet -- the interpreter answers it, and the
    // program cannot tell.
    agree(
        r#"
        src = "def evaled; 41 + 1; end; evaled"
        p eval(src)
        p evaled
        "#,
        "42\n42\n",
    );
}

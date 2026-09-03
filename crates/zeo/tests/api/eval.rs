//! A run-time `eval`, which zeo COMPILES (plan G6).
//!
//! Every case carries CRuby's own output. A prism-walking interpreter
//! answered these until 2026-08-21, as the differential oracle each
//! widening of the compiled path was measured against; the cases it never
//! covered say so where they are.

use crate::support::{RunResult, run_ruby};

fn run(source: &str) -> RunResult {
    run_ruby(source)
}

fn agree(source: &str, expected: &str) {
    let out = run(source);
    assert_eq!(
        out.stdout, expected,
        "the compiler diverges from CRuby (stderr: {})",
        out.stderr
    );
}

#[test]
fn a_rescue_clause_binds_inside_the_source() {
    // The compiler answers CRuby here where the INTERPRETER cannot: a
    // A `rescue => e` inside an eval is one of the nodes the retired
    // interpreter never covered. The compiled path lowers it like any
    // other body.
    let source = r#"
        rescued = "begin; Integer('x'); rescue ArgumentError => e; e.class; end"
        p eval(rescued)
        "#;
    agree(source, "ArgumentError\n");
}

#[test]
fn a_def_installs_at_its_document_position() {
    // `CompileMode::Eval` registers nothing, so the `def` reaches the
    // emitter's run-time install arm -- and installs PRIVATE on Object,
    // which is what a top-level `def` is. The retired interpreter got the
    // visibility wrong here (it answered `false`).
    let source = r#"
        src = "def evaled; 41 + 1; end; evaled"
        p eval(src)
        p evaled
        p self.class.private_instance_methods(false).include?(:evaled)
        "#;
    agree(source, "42\n42\ntrue\n");
}

#[test]
fn an_evaled_def_binds_the_whole_parameter_surface() {
    let source = r#"
        src = "def with_args(a, b = 2, *r, k: 3, &blk); [a, b, r, k, blk&.call]; end"
        eval(src)
        p with_args(1)
        p with_args(1, 5, 6, 7, k: 9) { :blk }
        "#;
    agree(source, "[1, 2, [], 3, nil]\n[1, 5, [6, 7], 9, :blk]\n");
}

#[test]
fn a_block_in_the_source_writes_the_callers_local() {
    // prism parsed the snippet alone, so it marked `total` a block-local:
    // it could not see the caller's declaration. The name the eval's own
    // scope holds came from the Binding, so the caller assigned it first
    // and ruby shares it -- both the capture and the per-invocation nil
    // fill have to agree about that -- a shape the retired interpreter
    // never covered (a block with a parameter inside an eval).
    let source = r#"
        total = 5
        src = "[1,2].each { |x| total += x }; total"
        p eval(src)
        p total
        "#;
    agree(source, "8\n8\n");
}

#[test]
fn defined_calls_a_callers_local_a_local() {
    // prism could only call it a vcall (it parsed the snippet alone), so
    // `defined?` reported an undefined method for a name the caller
    // holds. The retired interpreter answered `nil` here; CRuby and the
    // compiler agree
    // on "local-variable".
    let source = r#"
        loc = 5
        here = "defined?(loc)"
        gone = "defined?(nope)"
        p eval(here)
        p eval(gone)
        "#;
    agree(source, "\"local-variable\"\nnil\n");
}

#[test]
fn a_class_variable_and_a_constant_belong_to_the_cref() {
    // Both are owned by the class the eval runs in, which is a RUN-TIME
    // class -- so the id goes straight through, and the constant lands on
    // the receiver rather than on `Object`. The `@@n` needed the LOWERING
    // to know too: a snippet has no lexical cref, which is not the same as
    // being at the top level, and the fold that stands `@@x` up as ruby's
    // toplevel RuntimeError answered that question at compile time.
    let source = r#"
        class Counter
          @@n = 1
          def self.n = @@n
          SEED = 10
        end
        bump = "@@n += SEED; @@n"
        p Counter.class_eval(bump)
        p Counter.n
        add = "GRAND = @@n * 2"
        Counter.class_eval(add)
        p Counter::GRAND
        p Object.const_defined?(:GRAND)
        "#;
    agree(source, "11\n11\n22\nfalse\n");
}

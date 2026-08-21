//! A run-time `eval` through the REAL compiler (`ZEO_EVAL=compiler`,
//! plan G6) against the same program through the interpreter.
//!
//! Both must answer what CRuby answers, so each case carries the oracle's
//! own output and both evaluators are held to it. That is the whole
//! burn-down discipline: the interpreter is the differential oracle for
//! every shape until nothing falls back to it (G6-4).

use crate::support::{RunResult, run_ruby_configured};

/// The program under both evaluators. A compiled program installs the
/// compiler through `ProgramDesc.eval_install`, so the switch is all that
/// differs between the two runs.
fn both(source: &str) -> (RunResult, RunResult) {
    (interpreted(source), compiled(source))
}

/// Each leg names its evaluator explicitly: only `compiler` selects the
/// compiler, so anything else pins the interpreter whatever `ZEO_EVAL`
/// the test run itself was started with.
fn interpreted(source: &str) -> RunResult {
    run_ruby_configured(source, &[("ZEO_EVAL", "interpreter")], &[])
}

fn compiled(source: &str) -> RunResult {
    run_ruby_configured(source, &[("ZEO_EVAL", "compiler")], &[])
}

fn agree(source: &str, expected: &str) {
    let (vm, run) = both(source);
    assert_eq!(vm.stdout, expected, "the interpreter diverges from CRuby");
    assert_eq!(
        run.stdout, expected,
        "the compiler diverges from CRuby (stderr: {})",
        run.stderr
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
    let run = compiled(source);
    assert_eq!(run.stdout, "ArgumentError\n", "stderr: {}", run.stderr);
    assert!(
        interpreted(source).stderr.contains("NotImplementedError"),
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
fn a_def_installs_at_its_document_position() {
    // `CompileMode::Eval` registers nothing, so the `def` reaches the
    // emitter's run-time install arm -- and installs PRIVATE on Object,
    // which is what a top-level `def` is. The interpreter gets the
    // visibility wrong here (it answers `false`), so this is the second
    // shape the compiled path fixes rather than keeps.
    let source = r#"
        src = "def evaled; 41 + 1; end; evaled"
        p eval(src)
        p evaled
        p self.class.private_instance_methods(false).include?(:evaled)
        "#;
    let run = compiled(source);
    assert_eq!(run.stdout, "42\n42\ntrue\n", "stderr: {}", run.stderr);
}

#[test]
fn an_evaled_def_binds_the_whole_parameter_surface() {
    let source = r#"
        src = "def with_args(a, b = 2, *r, k: 3, &blk); [a, b, r, k, blk&.call]; end"
        eval(src)
        p with_args(1)
        p with_args(1, 5, 6, 7, k: 9) { :blk }
        "#;
    let run = compiled(source);
    assert_eq!(
        run.stdout, "[1, 2, [], 3, nil]\n[1, 5, [6, 7], 9, :blk]\n",
        "stderr: {}",
        run.stderr
    );
}

#[test]
fn instance_eval_binds_the_receiver_and_its_singleton() {
    agree(
        r#"
        obj = Object.new
        obj.instance_variable_set(:@v, 3)
        read = "@v * 2"
        p obj.instance_eval(read)
        defn = "def dbl; @v * 4; end"
        obj.instance_eval(defn)
        p obj.dbl
        "#,
        "6\n12\n",
    );
}

#[test]
fn class_eval_resolves_constants_in_the_receiver() {
    // The receiver is a class only the RUN TIME knows -- the snippet's own
    // compiler has no entry for it, so its id travels as an immediate and
    // every static fold stands down. The `def`'s body inherits the same
    // cref, which is what makes `def tell = SECRET` find `Host::SECRET`.
    agree(
        r#"
        class Host
          SECRET = :host_secret
        end
        reader = "SECRET"
        p Host.class_eval(reader)
        defn = "def tell = SECRET"
        Host.class_eval(defn)
        p Host.new.tell
        p Host.private_instance_methods(false)
        "#,
        ":host_secret\n:host_secret\n[]\n",
    );
}

#[test]
fn a_block_in_the_source_writes_the_callers_local() {
    // prism parsed the snippet alone, so it marked `total` a block-local:
    // it could not see the caller's declaration. The name the eval's own
    // scope holds came from the Binding, so the caller assigned it first
    // and ruby shares it -- both the capture and the per-invocation nil
    // fill have to agree about that. A third shape the eval VM never
    // covered (a block with a parameter inside an eval), so the compiler
    // is held to CRuby alone.
    let source = r#"
        total = 5
        src = "[1,2].each { |x| total += x }; total"
        p eval(src)
        p total
        "#;
    let run = compiled(source);
    assert_eq!(run.stdout, "8\n8\n", "stderr: {}", run.stderr);
    assert!(
        interpreted(source).stderr.contains("NotImplementedError"),
        "the eval VM was expected to decline this shape"
    );
}

#[test]
fn a_def_in_the_source_has_a_home_of_its_own() {
    // `super`, `yield` and `return` need the enclosing method's identity,
    // block channel and return target -- which a snippet's own level does
    // not have, but a `def` written INSIDE it does: the emitter's
    // run-time-installed body reads its defining class off the method
    // frame stack. So the refusal is about where they sit, not what they
    // are.
    agree(
        r#"
        class Base
          def greet = "base"
        end
        class Kid < Base; end
        src = "def greet; %(kid+) + super; end"
        Kid.class_eval(src)
        p Kid.new.greet
        yielder = "def y_it; yield 5; end"
        eval(yielder)
        p(y_it { |v| v * 3 })
        "#,
        "\"kid+base\"\n15\n",
    );
}

#[test]
fn defined_calls_a_callers_local_a_local() {
    // prism could only call it a vcall (it parsed the snippet alone), so
    // `defined?` reported an undefined method for a name the caller
    // holds. The eval VM answers `nil` here; CRuby and the compiler agree
    // on "local-variable".
    let source = r#"
        loc = 5
        here = "defined?(loc)"
        gone = "defined?(nope)"
        p eval(here)
        p eval(gone)
        "#;
    let run = compiled(source);
    assert_eq!(
        run.stdout, "\"local-variable\"\nnil\n",
        "stderr: {}",
        run.stderr
    );
    assert_eq!(interpreted(source).stdout, "nil\nnil\n");
}

#[test]
fn a_shape_the_compiler_declines_still_runs() {
    // A `class` in the source mints a run-time class, which this compiler
    // does not lower yet -- the interpreter answers it, and the program
    // cannot tell which one ran.
    agree(
        r#"
        src = "class Minted; def hi; :hi; end; end; Minted.new.hi"
        p eval(src)
        p Minted.name
        "#,
        ":hi\n\"Minted\"\n",
    );
}

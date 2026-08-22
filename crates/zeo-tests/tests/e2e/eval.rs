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

#[test]
fn a_mixin_in_the_source_is_the_send_ruby_writes() {
    // A whole-program compile records `include M` as a compile-time
    // ancestry EDIT and emits no statement for it. A snippet has no class
    // table to edit into, so analyze must leave the marker alone and the
    // emitter sends it -- receiverless, because ruby's top-level `include`
    // is a private method on `main`.
    agree(
        r#"
        module Greeter
          def hello = "hello from #{self.class}"
          def self.included(base) = puts("included into #{base}")
        end
        class Plain; end
        src = "include Greeter"
        Plain.class_eval(src)
        p Plain.new.hello
        p Plain.ancestors.include?(Greeter)
        obj = Object.new
        ext = "extend Greeter"
        obj.instance_eval(ext)
        p obj.hello
        "#,
        "included into Plain\n\"hello from Plain\"\ntrue\n\"hello from Object\"\n",
    );
}

#[test]
fn a_class_in_the_source_mints_and_runs() {
    // The header mints the class through the runtime; the BODY runs as one
    // more `class_eval` of its own source, which is what a class body IS
    // -- a scope of its own, with its own cref.
    agree(
        r#"
        src = "class Minted; def hi; :hi; end; end; Minted.new.hi"
        p eval(src)
        p Minted.name
        "#,
        ":hi\n\"Minted\"\n",
    );
}

#[test]
fn a_class_name_already_taken_is_a_type_error() {
    // Ruby raises before anything is minted. (It also warns
    // `previous definition of TOP was here`, which zeo does not track --
    // hence the stdout-only check rather than a golden.)
    agree(
        r#"
        TOP = 1
        src = "class TOP; end"
        begin
          eval(src)
        rescue TypeError => e
          puts e.message
        end
        "#,
        "TOP is not a class\n",
    );
}

#[test]
fn every_shape_an_eval_once_declined_now_compiles() {
    // There is no second evaluator to hand a declined shape to, so a shape
    // the compiler cannot lower raises `NotImplementedError` naming itself.
    // This test used to pin one such message -- and it had to be re-pointed
    // every time a shape closed, which is a test that rots into a lie. It
    // pins the CLOSURES instead: every shape a snippet once declined, each
    // answering what ruby answers.
    agree(
        r#"
        require "ffi"
        # an `FFI::Library` declaration: the directives stay ordinary calls
        # in a snippet, and the module's own rows attach at run time.
        eval <<~SRC
          module EvalLib
            extend FFI::Library
            ffi_lib FFI::Library::LIBC
            attach_function :abs, [:int], :int
          end
        SRC
        p EvalLib.abs(-3)
        # a `class < FFI::Struct`: its `layout` is replaced in place by
        # synthesized accessors, whose own source is the body text that runs.
        eval "class EvalStruct < FFI::Struct; layout :a, :int, :b, :int; end"
        p EvalStruct.size
        # `refine`/`using` at run time. (A `Ruby::Box` closed with them and
        # is pinned by `tests/gaps/a_snippet_mints_no_compile_time_box.rb`,
        # which the golden harness runs with `RUBY_BOX=1`.)
        eval "module EvalRef; refine(String) { def shout = upcase + '!' }; end"
        p eval("using EvalRef; 'hi'.shout")
        "#,
        "3\n8\n\"HI!\"\n",
    );
}

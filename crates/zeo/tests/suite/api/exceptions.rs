use crate::support::{compile_packages, run_ruby, run_ruby_packages};

#[test]
fn parse_error_is_a_clean_error_not_a_panic() {
    let err = zeo::check_program("def foo(\n").unwrap_err();
    assert!(
        err.contains("parse error"),
        "expected a parse error, got: {err}"
    );
}

#[test]
fn eval_of_a_class_definition_is_not_a_compile_error() {
    // The inline path can't express a top-level `class`/`def`, so a literal
    // `eval("class Foo; end")` falls through to the runtime VM instead of
    // failing the compile. (Actually DEFINING a class inside eval is a later
    // increment; the point here is that it compiles.)
    assert!(zeo::check_program(r#"eval("class Foo; end")"#).is_ok());
}

// --- Exceptions (begin/rescue/else/ensure/retry, raise, custom
// hierarchies), oracle-verified against real `ruby` first. `e`'s method
// calls (`e.message`) always go through `.send(:message)` -- a rescue
// binding is deliberately never narrowed to a concrete class (unlike a
// pattern's `Integer => n`): `rescue StandardError => e` must
// also match any raised SUBCLASS instance, so the binding stays
// dynamically typed -- see `clif::control::lower_begin`.

// --- Deeper edge-case and composition coverage, added
// after the initial batch above per an explicit request for more
// comprehensive tests. Every scenario oracle-verified against real `ruby`
// first, per this project's established convention.

/// `if /foo/` is `\/foo/ =~ $_`, and the corpus golden holds the answers --
/// this only pins that the form reaches codegen.
#[test]
fn a_bare_regexp_literal_used_as_an_implicit_condition_lowers() {
    zeo::check_program(
        r#"
        if /foo/
          puts "matched"
        end
        "#,
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// .freeze / .frozen? -- every snippet oracle-verified against
// real `ruby` first, per this project's standing convention. Semantics
// grounded in CRuby's actual implementation: freeze is SHALLOW, returns
// self, no-ops when repeated;
// immediates and Ranges are always frozen; mutation of a frozen value
// raises a catchable FrozenError (a RuntimeError subclass) with the message
// `can't modify frozen <Class>: <inspect>`, checked at the top of every
// mutator after argument evaluation.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// `send`'s missing-method fallback raises a real, catchable
// NoMethodError (via the factory generated main() installs) instead of the
// original whole-process `exit(1)` -- which would have killed every OTHER
// running Thread over one bad dispatch. Oracle-verified.
// ---------------------------------------------------------------------------

#[test]
fn bad_manifests_are_loud_configuration_errors() {
    // Name/directory mismatch.
    let err = compile_packages(
        &[
            // Gem named "bbb" living in a directory named "aaa".
            (
                "packages/aaa/aaa.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"bbb\"\n  s.version = \"1.0.0\"\nend\n",
            ),
            ("packages/aaa/lib/aaa.rb", "puts 1\n"),
            ("main.rb", "puts :ok\n"),
        ],
        "main.rb",
        &[],
        &["packages"],
    )
    .unwrap_err();
    assert!(
        err.contains("doesn't match its directory name"),
        "unexpected error: {err}"
    );

    // A gemspec that sets no name.
    let err = compile_packages(
        &[
            (
                "packages/aaa/aaa.gemspec",
                "Gem::Specification.new do |s|\n  s.version = \"1.0.0\"\nend\n",
            ),
            ("main.rb", "puts :ok\n"),
        ],
        "main.rb",
        &[],
        &["packages"],
    )
    .unwrap_err();
    assert!(err.contains("sets no `name`"), "unexpected error: {err}");
}

#[test]
fn a_metagem_whose_declared_lib_is_absent_contributes_no_load_path() {
    // `rails` is the real case: its gemspec declares `require_paths: [lib]` and
    // the gem ships only a README and a licence. RubyGems puts the missing
    // directory on `$LOAD_PATH` regardless and `require` simply never matches
    // inside it, so a metagem sitting in the gem set must not fail the build --
    // 43 gems in one corpus sweep depended on this. Verified against ruby 4.0.6.
    let result = run_ruby_packages(
        &[
            (
                "packages/aaa/aaa.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"aaa\"\n  s.version = \"1.0.0\"\nend\n",
            ),
            ("packages/aaa/README.md", "a metagem\n"),
            (
                "packages/bbb/bbb.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"bbb\"\n  s.version = \"1.0.0\"\nend\n",
            ),
            ("packages/bbb/lib/bbb.rb", "BBB = :loaded\n"),
            (
                "main.rb",
                "require \"bbb\"\np BBB\nbegin\n  require \"aaa\"\nrescue LoadError => e\n  \
                 puts e.message\nend\nputs :ok\n",
            ),
        ],
        "main.rb",
        &[],
        &["packages"],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, ":loaded\ncannot load such file -- aaa\nok\n");
}

#[test]
fn reopen_guards_mirror_rubys_type_errors() {
    // `TypeError: superclass mismatch for class Sub` in real Ruby -- an
    // exception at the definition, so the program compiles and raises it
    // there. `Sub` keeps the parent the definition that RAISED never changed.
    let result = run_ruby(
        r#"
        class Base; end
        class Other; end
        class Sub < Base; end
        begin
          class Sub < Other; end
        rescue TypeError => e
          puts e.message
        end
        puts Sub.superclass
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "superclass mismatch for class Sub\nBase\n");

    // Restating the ORIGINAL superclass is allowed (real Ruby).
    let result = run_ruby(
        r#"
        class Base
        end

        class Sub < Base
        end

        class Sub < Base
          def ok
            "explicit matching superclass ok"
          end
        end

        puts Sub.new.ok
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "explicit matching superclass ok\n");

    // A kind collision is `TypeError` too, with ruby's second line naming
    // where the constant was first bound.
    let result = run_ruby(
        r#"
        class Foo; end
        module Bar; end
        begin
          module Foo; end
        rescue TypeError => e
          puts e.message.lines.first
        end
        begin
          class Bar; end
        rescue TypeError => e
          puts e.message.lines.first
        end
        puts Foo.instance_of?(Class)
        puts Bar.instance_of?(Module)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Foo is not a module\nBar is not a class\ntrue\ntrue\n"
    );
}

#[test]
fn super_outside_any_method_raises_at_runtime_not_compile_time() {
    // `super` at top level (or in a top-level block) is a RUNTIME
    // NoMethodError in real Ruby -- "super called outside of method",
    // rescuable like any other raise (vm_insnhelper.c). The compiler used to
    // panic on the missing method context; now it emits that raise. The
    // block form also pins the `block in <main>` frame label and that no
    // orphaned `__blk` forwarding capture is emitted where no lexical block
    // binding exists.
    let rescued = run_ruby(
        r#"
        begin
          super
        rescue NoMethodError => e
          puts "top: #{e.message}"
        end
        [1].each do
          super
        rescue NoMethodError => e
          puts "block: #{e.message}"
        end
        "#,
    );
    assert!(rescued.status.success(), "stderr: {}", rescued.stderr);
    assert_eq!(
        rescued.stdout,
        "top: super called outside of method\nblock: super called outside of method\n",
    );

    let uncaught = run_ruby("[1].each { super }");
    assert!(!uncaught.status.success());
    assert!(
        uncaught
            .stderr
            .contains("in 'block in <main>': super called outside of method (NoMethodError)"),
        "stderr: {}",
        uncaught.stderr
    );
}

// --- Eager-codegen panics that name a legitimate runtime error are
// deferred to a runtime raise, so an undefined-constant reference in a dead or
// rescued branch compiles cleanly (CRuby only raises `uninitialized constant`
// if the branch actually runs) instead of aborting the whole compile.

// `rescue *exprs => e` -- a rescue clause whose exception list is a SPLATTED
// runtime array of classes (or a single class), rather than literal constants.
// This is the `rescue *yaml_errors => e` shape rubygems uses. Matched at
// runtime; composes with static classes, multiple clauses, ensure, retry, and
// module-include matching.

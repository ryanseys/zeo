//! In-process end-to-end tests via `support::run_ruby` (see its module docs).
//! Ports the 7 `examples/*.rb` golden fixtures to real Rust `#[test]`
//! functions asserting stdout/stderr/exit status separately, plus a
//! zero-subprocess negative-path tier (`compile_to_rust(...).unwrap_err()`)
//! for compile errors. `examples/*.rb` + `.expected` themselves are left in
//! place, still exercised by `cargo run -p xtask -- test`, as a smaller
//! "real CLI + real `ruby`-oracle" smoke suite -- this file is the new
//! default place to add coverage.

mod support;
use support::run_ruby;

#[test]
fn hello_prints_a_symbol() {
    let result = run_ruby("puts :ok");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "ok\n");
    assert_eq!(result.stderr, "");
}

#[test]
fn arith_adds_integer_literals() {
    let result = run_ruby("puts 1 + 1");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n");
}

#[test]
fn blocks_times_inlines_to_a_native_loop() {
    let result = run_ruby(
        r#"
        3.times do |i|
          puts i
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "0\n1\n2\n");
}

#[test]
fn ivars_store_real_per_instance_state() {
    let result = run_ruby(
        r#"
        class Point
          def initialize(x)
            @x = x
          end

          def x
            @x
          end
        end

        puts Point.new(5).x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\n");
}

#[test]
fn inheritance_super_chain() {
    let result = run_ruby(
        r#"
        class Animal
          def speak
            puts :generic
          end
        end

        class Dog < Animal
          def speak
            super
            puts :woof
          end
        end

        Dog.new.speak
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "generic\nwoof\n");
}

#[test]
fn dynamic_send_with_a_non_literal_target() {
    let result = run_ruby(
        r#"
        class Greeter
          define_method(:hello) do
            puts :hi
          end
        end

        name = :hello
        Greeter.new.send(name)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi\n");
}

#[test]
fn method_missing_fallback_on_a_total_miss() {
    let result = run_ruby(
        r#"
        class Greeter
          def method_missing(name)
            puts :missing
          end
        end

        Greeter.new.send(:nonexistent)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "missing\n");
}

#[test]
fn operators_work_on_locals_not_just_literals() {
    // Pre-Phase-1, only a literal-on-literal `+` compiled at all. This
    // proves the generalization: arithmetic on locals typed `Int` by the
    // forward local-type tracker (`analyze::locals`).
    let result = run_ruby(
        r#"
        x = 5
        y = 3
        puts x + y
        puts x - y
        puts x * y
        puts x / y
        puts x % y
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "8\n2\n15\n1\n2\n");
}

#[test]
fn comparison_operators_return_real_booleans() {
    let result = run_ruby(
        r#"
        puts 5 < 3
        puts 5 > 3
        puts 5 == 5
        puts 5 != 3
        puts 5 <= 5
        puts 5 >= 6
        puts(1 <=> 2)
        puts(2 <=> 2)
        puts(3 <=> 2)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "false\ntrue\ntrue\ntrue\ntrue\nfalse\n-1\n0\n1\n"
    );
}

#[test]
fn unary_operators() {
    let result = run_ruby(
        r#"
        x = 5
        puts(-x)
        puts(+x)
        puts(~x)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "-5\n5\n-6\n");
}

#[test]
fn boolean_and_or_return_the_operand_not_a_bool() {
    // Ruby's `&&`/`||` return the actual operand, not a coerced bool --
    // `false && x` is `false`, but `5 && x` is `x`, not `true`.
    let result = run_ruby(
        r#"
        x = 5
        y = 3
        puts((x > y) && :yes)
        puts((x < y) && :yes)
        puts((x < y) || :fallback)
        puts((x > y) || :fallback)
        puts(!(x > y))
        puts(!(x < y))
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "yes\nfalse\nfallback\ntrue\nfalse\ntrue\n");
}

#[test]
fn defined_classifies_syntactic_form() {
    let result = run_ruby(
        r#"
        x = 5
        puts(defined?(x))
        puts(defined?(1 + 1))

        class Foo
          def check
            @bar = 1
            puts(defined?(@bar))
          end
        end
        Foo.new.check
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "local-variable\nmethod\ninstance-variable\n");
}

#[test]
fn unsupported_syntax_is_a_clean_error_not_a_panic() {
    let err = spinelc::compile_to_rust("if true\n  puts 1\nend\n").unwrap_err();
    assert!(
        err.contains("unsupported syntax"),
        "expected an unsupported-syntax error, got: {err}"
    );
}

#[test]
fn parse_error_is_a_clean_error_not_a_panic() {
    let err = spinelc::compile_to_rust("def foo(\n").unwrap_err();
    assert!(
        err.contains("parse error"),
        "expected a parse error, got: {err}"
    );
}

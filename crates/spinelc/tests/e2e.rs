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
fn if_elsif_else_as_statement_and_as_value() {
    let result = run_ruby(
        r#"
        n = -5
        if n < 0
          puts :negative
        elsif n == 0
          puts :zero
        else
          puts :positive
        end

        n2 = 0
        result = if n2 < 0
          :negative
        elsif n2 == 0
          :zero
        else
          :positive
        end
        puts result
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "negative\nzero\n");
}

#[test]
fn unless_with_else() {
    let result = run_ruby(
        r#"
        n = 5
        unless n < 0
          puts :non_negative
        else
          puts :negative
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "non_negative\n");
}

#[test]
fn ternary() {
    let result = run_ruby(
        r#"
        n = 7
        puts(n > 5 ? :big : :small)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "big\n");
}

#[test]
fn case_when_with_and_without_a_subject() {
    let result = run_ruby(
        r#"
        day = :tue
        case day
        when :sat, :sun
          puts :weekend
        when :mon, :tue, :wed, :thu, :fri
          puts :weekday
        else
          puts :unknown
        end

        n = 7
        case
        when n < 0
          puts :negative
        when n == 0
          puts :zero
        else
          puts :positive
        end

        x = :z
        case x
        when :a
          puts :got_a
        else
          puts :fallback
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "weekday\npositive\nfallback\n");
}

#[test]
fn safe_navigation_on_a_non_nil_receiver() {
    // Only proves `&.` doesn't regress a normal, statically-typed dispatch --
    // a receiver that's *actually* nil at runtime needs a nilable/union type
    // this spike's `TyKind` doesn't have yet (every `New` is unconditionally
    // a concrete `Object(ClassId)`, never possibly-nil), so that half of
    // `&.`'s behavior isn't testable end to end until then.
    let result = run_ruby(
        r#"
        class Box
          def initialize(value)
            @value = value
          end
          def value
            @value
          end
        end

        b = Box.new(:present)
        puts(b&.value)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "present\n");
}

#[test]
fn array_literal_indexing_and_mutation() {
    let result = run_ruby(
        r#"
        a = [1, 2, 3]
        puts(a[0])
        puts(a[2])
        puts(a[-1])
        puts(a[10])
        a[1] = 99
        puts(a[1])
        puts(a.length)
        puts(a.size)

        rest = [3, 4]
        b = [1, 2, *rest, 5]
        puts(b.length)
        puts(b[2])
        puts(b[4])
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "1\n3\n3\n\n99\n3\n3\n5\n3\n5\n"
    );
}

#[test]
fn hash_literal_indexing_and_mutation() {
    let result = run_ruby(
        r#"
        h = { a: 1, b: 2 }
        puts(h[:a])
        puts(h[:b])
        puts(h[:missing])
        h[:c] = 3
        puts(h[:c])
        h[:a] = 10
        puts(h[:a])
        puts(h.length)
        puts(h.size)
        puts(h)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "1\n2\n\n3\n10\n3\n3\n{a: 10, b: 2, c: 3}\n"
    );
}

#[test]
fn range_literal_and_accessors() {
    let result = run_ruby(
        r#"
        r = 1..5
        puts(r.first)
        puts(r.last)
        puts(r.exclude_end?)
        puts(r)

        r2 = 1...5
        puts(r2.exclude_end?)
        puts(r2)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n5\nfalse\n1..5\ntrue\n1...5\n");
}

#[test]
fn string_literal_interpolation_indexing_and_mutation() {
    let result = run_ruby(
        r#"
        name = "world"
        greeting = "hello #{name}, #{1 + 2} times"
        puts(greeting)
        puts(greeting.length)
        puts(greeting[0])
        puts(greeting[-1])
        s = "cat"
        s[0] = "b"
        puts(s)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "hello world, 3 times\n20\nh\ns\nbat\n"
    );
}

#[test]
fn while_loop_with_break_and_next_values() {
    let result = run_ruby(
        r#"
        i = 0
        sum = 0
        while i < 10
          i += 1
          next if i == 5
          break if i == 8
          sum += i
        end
        puts sum
        puts i
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "23\n8\n");
}

#[test]
fn until_loop_and_modifier_forms() {
    let result = run_ruby(
        r#"
        n = 5
        until n == 0
          n -= 1
        end
        puts n

        count = 0
        count += 1 while count < 3
        puts count

        x = 10
        x -= 1 until x <= 5
        puts x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "0\n3\n5\n");
}

#[test]
fn loop_do_end_with_break_value() {
    let result = run_ruby(
        r#"
        i = 0
        result = loop do
          i += 1
          break i * 10 if i == 3
        end
        puts result
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "30\n");
}

#[test]
fn for_loop_over_range_and_array() {
    // Arithmetic on the Range case works because `for`'s index variable is
    // provably `Int` (see `codegen::loops::emit_for`'s `Ctx::for_var_override`
    // use); an `Array`'s elements aren't tracked per-element (`Poly`), so the
    // array case just displays each value -- arithmetic on a `Poly`-typed
    // value is an existing, documented gap (see `codegen::call`'s
    // `INT_BINARY_OPS` docs), not something this phase changes.
    let result = run_ruby(
        r#"
        sum = 0
        for i in 1..5
          sum += i
        end
        puts sum
        puts i

        arr = [10, 20, 30]
        for el in arr
          puts el
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "15\n5\n10\n20\n30\n");
}

#[test]
fn redo_reruns_the_current_iteration_without_retesting() {
    let result = run_ruby(
        r#"
        i = 0
        attempts = 0
        while i < 3
          attempts += 1
          if attempts < 5 && i == 1
            redo
          end
          i += 1
        end
        puts i
        puts attempts
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n6\n");
}

#[test]
fn multi_assign_with_and_without_a_splat() {
    let result = run_ruby(
        r#"
        a, b = 1, 2
        puts a
        puts b

        a, *b, c = [1, 2, 3, 4, 5]
        puts a
        puts b
        puts c
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n2\n1\n2\n3\n4\n5\n");
}

#[test]
fn unsupported_syntax_is_a_clean_error_not_a_panic() {
    // `begin`/`rescue` isn't supported until Phase 9 (exceptions) -- update
    // this to a still-unsupported construct if that lands and makes this
    // compile.
    let err = spinelc::compile_to_rust("begin\n  puts 1\nend\n").unwrap_err();
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

#[test]
fn eval_of_a_literal_string_runs_inline() {
    let result = run_ruby(r#"puts eval("1 + 2")"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n");
}

#[test]
fn eval_shares_the_enclosing_local_scope() {
    let result = run_ruby(
        r#"
        x = 1
        eval("x = x + 1")
        puts x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n");
}

#[test]
fn eval_can_write_an_ivar_on_self() {
    let result = run_ruby(
        r#"
        class Foo
          def initialize
            eval("@x = 42")
          end
          def x
            @x
          end
        end
        puts Foo.new.x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn eval_of_invalid_syntax_is_a_clean_compile_error() {
    let err = spinelc::compile_to_rust(r#"eval("1 +")"#).unwrap_err();
    assert!(
        err.contains("eval") && err.contains("parse error"),
        "expected eval's inner parse error to surface, got: {err}"
    );
}

#[test]
fn eval_of_a_non_literal_argument_is_a_clean_compile_error() {
    let err = spinelc::compile_to_rust("y = 1\neval(y.to_s)\n").unwrap_err();
    assert!(
        err.contains("non-literal argument"),
        "expected the non-literal-eval rejection, got: {err}"
    );
}

#[test]
fn eval_of_a_top_level_class_is_a_clean_compile_error() {
    let err = spinelc::compile_to_rust(r#"eval("class Foo; end")"#).unwrap_err();
    assert!(
        err.contains("top-level `class`/`def`"),
        "expected the top-level-def rejection, got: {err}"
    );
}

#[test]
fn optional_params_use_the_default_only_when_omitted() {
    let result = run_ruby(
        r##"
        class Greeter
          def greet(name, greeting = "Hello")
            "#{greeting}, #{name}!"
          end
        end
        g = Greeter.new
        puts g.greet("Ada")
        puts g.greet("Ada", "Hi")
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Hello, Ada!\nHi, Ada!\n");
}

#[test]
fn rest_and_post_params_bind_a_real_array() {
    let result = run_ruby(
        r##"
        class Collector
          def count(*nums)
            nums.length
          end
          def between(a, *mid, z)
            "#{a}-#{mid.length}-#{z}"
          end
        end
        c = Collector.new
        puts c.count(1, 2, 3)
        puts c.count
        puts c.between(1, 2, 3, 9)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n0\n1-2-9\n");
}

#[test]
fn keyword_params_bind_by_name_regardless_of_call_site_order() {
    let result = run_ruby(
        r#"
        class Kw
          def greet(x:, y: 10)
            x + y
          end
          def opts(**rest)
            rest.length
          end
        end
        k = Kw.new
        puts k.greet(x: 1)
        puts k.greet(x: 1, y: 2)
        puts k.greet(y: 3, x: 4)
        puts k.opts(a: 1, b: 2, c: 3)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "11\n3\n7\n3\n");
}

#[test]
fn attr_accessor_reader_and_writer_generate_real_methods() {
    let result = run_ruby(
        r#"
        class Box
          attr_accessor :value
          attr_reader :ro
          def initialize
            @value = 1
            @ro = 7
          end
        end
        b = Box.new
        puts b.value
        b.value = 99
        puts b.value
        puts b.ro
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n99\n7\n");
}

#[test]
fn visibility_keywords_are_recognized_and_do_not_error() {
    // `private`/`public`/`protected` are recognized and dropped (a
    // documented scope-cut -- not enforced yet, see
    // `parse::lower_class_body_statement`'s docs). This only exercises
    // "doesn't break compilation," not enforcement -- calling a private
    // method from another method of the same class needs `self.`/implicit-
    // self dispatch to a user-defined method, which is a separate,
    // pre-existing gap this phase doesn't touch (see
    // docs/PORTING_ANALYSIS.md).
    let result = run_ruby(
        r#"
        class Box
          def initialize
            @x = 1
          end
          private
          def helper
            2
          end
          public
          def x
            @x
          end
        end
        puts Box.new.x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n");
}

#[test]
fn endless_method_definition() {
    let result = run_ruby(
        r#"
        class Calc
          def double(x) = x * 2
        end
        puts Calc.new.double(21)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn arithmetic_on_a_plain_method_parameter_works() {
    // Method params are always statically `Poly` (spinelc never infers a
    // param's type from call sites) -- this exercises the runtime-checked
    // fallback in `codegen::call::dispatch`, not just literal/local `Int`
    // operands.
    let result = run_ruby(
        r#"
        class Adder
          def add(a, b)
            a + b
          end
        end
        puts Adder.new.add(3, 4)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\n");
}

#[test]
fn a_block_parameter_is_a_clean_compile_error() {
    let err = spinelc::compile_to_rust("class Foo\n  def bar(&blk)\n  end\nend\n").unwrap_err();
    assert!(
        err.contains("&block"),
        "expected the &block-parameter rejection, got: {err}"
    );
}

#[test]
fn forwarding_params_are_a_clean_compile_error() {
    let err = spinelc::compile_to_rust("class Foo\n  def bar(...)\n  end\nend\n").unwrap_err();
    assert!(
        err.contains("forwarding"),
        "expected the `...`-forwarding rejection, got: {err}"
    );
}

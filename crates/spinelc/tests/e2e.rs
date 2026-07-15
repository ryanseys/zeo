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
use support::run_ruby_project;

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
    // A double-splat (`**h`) inside a HASH LITERAL isn't supported yet
    // (spike scope) -- update this to a still-unsupported construct if that
    // ever lands and makes this compile. `begin`/`rescue` (Phase 9) and a
    // call-site splat (Phase 12.5) are no longer valid examples here.
    let err = spinelc::compile_to_rust("h = {a: 1}\nputs({b: 2, **h})\n").unwrap_err();
    assert!(
        err.contains("double-splat"),
        "expected a double-splat-related unsupported-syntax error, got: {err}"
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
fn visibility_keywords_switch_the_default_for_subsequent_defs() {
    // A bare `private`/`public` switches the DEFAULT visibility for every
    // subsequent `def` in the class body (see
    // `parse::lower_class_body_statement`'s docs) -- this test only checks
    // that a PUBLIC method compiles/runs normally after a `private` section;
    // see the dedicated visibility-enforcement tests below for the actual
    // private/protected/public_send rejection cases.
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
fn a_named_block_parameter_can_be_called_explicitly() {
    // A `&block` parameter is real syntax now (Phase 6 lifted the Phase-5
    // rejection this test used to check for).
    let result = support::run_ruby(
        "class Foo\n  def bar(&blk)\n    blk.call(5)\n  end\nend\nputs Foo.new.bar { |x| x * 2 }\n",
    );
    assert_eq!(result.stdout, "10\n");
}

#[test]
fn forwarding_params_are_a_clean_compile_error() {
    let err = spinelc::compile_to_rust("class Foo\n  def bar(...)\n  end\nend\n").unwrap_err();
    assert!(
        err.contains("forwarding"),
        "expected the `...`-forwarding rejection, got: {err}"
    );
}

// --- Phase 6: real escaping Proc/closures, yield, block_given?, self-capture ---

#[test]
fn yield_and_block_given_branch_on_whether_a_block_was_passed() {
    let result = run_ruby(
        r#"
        class Foo
          def maybe_yield
            if block_given?
              yield 42
            else
              -1
            end
          end
        end
        f = Foo.new
        puts f.maybe_yield { |x| x * 2 }
        puts f.maybe_yield
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "84\n-1\n");
}

#[test]
fn an_escaping_block_can_mutate_a_captured_enclosing_local() {
    let result = run_ruby(
        r#"
        class Collector
          def each_num(a, b, c)
            yield a
            yield b
            yield c
          end
        end
        total = 0
        Collector.new.each_num(1, 2, 3) { |n| total += n }
        puts total
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n");
}

#[test]
fn an_escaping_block_can_mutate_an_ivar_via_self_capture() {
    // Exercises the self:Rc<Self> migration + self-capture path: the block
    // is written inside `Box#run` (so `self`/`@sum` are in scope there) and
    // passed to a call on an EXPLICIT other receiver (`c`, a Collector
    // constructed directly as a LOCAL, not taken as a method parameter --
    // method params are always statically `Poly` in this spike, a separate
    // pre-existing gap; a `New`-assigned local's class IS statically known,
    // so this routes around that while still exercising real self-capture.
    // Implicit-self calls to a user method are ALSO a separate, unrelated,
    // still-unsupported gap, avoided here the same way).
    let result = run_ruby(
        r#"
        class Collector
          def each_num(a, b, c)
            yield a
            yield b
            yield c
          end
        end
        class Box
          def initialize
            @sum = 0
          end
          def total
            @sum
          end
          def run
            c = Collector.new
            c.each_num(1, 2, 3) { |n| @sum += n }
          end
        end
        b = Box.new
        b.run
        puts b.total
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n");
}

#[test]
fn break_and_next_inside_a_yielded_block_behave_like_real_ruby() {
    // `next` skips to the next yield; `break value` makes the WHOLE call
    // (`each_num(...)`) evaluate to that value, stopping further yields.
    let result = run_ruby(
        r#"
        class Collector
          def each_num(a, b, c)
            yield a
            yield b
            yield c
          end
        end
        result = Collector.new.each_num(1, 2, 3) { |n| next if n == 2; break "stopped" if n == 3; puts n }
        puts result
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\nstopped\n");
}

#[test]
fn redo_inside_a_yielded_block_reruns_without_advancing() {
    let result = run_ruby(
        r#"
        class Collector
          def each_num(a, b)
            yield a
            yield b
          end
        end
        attempts = 0
        Collector.new.each_num(1, 2) do |n|
          attempts += 1
          if n == 1 && attempts < 2
            redo
          end
          puts "n=#{n} attempts=#{attempts}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "n=1 attempts=2\nn=2 attempts=3\n");
}

#[test]
fn explicit_return_inside_a_block_exits_the_enclosing_method() {
    // `c` is a `New`-assigned LOCAL, not a method parameter -- see
    // `an_escaping_block_can_mutate_an_ivar_via_self_capture`'s comment for
    // why (params are always statically `Poly`, a separate pre-existing
    // gap unrelated to this test's actual point: `return` inside a real
    // escaping Proc unwinding all the way out of `find_even`, not just the
    // block/`.each_num` call).
    let result = run_ruby(
        r#"
        class Collector
          def each_num(a, b, c)
            yield a
            yield b
            yield c
          end
        end
        class Finder
          def find_even
            c = Collector.new
            c.each_num(1, 3, 4) { |n| return n if n % 2 == 0 }
            -1
          end
        end
        puts Finder.new.find_even
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "4\n");
}

#[test]
fn dynamic_send_carries_a_block_through_to_a_yield_using_method() {
    let result = run_ruby(
        r#"
        class Collector
          def each_num(a, b, c)
            yield a
            yield b
            yield c
          end
        end
        total = 0
        Collector.new.send(:each_num, 1, 2, 3) { |n| total += n }
        puts total
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n");
}

#[test]
fn numbered_params_work_through_the_times_inline_path() {
    let result = run_ruby("3.times { puts _1 * 10 }");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "0\n10\n20\n");
}

#[test]
fn numbered_params_work_through_a_real_escaping_proc() {
    let result = run_ruby(
        r#"
        class Collector
          def each_num(a, b)
            yield a
            yield b
          end
        end
        Collector.new.each_num(5, 6) { puts _1 * 2 }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n12\n");
}

#[test]
fn it_works_through_a_real_escaping_proc() {
    let result = run_ruby(
        r#"
        class Collector
          def each_num(a, b)
            yield a
            yield b
          end
        end
        Collector.new.each_num(7, 8) { puts it + 1 }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "8\n9\n");
}

#[test]
fn a_block_with_more_params_than_yielded_values_nil_fills_leniently() {
    // `puts` on `nil` prints an empty line -- real Ruby's own behavior, and
    // avoids `Object#inspect` (a separate, unrelated, unimplemented method).
    let result = run_ruby(
        r#"
        class Collector
          def each_one(a)
            yield a
          end
        end
        Collector.new.each_one(1) { |a, b| puts a; puts b }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n\n");
}

#[test]
fn keyword_params_on_a_block_bind_from_a_trailing_yielded_hash() {
    let result = run_ruby(
        r#"
        class Collector
          def emit
            yield x: 1, y: 2
          end
        end
        Collector.new.emit { |x:, y: 10| puts x + y }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n");
}

#[test]
fn a_named_block_param_can_be_forwarded_to_another_call() {
    // `c` is a `New`-assigned LOCAL inside `go`, not a parameter or an
    // implicit-self call target -- see the self-capture test's comment for
    // why (both are separate, pre-existing, unrelated gaps).
    let result = run_ruby(
        r#"
        class Collector
          def each_num(a, b)
            yield a
            yield b
          end
        end
        class Relay
          def go(a, b, &blk)
            c = Collector.new
            c.each_num(a, b, &blk)
          end
        end
        total = 0
        Relay.new.go(3, 4) { |n| total += n }
        puts total
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\n");
}

#[test]
fn a_block_escaping_from_inside_another_escaping_block_works() {
    // Phase 6's blanket rejection of Proc-within-Proc was LIFTED in Phase
    // 13.5 (`Thread.new { m.synchronize { } }` is the canonical threading
    // idiom): method-level captures are shared cells that compose through
    // any nesting depth. This exact snippet was that rejection's own
    // negative test -- now a positive one, oracle-verified.
    let result = run_ruby(
        r#"
        class Collector
          def each_num(a, b)
            yield a
            yield b
          end
        end
        c = Collector.new
        c.each_num(1, 2) { |n| c.each_num(n, n) { |m| puts m } }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n1\n2\n2\n");
}

#[test]
#[should_panic(expected = "capturing its enclosing BLOCK's own local `n`")]
fn a_nested_block_capturing_the_outer_blocks_own_local_is_a_clean_compile_error() {
    // The one nesting sub-case still rejected (see
    // `codegen::call::emit_proc_or_lambda_value`'s guard): the INNER block
    // reads the OUTER block's own param/local -- a plain per-invocation
    // binding, not a shared cell, which a `move` closure can't share
    // correctly. Clean codegen-time panic, not a silent nil.
    let _ = spinelc::compile_to_rust(
        r#"
        class Collector
          def each_num(a, b)
            yield a
            yield b
          end
        end
        c = Collector.new
        c.each_num(1, 2) { |n| c.each_num(3, 4) { |m| puts m + n } }
        "#,
    );
}

#[test]
fn yield_inside_a_nested_block_literal_is_a_clean_compile_error() {
    // `yield`/`block_given?` lexically inside a block passed elsewhere
    // refers to a DIFFERENT enclosing method's block in real Ruby -- a
    // documented spike-scope rejection (see `analyze::scan_bare_block_use`),
    // checked during `register_class`, before codegen ever runs.
    let err = spinelc::compile_to_rust(
        r#"
        class Foo
          def helper(x)
            yield x
          end
          def bar
            helper(1) { yield }
          end
        end
        "#,
    )
    .unwrap_err();
    assert!(
        err.contains("nested block"),
        "expected the nested-yield rejection, got: {err}"
    );
}

// --- Phase 7: full MRO (include/extend/prepend), inherited ivars, class
// variables, minimal raise/exception foundation -- oracle-verified against
// real `ruby` first, per this project's established convention.

#[test]
fn a_subclass_calls_a_non_overridden_inherited_method() {
    // Closes a real, pre-existing latent gap: today's spike only ever
    // generated a Rust method for a class's own LITERAL methods --
    // `Dog.new.speak` with no override at all would fail to compile (Rust
    // has no cross-struct inherent-method inheritance). `analyze::mro`'s
    // materialization fixes this as a byproduct of doing modules correctly
    // (every MRO-reachable method gets its own Scope on the receiver).
    let result = run_ruby(
        r#"
        class Animal
          def speak
            "generic"
          end
        end
        class Dog < Animal
        end
        puts Dog.new.speak
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "generic\n");
}

#[test]
fn a_class_own_method_shadows_an_included_module_method() {
    let result = run_ruby(
        r#"
        module Greetable
          def greet
            "hi"
          end
        end
        class Person
          include Greetable
          def greet
            "overridden"
          end
        end
        class Robot
          include Greetable
        end
        puts Person.new.greet
        puts Robot.new.greet
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "overridden\nhi\n");
}

#[test]
fn super_from_an_override_reaches_an_included_module_method() {
    let result = run_ruby(
        r#"
        module Tagged
          def tag
            "base(#{super})"
          end
        end
        class Widget
          include Tagged
          def tag
            "widget"
          end
        end
        puts Widget.new.tag
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "widget\n");
}

#[test]
fn stacked_prepends_resolve_in_reverse_declaration_order() {
    // `prepend M1; prepend M2` -- M2 (most recently prepended) is closest,
    // ahead of M1, ahead of the class's own definition.
    let result = run_ruby(
        r#"
        module M1
          def label
            "m1"
          end
        end
        module M2
          def label
            "m2"
          end
        end
        class Stacked
          prepend M1
          prepend M2
          def label
            "own"
          end
        end
        puts Stacked.new.label
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "m2\n");
}

#[test]
fn a_module_of_module_diamond_dispatches_and_is_a_resolves_transitively() {
    // The exact shape spinel's OWN reflection gets wrong (verified by
    // running a repro against spinel's own binary): `D` included via two
    // separate paths (`B`/`C`, both including `D`) must be deduped to a
    // single shared position, and `is_a?(D)` must resolve `true` even
    // though `D` was never included DIRECTLY by `A`.
    let result = run_ruby(
        r#"
        module D
          def who
            "D"
          end
        end
        module B
          include D
        end
        module C
          include D
        end
        class A
          include B
          include C
        end
        class Unrelated
        end
        puts A.new.who
        puts A.new.is_a?(D)
        puts A.new.is_a?(B)
        puts A.new.is_a?(Unrelated)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "D\ntrue\ntrue\nfalse\n");
}

#[test]
fn extend_pulls_in_module_instance_methods_as_class_methods() {
    let result = run_ruby(
        r#"
        module MathHelpers
          def double(x)
            x * 2
          end
        end
        class Calc
          extend MathHelpers
        end
        puts Calc.double(21)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn a_module_function_is_callable_via_its_own_def_self() {
    let result = run_ruby(
        r#"
        module Utility
          def self.triple(x)
            x * 3
          end
        end
        puts Utility.triple(4)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "12\n");
}

#[test]
fn class_methods_are_inherited_by_a_subclass_with_no_override() {
    let result = run_ruby(
        r#"
        class Base
          def self.bump
            1
          end
        end
        class Sub < Base
        end
        puts Sub.bump
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n");
}

#[test]
fn a_class_variable_is_shared_with_a_subclass_that_never_declares_its_own() {
    let result = run_ruby(
        r#"
        class Base
          @@count = 0
          def bump
            @@count += 1
          end
          def count
            @@count
          end
        end
        class Sub < Base
          def bump_twice
            @@count += 1
            @@count += 1
          end
        end
        b = Base.new
        s = Sub.new
        b.bump
        s.bump_twice
        puts b.count
        puts s.count
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n3\n");
}

#[test]
fn a_subclass_reassigning_a_cvar_mutates_the_shared_inherited_storage() {
    // Verified against real Ruby first: a subclass's own `@@x = ...`
    // does NOT shadow -- it finds and mutates the SAME storage inherited
    // from the superclass (real Ruby's actual, if slightly surprising,
    // class-variable semantics). This is also the exact scenario spinel's
    // own C implementation gets wrong (a subclass writing a superclass-only
    // cvar allocates fresh, separate storage there -- an outright compile
    // failure in spinel's case; see the plan's Part 6).
    let result = run_ruby(
        r#"
        class Base
          @@x = 1
          def base_x
            @@x
          end
        end
        class Sub < Base
          @@x = 99
          def sub_x
            @@x
          end
        end
        puts Base.new.base_x
        puts Sub.new.sub_x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "99\n99\n");
}

#[test]
fn an_unrelated_class_own_cvar_is_independent_storage() {
    let result = run_ruby(
        r#"
        class Base
          @@count = 0
          def count
            @@count
          end
        end
        class Other
          @@count = 100
          def count
            @@count
          end
        end
        puts Base.new.count
        puts Other.new.count
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "0\n100\n");
}

#[test]
fn raise_with_a_bare_class_defaults_the_message_to_the_class_name() {
    // No `rescue` exists yet (Phase 9) -- assert the UNCAUGHT path instead:
    // an unhandled `raise MyError` (no explicit message) exits 1 with the
    // class's own name as the message (no runtime `self.class` reflection
    // needed -- see `codegen::expr::emit_raise_value`'s docs).
    let result = run_ruby(
        r#"
        class MyError < StandardError
        end
        class Box
          def check
            raise MyError
          end
        end
        Box.new.check
        "#,
    );
    assert!(!result.status.success());
    assert!(
        result.stderr.contains("MyError"),
        "expected the default message to be the class's own name, got: {}",
        result.stderr
    );
}

#[test]
fn raise_with_an_explicit_message_and_class() {
    let result = run_ruby(
        r#"
        class MyError < StandardError
        end
        class Risky
          def check(n)
            raise MyError, "bad value: #{n}" if n < 0
            n * 2
          end
        end
        r = Risky.new
        puts r.check(5)
        puts r.check(-1)
        "#,
    );
    assert!(!result.status.success());
    assert_eq!(result.stdout, "10\n");
    assert!(
        result.stderr.contains("bad value: -1"),
        "expected the explicit message in stderr, got: {}",
        result.stderr
    );
}

#[test]
fn raise_of_a_plain_string_is_an_implicit_runtime_error() {
    let result = run_ruby(
        r#"
        class Risky
          def check
            raise "plain string error"
          end
        end
        Risky.new.check
        "#,
    );
    assert!(!result.status.success());
    assert!(
        result.stderr.contains("plain string error"),
        "expected the string message in stderr, got: {}",
        result.stderr
    );
}

#[test]
fn bare_raise_with_no_active_rescue_constructs_a_runtime_error() {
    // Bare `raise` (re-raise) outside any active `rescue` clause -- real
    // Ruby constructs a fresh `RuntimeError` with an EMPTY message rather
    // than erroring (oracle-verified: `ruby -e 'begin; raise; rescue => e;
    // puts "[#{e.message}]"; end'` -> `"[]"`) -- see
    // `codegen::expr::emit_raise`'s docs. This used to be a clean codegen
    // panic before Phase 9's `spinel_rt::current_exception` fallback shipped.
    let result = run_ruby(
        r#"
        begin
          raise
        rescue => e
          puts "[#{e.message}]"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[]\n");
}

// --- Phase 7: deeper include/extend/prepend/inherited-ivar coverage,
// oracle-verified against real `ruby` first (added after the initial batch
// above, per the user's request for more comprehensive coverage of these
// specifically).

#[test]
fn a_non_overridden_inherited_initialize_flattens_ivars_onto_the_subclass_struct() {
    let result = run_ruby(
        r#"
        class Parent
          def initialize(x)
            @x = x
          end
        end
        class Child < Parent
          def show
            @x * 10
          end
        end
        puts Child.new(4).show
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "40\n");
}

#[test]
fn an_included_module_method_mutates_an_ivar_on_the_includer() {
    // Real self/ivar capture for a materialized module method -- proves
    // the module body was re-typechecked against the INCLUDER's own
    // concrete struct, not some shared/aliased representation.
    let result = run_ruby(
        r#"
        module Counter
          def bump
            @count += 1
          end
        end
        class Widget
          include Counter
          def initialize
            @count = 0
          end
          def count
            @count
          end
        end
        w = Widget.new
        w.bump
        w.bump
        w.bump
        puts w.count
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n");
}

#[test]
fn multiple_modules_in_one_include_statement_resolve_in_given_order() {
    let result = run_ruby(
        r#"
        module A
          def a
            "a"
          end
        end
        module B
          def b
            "b"
          end
        end
        class C
          include A, B
        end
        c = C.new
        puts c.a
        puts c.b
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "a\nb\n");
}

#[test]
fn prepend_and_include_together_resolve_in_correct_precedence_order() {
    // MRO here is `Pre, Own, Inc, Object` -- `Own`'s own `label` calls
    // `super` and reaches `Inc` (the next ancestor after `Own`); `Pre`'s
    // `label` calls `super` and reaches `Own`.
    let result = run_ruby(
        r#"
        module Pre
          def label
            "pre(#{super})"
          end
        end
        module Inc
          def label
            "inc"
          end
        end
        class Own
          prepend Pre
          include Inc
          def label
            "own(#{super})"
          end
        end
        puts Own.new.label
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "pre(own(inc))\n");
}

#[test]
fn a_module_class_variable_is_shared_across_every_including_class() {
    // Verified against real Ruby first: `@@total` declared inside the
    // MODULE's own body is genuinely owned by the module itself -- every
    // class that includes it (even two entirely unrelated classes) shares
    // the SAME storage, not one copy per includer. Exercises
    // `codegen::expr::cvar_owner_id` consulting `Ctx.defining_class`
    // (the module a materialized method's body actually came from), not
    // `Ctx.current_class` (whichever class it's materialized onto).
    let result = run_ruby(
        r#"
        module Shared
          @@total = 0
          def add(n)
            @@total += n
          end
          def total
            @@total
          end
        end
        class Left
          include Shared
        end
        class Right
          include Shared
        end
        l = Left.new
        r = Right.new
        l.add(5)
        r.add(7)
        puts l.total
        puts r.total
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "12\n12\n");
}

#[test]
fn a_three_level_super_chain_crosses_a_module_boundary() {
    // `Parent#greet` calls `super`, reaching `Middle#greet` (an included
    // module), which itself calls `super`, reaching `GrandParent#greet` --
    // a chain of 3, verifying `emit_super_inline`'s ancestors-based search
    // composes correctly across more than one hop and a mixed
    // class/module boundary.
    let result = run_ruby(
        r#"
        class GrandParent
          def greet
            "grandparent"
          end
        end
        module Middle
          def greet
            "middle(#{super})"
          end
        end
        class Parent < GrandParent
          include Middle
          def greet
            "parent(#{super})"
          end
        end
        puts Parent.new.greet
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "parent(middle(grandparent))\n");
}

#[test]
fn extend_and_include_can_be_combined_on_the_same_class() {
    let result = run_ruby(
        r#"
        module Ext
          def helper(x)
            x + 1
          end
        end
        module Inc
          def instance_helper
            "inc"
          end
        end
        class Both
          extend Ext
          include Inc
        end
        puts Both.helper(9)
        puts Both.new.instance_helper
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\ninc\n");
}

#[test]
fn ivars_from_a_three_level_plain_inheritance_chain_are_all_present_on_the_leaf() {
    // A REAL bug found while adding this test: ivar-flattening originally
    // only scanned the MRO-winning materialized methods, missing any ivar
    // only ever touched through a `super`-reachable (but shadowed, not
    // winning) ancestor body -- `B#initialize`/`C#initialize` both call
    // `super` and their OWN bodies don't textually mention `@a`, only
    // `A#initialize`'s spliced-in body does. Fixed by collecting ivars from
    // EVERY ancestor's own methods, not just the ones that end up
    // materialized as the winning definition.
    let result = run_ruby(
        r#"
        class A
          def initialize
            @a = 1
          end
        end
        class B < A
          def initialize
            super
            @b = 2
          end
        end
        class C < B
          def initialize
            super
            @c = 3
          end
          def total
            @a + @b + @c
          end
        end
        puts C.new.total
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n");
}

// Phase 8: `case/in` pattern matching. Every test below was oracle-verified
// against real `ruby` first, per this project's established convention.

#[test]
fn case_in_class_check_pattern_binds_and_statically_narrows_to_int() {
    // `Integer => n` both binds `n` AND statically narrows its type for the
    // rest of the arm's body -- `n + 1` should take the native `Int`
    // arithmetic fast path, not a runtime Poly fallback (verified indirectly:
    // if narrowing were broken this would still print `6`, but a fast-path
    // regression would show up as a `spinelc` panic on `+` instead, since a
    // Poly local has no runtime `+` fallback for a non-builtin-typed operand
    // -- see `codegen::call::dispatch`'s docs).
    let result = run_ruby("case 5\nin Integer => n\n  puts n + 1\nend\n");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n");
}

#[test]
fn case_in_array_pattern_binds_pre_and_rest() {
    let result = run_ruby(
        r#"
        case [1, 2, 3]
        in [Integer => a, *rest]
          puts a
          puts rest.length
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n2\n");
}

#[test]
fn case_in_array_pattern_pre_and_post_splat() {
    let result = run_ruby(
        r#"
        case [1, 2, 3, 4, 5]
        in [*pre, 3, *post]
          puts pre.length
          puts post.length
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n2\n");
}

#[test]
fn case_in_pin_pattern_matches_only_the_pinned_value() {
    let result = run_ruby(
        r#"
        x = 5
        case 10
        in ^x
          puts "same as x"
        else
          puts "different"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "different\n");
}

#[test]
fn case_in_alternation_pattern_matches_any_member() {
    let result = run_ruby(
        r#"
        case 3
        in 1 | 2 | 3
          puts "small"
        else
          puts "big"
        end
        case 99
        in 1 | 2 | 3
          puts "small"
        else
          puts "big"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "small\nbig\n");
}

#[test]
fn case_in_guard_and_class_precedence() {
    let result = run_ruby(
        r#"
        class Classifier
          def classify(v)
            case v
            in Integer => n if n % 2 == 0
              "even int #{n}"
            in Integer
              "odd int"
            in String
              "string"
            else
              "other"
            end
          end
        end
        c = Classifier.new
        puts c.classify(4)
        puts c.classify(3)
        puts c.classify("hi")
        puts c.classify(nil)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "even int 4\nodd int\nstring\nother\n");
}

#[test]
fn case_in_unless_guard() {
    let result = run_ruby(
        r#"
        case [1, 2]
        in [a, *b] unless a == 0
          puts a
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n");
}

#[test]
fn case_in_hash_pattern_binds_value_and_rest() {
    let result = run_ruby(
        r#"
        h = { a: 1, b: 2, c: 3 }
        case h
        in { a: Integer => av, **rest }
          puts av
          puts rest.length
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n2\n");
}

#[test]
fn case_in_hash_pattern_shorthand_binds_local_named_after_key() {
    let result = run_ruby(
        r#"
        h = { name: 1 }
        case h
        in { name: }
          puts name
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n");
}

#[test]
fn case_in_hash_pattern_no_more_keys_rejects_extra_keys() {
    let result = run_ruby(
        r#"
        case { a: 1 }
        in { a: 1, **nil }
          puts "exact match"
        end
        case { a: 1, b: 2 }
        in { a: 1, **nil }
          puts "should not print"
        else
          puts "extra keys rejected"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "exact match\nextra keys rejected\n");
}

#[test]
fn case_in_find_pattern_locates_a_matching_window() {
    let result = run_ruby(
        r##"
        case [1, 2, 3, 4, 5]
        in [*, Integer => a, Integer => b, *]
          puts "#{a} #{b}"
        end
        case [10, 20, 30]
        in [*, 99, *]
          puts "found 99"
        else
          puts "not found"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1 2\nnot found\n");
}

#[test]
fn case_in_nested_array_pattern_narrows_each_element() {
    let result = run_ruby(
        r##"
        case [[1, "a"], [2, "b"]]
        in [[Integer => n1, String => s1], [Integer => n2, String => s2]]
          puts "#{n1}#{s1} #{n2}#{s2}"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1a 2b\n");
}

#[test]
fn case_in_range_pattern() {
    let result = run_ruby(
        r#"
        case 5
        in 1..10
          puts "in range"
        end
        case 15
        in 1..10
          puts "in range"
        else
          puts "out of range"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "in range\nout of range\n");
}

#[test]
fn case_in_nil_true_false_literal_patterns() {
    let result = run_ruby(
        r#"
        case nil
        in nil
          puts "was nil"
        end
        case true
        in true
          puts "was true"
        end
        case false
        in false
          puts "was false"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "was nil\nwas true\nwas false\n");
}

#[test]
fn case_in_bare_bind_pattern_matches_anything() {
    let result = run_ruby("case 42\nin x\n  puts \"bound: #{x}\"\nend\n");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "bound: 42\n");
}

#[test]
fn case_in_object_deconstruct_dispatches_to_array_pattern() {
    let result = run_ruby(
        r#"
        class Point
          def initialize(x, y)
            @x = x
            @y = y
          end
          def deconstruct
            [@x, @y]
          end
        end
        p1 = Point.new(1, 2)
        case p1
        in [px, py]
          puts "array: #{px}, #{py}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "array: 1, 2\n");
}

#[test]
fn case_in_object_deconstruct_keys_with_constant_guard() {
    let result = run_ruby(
        r#"
        class Point
          def initialize(x, y)
            @x = x
            @y = y
          end
          def deconstruct_keys(keys)
            { x: @x, y: @y }
          end
        end
        p1 = Point.new(1, 2)
        case p1
        in Point(x:, y:)
          puts "constant: #{x}, #{y}"
        end
        case p1
        in { x:, y: }
          puts "plain: #{x}, #{y}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "constant: 1, 2\nplain: 1, 2\n");
}

#[test]
fn case_in_object_with_no_deconstruct_falls_through_to_else() {
    // A statically-provable-never-matches array pattern (this class simply
    // has no `#deconstruct`) is resolved entirely at CODEGEN time (a
    // compile-time-constant `false`, no runtime call attempted at all) --
    // see `codegen::patterns::emit_array_binding`'s docs.
    let result = run_ruby(
        r#"
        class Plain
        end
        o = Plain.new
        case o
        in [a, b]
          puts "matched"
        else
          puts "no deconstruct, fell to else"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "no deconstruct, fell to else\n");
}

#[test]
fn match_predicate_one_liner_is_a_boolean_that_never_raises() {
    let result = run_ruby(
        r#"
        if [1, 2] in [Integer, Integer]
          puts "matched"
        end
        if [1, "x"] in [Integer, Integer]
          puts "should not print"
        else
          puts "did not match"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "matched\ndid not match\n");
}

#[test]
fn match_predicate_bindings_escape_to_the_enclosing_scope() {
    let result = run_ruby(
        r#"
        arr = [1, "hi"]
        arr in [Integer => a, String => b]
        puts a
        puts b
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\nhi\n");
}

#[test]
fn match_required_one_liner_binds_on_success() {
    let result = run_ruby(
        r#"
        arr = [1, 2]
        arr => [first, second]
        puts first
        puts second
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n2\n");
}

#[test]
fn match_required_one_liner_raises_no_matching_pattern_error_on_failure() {
    let result = run_ruby("5 => String\n");
    assert!(!result.status.success());
    assert!(
        result.stderr.contains("uncaught exception"),
        "stderr: {}",
        result.stderr
    );
}

#[test]
fn case_in_with_no_matching_arm_and_no_else_raises() {
    let result = run_ruby("case 5\nin String\n  puts \"no\"\nend\n");
    assert!(!result.status.success());
    assert!(
        result.stderr.contains("uncaught exception: no matching pattern"),
        "stderr: {}",
        result.stderr
    );
}

#[test]
fn case_in_capture_of_a_bare_bind_can_write_the_same_subject_to_two_names() {
    // A real latent hazard this test guards against: `Capture(Bind(x), y)`
    // writes the SAME scrutinee into two different locals -- if either write
    // MOVED instead of CLONED the scrutinee, the second write would fail to
    // compile ("use of moved value"). See `emit_pattern_match`'s docs.
    let result = run_ruby("case 5\nin x => y\n  puts x\n  puts y\nend\n");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\n5\n");
}

// --- Phase 9: exceptions (begin/rescue/else/ensure/retry, raise, custom
// hierarchies), oracle-verified against real `ruby` first. `e`'s method
// calls (`e.message`) always go through `.send(:message)` -- a rescue
// binding is deliberately never narrowed to a concrete class (unlike a
// pattern's `Integer => n`): unlike a builtin primitive's runtime tag check,
// dispatch there is Rust `downcast::<T>()`-based, which only succeeds
// against the EXACT concrete struct, and `rescue StandardError => e` must
// also match any raised SUBCLASS instance -- see
// `codegen::exceptions::emit_rescue_chain`'s docs.

#[test]
fn begin_rescue_catches_and_ensure_always_runs() {
    let result = run_ruby(
        r#"
        begin
          puts "try"
          raise ArgumentError, "bad"
        rescue ArgumentError => e
          puts "caught: #{e.send(:message)}"
        ensure
          puts "ensure ran"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "try\ncaught: bad\nensure ran\n");
}

#[test]
fn else_clause_runs_only_on_the_no_exception_path() {
    let result = run_ruby(
        r#"
        begin
          puts "body"
        rescue
          puts "rescued"
        else
          puts "else ran"
        ensure
          puts "ensure ran"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "body\nelse ran\nensure ran\n");
}

#[test]
fn rescuing_a_middle_class_catches_a_raised_leaf_subclass() {
    let result = run_ruby(
        r#"
        class AppError < StandardError
        end
        class ValidationError < AppError
        end

        begin
          raise ValidationError, "bad input"
        rescue AppError => e
          puts "caught as AppError: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught as AppError: bad input\n");
}

#[test]
fn rescue_with_multiple_classes_in_one_clause() {
    let result = run_ruby(
        r#"
        begin
          raise TypeError, "wrong type"
        rescue ArgumentError, TypeError => e
          puts "caught one of: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught one of: wrong type\n");
}

#[test]
fn built_in_hierarchy_extension_classes_are_real_and_ancestor_matched() {
    // `KeyError < IndexError` -- exercises the extended built-in hierarchy
    // (Part 8's Phase 9 addition beyond Part 6's minimal foundation).
    let result = run_ruby(
        r#"
        begin
          raise KeyError, "missing"
        rescue IndexError => e
          puts "caught KeyError via IndexError: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught KeyError via IndexError: missing\n");
}

#[test]
fn bare_raise_re_raises_preserving_a_custom_exceptions_own_ivars() {
    let result = run_ruby(
        r#"
        class MyError < StandardError
          def initialize(msg, code)
            super(msg)
            @code = code
          end
          def code
            @code
          end
        end

        class Inner
          def call
            begin
              raise MyError.new("failed with code 42", 42)
            rescue MyError => e
              puts "inner saw code #{e.send(:code)}"
              raise
            end
          end
        end

        begin
          Inner.new.call
        rescue MyError => e
          puts "outer saw code #{e.send(:code)}"
          puts "outer message: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "inner saw code 42\nouter saw code 42\nouter message: failed with code 42\n"
    );
}

#[test]
fn return_from_a_rescue_body_still_runs_ensure_exactly_once() {
    let result = run_ruby(
        r#"
        class ReturnTester
          def m
            begin
              raise "x"
            rescue
              return "returned"
            ensure
              puts "ensure ran before return"
            end
          end
        end
        puts ReturnTester.new.m
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "ensure ran before return\nreturned\n");
}

#[test]
fn retry_loop_runs_ensure_exactly_once_not_per_attempt() {
    let result = run_ruby(
        r#"
        class Attempt
          def initialize
            @attempts = 0
          end
          def run
            begin
              @attempts += 1
              raise "fail" if @attempts < 3
              puts "succeeded after #{@attempts} attempts"
            rescue
              retry if @attempts < 3
            ensure
              puts "ensure ran, attempts=#{@attempts}"
            end
          end
        end
        Attempt.new.run
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "succeeded after 3 attempts\nensure ran, attempts=3\n");
}

#[test]
fn nested_retry_only_restarts_the_innermost_begin() {
    let result = run_ruby(
        r#"
        outer_runs = 0
        inner_attempts = 0
        begin
          outer_runs += 1
          begin
            inner_attempts += 1
            raise "x" if inner_attempts < 2
            puts "inner ok after #{inner_attempts}"
          rescue
            retry if inner_attempts < 2
          end
          puts "outer ran #{outer_runs} times"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "inner ok after 2\nouter ran 1 times\n");
}

#[test]
fn method_level_implicit_rescue_with_no_explicit_begin_end() {
    // `def m; ...; rescue => e; ...; end` -- `DefNode::body()` is directly a
    // `BeginNode` with no `begin_keyword_loc` in this shape (confirmed via
    // `Prism.parse`), flowing through the SAME `HirNode::Begin` lowering as
    // an explicit `begin`.
    let result = run_ruby(
        r#"
        class Worker
          def safe
            raise "oops"
          rescue => e
            "handled: #{e.send(:message)}"
          end
        end
        puts Worker.new.safe
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "handled: oops\n");
}

#[test]
fn endless_method_rescue_modifier() {
    let result = run_ruby(
        r#"
        class Unsafe
          def op(n)
            raise "negative" if n < 0
            n * 2
          end
        end
        class Calc
          def safe_op(n) = Unsafe.new.op(n) rescue -1
        end
        c = Calc.new
        puts c.safe_op(5)
        puts c.safe_op(-5)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n-1\n");
}

#[test]
fn assignment_rescue_modifier() {
    let result = run_ruby(
        r#"
        class TopRisk
          def risky_top
            raise "bad"
          end
        end
        x = TopRisk.new.risky_top rescue "fallback"
        puts x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "fallback\n");
}

#[test]
fn unmatched_rescue_class_propagates_to_an_outer_rescue() {
    let result = run_ruby(
        r#"
        begin
          begin
            raise TypeError, "inner"
          rescue ArgumentError
            puts "wrong handler"
          end
        rescue TypeError => e
          puts "outer caught: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "outer caught: inner\n");
}

#[test]
fn uncaught_raise_with_no_rescue_anywhere_exits_with_the_message() {
    let result = run_ruby("raise \"boom\"\n");
    assert!(!result.status.success());
    assert!(
        result.stderr.contains("uncaught exception: boom"),
        "stderr: {}",
        result.stderr
    );
}

#[test]
fn break_next_from_inside_begin_rescue_nested_in_a_real_escaping_block_works() {
    // `Array#each`/general Enumerable iteration isn't implemented (Phase 3's
    // documented scope-cut), so a custom `yield`-based method is the
    // supported way to attach a real escaping block here. The begin/rescue
    // closure boundary (see `codegen::exceptions`'s module docs) needs no
    // special handling: the block passed to `each_num` is ALREADY a real
    // escaping `Proc` (its own closure boundary, `in_real_proc` already
    // true), so a `next` inside the nested `begin`'s rescue clause raises
    // the exact same `Signal` it already would have -- caught by the Proc's
    // own wrapper loop, not by the (irrelevant here) native-loop rejection
    // check.
    let result = run_ruby(
        r#"
        class Each3
          def each_num
            yield 1
            yield 2
            yield 3
          end
        end

        Each3.new.each_num do |i|
          begin
            raise "boom" if i == 2
            puts "ok #{i}"
          rescue
            next
          ensure
            puts "ensure #{i}"
          end
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "ok 1\nensure 1\nensure 2\nok 3\nensure 3\n");
}

#[test]
#[should_panic(expected = "targeting a loop OUTSIDE it")]
fn break_inside_begin_rescue_targeting_an_outer_native_loop_is_a_clean_compile_error() {
    // See `codegen::exceptions`'s module docs for why this specific shape
    // (a native `while`/`for`/`.times` loop OUTSIDE the `begin`) can't be
    // supported without abandoning loops' own zero-cost literal-label
    // design -- a deliberate, documented scope-cut, not an oversight. A loop
    // written INSIDE the `begin` itself (the previous test) is unaffected.
    let _ = spinelc::compile_to_rust(
        r#"
        i = 0
        while i < 3
          begin
            break if i == 1
          rescue
          end
          i += 1
        end
        "#,
    );
}

// --- Phase 9 continued: deeper edge-case and composition coverage, added
// after the initial batch above per an explicit request for more
// comprehensive tests. Every scenario oracle-verified against real `ruby`
// first, per this project's established convention.

#[test]
fn bare_rescue_does_not_catch_a_script_error_level_exception() {
    // A bare `rescue` matches `StandardError` and below ONLY -- it must NOT
    // catch a raised `ScriptError` (a sibling branch of the hierarchy, both
    // direct children of `Exception`). This is the real semantic bare
    // `rescue`'s default narrows to `StandardError`, not `Exception`.
    let result = run_ruby(
        r#"
        begin
          begin
            raise ScriptError, "script problem"
          rescue => e
            puts "should not print: #{e.send(:message)}"
          end
        rescue ScriptError => e
          puts "outer caught ScriptError: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "outer caught ScriptError: script problem\n");
}

#[test]
fn rescue_exception_class_explicitly_catches_a_script_error() {
    let result = run_ruby(
        r#"
        begin
          raise ScriptError, "script problem"
        rescue Exception => e
          puts "caught via Exception: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught via Exception: script problem\n");
}

#[test]
fn return_from_ensure_overrides_return_from_the_begin_body() {
    // Real, tricky Ruby semantic: a `return` inside `ensure` wins over a
    // `return` already in flight from `begin`'s own body -- not just "ensure
    // runs afterward", it actually REPLACES the method's return value.
    // Falls out for free here: `ensure`'s own statements are ordinary Rust
    // code (not wrapped in the begin/rescue closure boundary -- see
    // `codegen::exceptions`'s module docs), so a literal `return` inside it
    // exits the enclosing method directly, superseding whatever `__final`
    // already held.
    let result = run_ruby(
        r#"
        class M
          def m
            begin
              return "from begin"
            ensure
              return "from ensure"
            end
          end
        end
        puts M.new.m
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "from ensure\n");
}

#[test]
fn raise_inside_ensure_replaces_the_original_exception() {
    let result = run_ruby(
        r#"
        begin
          begin
            raise "original"
          ensure
            raise "from ensure"
          end
        rescue => e
          puts "caught: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught: from ensure\n");
}

#[test]
fn nested_begin_ensure_runs_inner_before_outer() {
    let result = run_ruby(
        r#"
        begin
          begin
            puts "inner body"
          ensure
            puts "inner ensure"
          end
        ensure
          puts "outer ensure"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "inner body\ninner ensure\nouter ensure\n");
}

#[test]
fn re_raise_preserves_the_exact_same_object_not_a_copy() {
    // Proves re-raise doesn't construct a fresh exception -- mutating an
    // ivar (via an `attr_accessor`-generated setter) INSIDE the inner
    // `rescue`, then bare `raise`-ing, must be visible to the OUTER
    // `rescue` reading the same ivar back. Constructed and raised in ONE
    // expression (`raise Tagged.new(...)`), not first assigned to a named
    // local -- a named local assigned an `Object`-typed value inside a
    // `begin`'s body (or any branching construct) hits a real, pre-existing,
    // Phase-9-independent codegen gap when that local's type has to widen
    // to `Poly` across branches (confirmed to affect plain `if`/`else` too,
    // not something this phase introduced or is responsible for fixing).
    let result = run_ruby(
        r#"
        class Tagged < StandardError
          def initialize(msg, tag)
            super(msg)
            @tag = tag
          end
          attr_accessor :tag
        end
        begin
          begin
            raise Tagged.new("x", "initial")
          rescue Tagged => e
            e.send(:tag=, "mutated")
            raise
          end
        rescue Tagged => e
          puts "tag: #{e.send(:tag)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "tag: mutated\n");
}

#[test]
fn begin_rescue_used_as_an_expressions_value() {
    let result = run_ruby(
        r#"
        x = begin
          raise "bad"
        rescue
          -1
        end
        puts x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "-1\n");
}

#[test]
fn begin_ensure_with_no_rescue_clause_still_runs_ensure_and_propagates() {
    let result = run_ruby(
        r#"
        begin
          begin
            raise "boom"
          ensure
            puts "ensure ran"
          end
        rescue => e
          puts "caught outside: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "ensure ran\ncaught outside: boom\n");
}

#[test]
fn ensure_runs_when_no_exception_is_raised_and_there_is_no_rescue_at_all() {
    let result = run_ruby(
        r#"
        begin
          puts "no exception"
        ensure
          puts "ensure always"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "no exception\nensure always\n");
}

#[test]
fn three_rescue_clauses_skips_non_matching_ones_in_order() {
    let result = run_ruby(
        r#"
        begin
          raise RangeError, "oops"
        rescue ArgumentError
          puts "arg"
        rescue TypeError
          puts "type"
        rescue RangeError => e
          puts "range: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "range: oops\n");
}

#[test]
fn plain_string_raise_is_caught_by_an_explicit_runtime_error_rescue() {
    let result = run_ruby(
        r#"
        begin
          raise "just a string"
        rescue RuntimeError => e
          puts "caught runtime: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught runtime: just a string\n");
}

#[test]
fn case_in_pattern_matching_inside_a_rescue_body() {
    // Composes Phase 8 (pattern matching) with Phase 9 (exceptions) --
    // `e.send(:message)` is a `Poly` expression, matched via `case/in`'s own
    // `#deconstruct`-independent `ClassCheck`/`Capture` path.
    let result = run_ruby(
        r#"
        begin
          raise "boom"
        rescue => e
          case e.send(:message)
          in String => s
            puts "matched string: #{s}"
          end
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "matched string: boom\n");
}

#[test]
fn times_loop_containing_begin_rescue_with_no_crossing_control_flow_still_works() {
    // A native loop CONTAINING a `begin`/`rescue` (as opposed to a `begin`
    // containing a bare `break`/`next` TARGETING an outer loop, the
    // rejected shape) is completely unaffected -- the `.times` loop's own
    // control flow doesn't cross the begin/rescue closure boundary at all
    // here, so it just runs normally, once per iteration.
    let result = run_ruby(
        r#"
        3.times do |i|
          begin
            raise "x" if i == 1
            puts "ok #{i}"
          rescue
            puts "rescued #{i}"
          end
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "ok 0\nrescued 1\nok 2\n");
}

#[test]
fn sequential_top_level_begin_blocks_do_not_leak_handling_state() {
    // Guards `spinel_rt::handling`'s push/pop discipline: three INDEPENDENT
    // `begin` blocks in sequence, the last a bare re-raise with nothing
    // currently being handled -- if an earlier block's `pop_handling` were
    // ever skipped (e.g. on an unusual exit path), this would incorrectly
    // re-raise a STALE exception instead of falling back to a fresh
    // `RuntimeError`.
    let result = run_ruby(
        r#"
        begin
          raise "first"
        rescue => e
          puts "1: #{e.send(:message)}"
        end
        begin
          raise "second"
        rescue => e
          puts "2: #{e.send(:message)}"
        end
        begin
          raise
        rescue => e
          puts "3: [#{e.send(:message)}]"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1: first\n2: second\n3: []\n");
}

#[test]
fn class_method_begin_rescue_ensure_with_return() {
    // Exercises `codegen::mod::emit_class_method_fn`'s own `Signal::Return`
    // catch (added specifically for `Begin` nodes inside a class method --
    // class methods can't contain an escaping block at all, so `Begin` was
    // the only possible trigger there).
    let result = run_ruby(
        r#"
        class Factory
          def self.build(fail_it)
            begin
              raise "nope" if fail_it
              "built"
            rescue
              return "fallback"
            ensure
              puts "factory ensure"
            end
          end
        end
        puts Factory.build(false)
        puts Factory.build(true)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "factory ensure\nbuilt\nfactory ensure\nfallback\n");
}

#[test]
fn defined_on_a_begin_expression_classifies_as_expression() {
    let result = run_ruby("puts defined?(begin; 1; end)\n");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "expression\n");
}

#[test]
fn retry_and_ensure_combined_inside_an_enclosing_while_loop() {
    // The `begin`'s own `retry` restarts just its OWN body (not the `while`
    // loop), and `ensure` runs once per `while` ITERATION (twice total,
    // once per `i`) -- not once per `retry` attempt within an iteration.
    let result = run_ruby(
        r#"
        total_ensure = 0
        i = 0
        while i < 2
          attempts = 0
          begin
            attempts += 1
            raise "x" if attempts < 2
          rescue
            retry if attempts < 2
          ensure
            total_ensure += 1
          end
          i += 1
        end
        puts total_ensure
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n");
}

#[test]
fn unrescued_raise_propagates_through_multiple_method_call_frames() {
    // Three separate objects (not sibling methods on one class) --
    // implicit-self calls to a sibling method aren't supported yet
    // (a separate, pre-existing, documented gap from Phase 5/6), so each
    // level calls the next via an explicit receiver instead. Still a
    // genuine 3-frame unwind, crossing object boundaries too.
    let result = run_ruby(
        r#"
        class Level3
          def run
            raise "deep failure"
          end
        end
        class Level2
          def run
            Level3.new.run
          end
        end
        class Level1
          def run
            Level2.new.run
          end
        end
        begin
          Level1.new.run
        rescue => e
          puts "caught from deep: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught from deep: deep failure\n");
}

// Correctness-fix regression tests below -- each reproduces one bug a
// codebase-wide gap audit found (see docs/PORTING_ANALYSIS.md).

#[test]
fn super_with_explicit_args_binds_the_parents_own_param_names() {
    // Before this fix, `emit_super_inline` spliced the parent's body without
    // ever binding its parameter names -- this only "worked" when parent/
    // child happened to share names. Here they deliberately DON'T (`name`
    // vs. `name, breed`), so this only passes once `super(name)` actually
    // binds Animal's own `name` parameter fresh.
    let result = run_ruby(
        r#"
        class Animal
          def initialize(name)
            @name = name
          end
          def name
            @name
          end
        end

        class Dog < Animal
          def initialize(name, breed)
            super(name)
            @breed = breed
          end
          def breed
            @breed
          end
        end

        d = Dog.new("Rex", "Lab")
        puts d.name
        puts d.breed
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Rex\nLab\n");
}

#[test]
fn bare_super_forwards_the_current_methods_own_params() {
    // Bare `super` (no parens) forwards the CURRENT method's own already-
    // bound parameter values positionally -- also unbound before this fix
    // (same root cause as the explicit-args case above).
    let result = run_ruby(
        r#"
        class Animal
          def initialize(name)
            @name = name
          end
          def name
            @name
          end
        end

        class Dog < Animal
          def initialize(name)
            super
            @greeting = "woof from #{name}"
          end
          def greeting
            @greeting
          end
        end

        d = Dog.new("Rex")
        puts d.name
        puts d.greeting
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Rex\nwoof from Rex\n");
}

#[test]
fn object_typed_local_widened_to_poly_across_branches_boxes_correctly() {
    // `x`'s two branches assign DIFFERENT classes, so
    // `analyze::locals::merge_locals` widens its whole-scope type to `Poly`
    // (a plain `RubyValue` slot) -- but each branch's own `HirNode::New`
    // codegen still produces a bare, unboxed `Arc<Concrete>` unless the
    // write site itself boxes it. Before this fix, this was
    // a genuine `rustc` type-mismatch in the GENERATED Rust, not just a
    // wrong answer.
    let result = run_ruby(
        r#"
        class Foo
          def initialize
            @tag = "foo"
          end
          def tag
            @tag
          end
        end

        class Bar
          def initialize
            @tag = "bar"
          end
          def tag
            @tag
          end
        end

        class Picker
          def pick(flag)
            if flag
              x = Foo.new
            else
              x = Bar.new
            end
            x
          end
        end

        p = Picker.new
        puts p.pick(true).tag
        puts p.pick(false).tag
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "foo\nbar\n");
}

#[test]
fn array_index_assign_out_of_range_raises_index_error_not_a_panic() {
    // Before this fix, a negative out-of-range `Array#[]=` index was an
    // unconditional Rust `panic!` -- a real `IndexError` (catchable) is
    // constructed instead now, and the array is otherwise
    // unaffected (an in-range negative-from-end or growing-positive index
    // still works exactly as before).
    let result = run_ruby(
        r#"
        a = [1, 2, 3]
        begin
          a[-10] = :x
        rescue IndexError => e
          puts "caught"
        end
        a[5] = :y
        puts a.length
        puts a[5]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught\n6\ny\n");
}

#[test]
#[should_panic(expected = "dynamic dispatch of `send` with keyword arguments isn't supported yet")]
fn send_with_a_non_literal_target_and_keyword_args_is_a_clean_error() {
    // Before this fix, `send`'s truly-dynamic fallback (reached because the
    // target name isn't a literal symbol here) silently DROPPED `kwargs`
    // and dispatched without them -- now it raises the same clear codegen
    // error a directly-called method with keyword params already gives via
    // `emit_dynamic_trampoline`, instead of silently
    // diverging from that behavior.
    let _ = spinelc::compile_to_rust(
        r#"
        class Foo
          def bar(x:)
            x
          end
        end
        name = :bar
        Foo.new.send(name, x: 1)
        "#,
    );
}

#[test]
#[should_panic(expected = "dynamic dispatch of `foo` with keyword arguments isn't supported yet")]
fn ordinary_call_on_a_poly_receiver_with_keyword_args_is_a_clean_error() {
    // Same bug, the OTHER silent-drop site: an ordinary (non-`send`) call on
    // a `Poly`-typed receiver (a rescued exception binding is never narrowed
    // to a concrete class -- see `codegen::exceptions`'s docs) with keyword
    // arguments used to dispatch silently without them.
    let _ = spinelc::compile_to_rust(
        r#"
        begin
          raise "boom"
        rescue => e
          e.foo(bar: 1)
        end
        "#,
    );
}

#[test]
fn optional_param_default_expr_referencing_an_ivar_is_scanned() {
    // Before this fix, `analyze::collect_ivars`/`locals::track_extra`/
    // `hoisting`'s per-method scans never walked into a `Params::optional`/
    // `keywords` default expression -- an ivar referenced ONLY there (never
    // in the method's own body) would be missing from the generated
    // struct's fields entirely, a "no such field" codegen error. Also
    // exercises the keyword-optional case (`y:`) in the same call.
    let result = run_ruby(
        r#"
        class Greeter
          def initialize
            @default_name = "World"
          end
          def greet(x = @default_name, y: @default_name)
            "hi #{x} and #{y}"
          end
        end

        puts Greeter.new.greet
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi World and World\n");
}

#[test]
fn implicit_self_call_dispatches_to_a_sibling_method() {
    let result = run_ruby(
        r#"
        class Greeter
          def greet
            hello
          end

          def hello
            "hi from hello"
          end
        end

        puts Greeter.new.greet
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi from hello\n");
}

#[test]
fn implicit_self_call_with_args_can_mutate_an_ivar() {
    let result = run_ruby(
        r#"
        class Counter
          def initialize
            @count = 0
          end

          def bump(n)
            add(n)
            @count
          end

          def add(n)
            @count = @count + n
          end
        end

        c = Counter.new
        puts c.bump(5)
        puts c.bump(2)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\n7\n");
}

#[test]
fn explicit_self_dot_method_and_bare_self_as_a_value() {
    let result = run_ruby(
        r#"
        class Box
          def initialize(v)
            @v = v
          end

          def value
            @v
          end

          def describe
            self.value
          end

          def identity
            self
          end
        end

        b = Box.new(42)
        puts b.describe
        puts b.identity.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n42\n");
}

#[test]
fn respond_to_true_and_false_on_a_statically_known_receiver() {
    let result = run_ruby(
        r#"
        class Dog
          def bark
            "woof"
          end
        end

        d = Dog.new
        puts d.respond_to?(:bark)
        puts d.respond_to?(:meow)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\n");
}

#[test]
fn respond_to_on_a_poly_typed_rescue_binding() {
    // `e` (a rescue clause's exception binding) is never narrowed to a
    // concrete class (see `codegen::exceptions`'s docs) -- exercises
    // `respond_to?`'s runtime `class_id()` fallback path, not the
    // statically-known-class one the test above exercises.
    let result = run_ruby(
        r#"
        begin
          raise "boom"
        rescue => e
          puts e.respond_to?(:message)
          puts e.respond_to?(:not_a_thing)
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\n");
}

#[test]
fn an_escaping_block_capturing_self_via_explicit_self_dot_method() {
    // Before `HirNode::SelfRef` existed, an escaping block could only ever
    // capture `self` implicitly via a bare `@ivar` reference --
    // `self.method_name` is another way a block needs the same capture (see
    // `codegen::captures`'s new `SelfRef` arm). Also exercises implicit-self
    // dispatch (`each_num(a, b)`, no receiver) from inside the SAME method
    // that constructs the escaping block.
    let result = run_ruby(
        r#"
        class Collector
          def initialize(tag)
            @tag = tag
          end

          def tag
            @tag
          end

          def each_num(a, b)
            yield a
            yield b
          end

          def run(a, b)
            each_num(a, b) { |n| puts "tag=#{self.tag}:#{n}" }
          end
        end

        Collector.new("x").run(1, 2)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "tag=x:1\ntag=x:2\n");
}

#[test]
#[should_panic(expected = "`self` isn't supported inside a class method")]
fn self_inside_a_class_method_is_a_clean_compile_error() {
    // No first-class `Class`/`Module` runtime value exists (a documented
    // scope-cut -- see the plan's Part 6), so `self` inside `def self.x`
    // has nothing to represent it. Must be a clean codegen-time panic, not
    // a `rustc` failure on the generated program referencing a Rust `self`
    // binding that doesn't exist in a class method's signature.
    let _ = spinelc::compile_to_rust(
        r#"
        class Foo
          def self.bar
            self
          end
        end
        Foo.bar
        "#,
    );
}

#[test]
#[should_panic(expected = "private method `helper` called with an explicit receiver")]
fn private_method_called_with_an_explicit_receiver_is_a_clean_compile_error() {
    let _ = spinelc::compile_to_rust(
        r#"
        class Box
          def initialize
            @x = 1
          end

          private

          def helper
            2
          end
        end

        b = Box.new
        puts b.helper
        "#,
    );
}

#[test]
fn private_method_callable_via_implicit_self_and_explicit_self_dot() {
    // Real Ruby (2.7+): a private method IS callable with an explicit
    // receiver as long as it's a literal `self` -- not just implicit-self
    // (no receiver at all).
    let result = run_ruby(
        r#"
        class Box
          def run
            self.helper + implicit_helper
          end

          private

          def helper
            10
          end

          def implicit_helper
            helper
          end
        end

        puts Box.new.run
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "20\n");
}

#[test]
fn private_def_idiom_and_retroactive_private_by_name() {
    let result = run_ruby(
        r#"
        class A
          def pub_a
            priv_a
          end

          private def priv_a
            "priv_a"
          end
        end

        class B
          def pub_b
            priv_b
          end

          def priv_b
            "priv_b"
          end

          private :priv_b
        end

        puts A.new.pub_a
        puts B.new.pub_b
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "priv_a\npriv_b\n");
}

#[test]
fn protected_method_callable_from_a_related_classs_own_method() {
    let result = run_ruby(
        r#"
        class Money
          def initialize(amount)
            @amount = amount
          end

          def greater_than_five
            other = Money.new(5)
            amount > other.amount
          end

          protected

          def amount
            @amount
          end
        end

        puts Money.new(10).greater_than_five
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\n");
}

#[test]
#[should_panic(expected = "protected method `amount` called from outside a related class")]
fn protected_method_called_from_outside_any_related_class_is_a_clean_compile_error() {
    let _ = spinelc::compile_to_rust(
        r#"
        class Money
          def initialize(amount)
            @amount = amount
          end

          protected

          def amount
            @amount
          end
        end

        a = Money.new(10)
        puts a.amount
        "#,
    );
}

#[test]
fn send_bypasses_visibility_entirely() {
    let result = run_ruby(
        r#"
        class Box
          private

          def secret
            "shh"
          end
        end

        puts Box.new.send(:secret)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "shh\n");
}

#[test]
#[should_panic(expected = "`public_send` cannot call non-public method `secret`")]
fn public_send_still_enforces_visibility_unlike_send() {
    let _ = spinelc::compile_to_rust(
        r#"
        class Box
          private

          def secret
            "shh"
          end
        end

        puts Box.new.public_send(:secret)
        "#,
    );
}

#[test]
fn float_literals_and_arithmetic() {
    let result = run_ruby(
        r#"
        puts 1.5 + 2.5
        puts 3.0 - 1
        puts 2.0 * 3
        puts 7.0 / 2
        puts 7.5 % 2
        puts 2.0 ** 3
        puts 1.0 == 1
        puts 1.5 < 2.5
        puts(1.5 <=> 2.5)
        puts(-1.5)
        puts 1.0
        puts 100.0
        puts 3.14
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "4.0\n2.0\n6.0\n3.5\n1.5\n8.0\ntrue\ntrue\n-1\n-1.5\n1.0\n100.0\n3.14\n"
    );
}

#[test]
fn float_and_int_mixed_arithmetic_promotes_to_float() {
    let result = run_ruby(
        r#"
        class Adder
          def add(a, b)
            a + b
          end
        end
        puts Adder.new.add(1, 2.5)
        puts Adder.new.add(2.5, 1)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3.5\n3.5\n");
}

#[test]
fn lambda_call_dot_call_and_bracket_syntax() {
    let result = run_ruby(
        r#"
        add = ->(x, y) { x + y }
        puts add.call(3, 4)
        puts add.(3, 4)
        puts add[3, 4]

        square = lambda { |x| x * x }
        puts square.call(5)

        incr = -> (n = 1) { n + 1 }
        puts incr.call
        puts incr.call(10)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\n7\n7\n25\n2\n11\n");
}

#[test]
fn lambda_return_and_break_terminate_only_the_lambda_itself() {
    // Real Ruby: unlike an ordinary Proc/block, `return`/`break` inside a
    // lambda act like a method boundary -- they terminate just the lambda
    // call, never the enclosing method.
    let result = run_ruby(
        r#"
        class Runner
          def return_test
            f = -> {
              return 10
              20
            }
            puts f.call
            "after"
          end

          def break_test
            g = -> {
              break 99
              100
            }
            puts g.call
            "after"
          end
        end

        r = Runner.new
        puts r.return_test
        puts r.break_test
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\nafter\n99\nafter\n");
}

#[test]
fn lambda_enforces_strict_arity_via_argument_error() {
    let result = run_ruby(
        r#"
        f = ->(x, y) { x + y }
        begin
          f.call(1)
        rescue ArgumentError => e
          puts "caught: #{e.message}"
        end
        puts f.call(1, 2)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "caught: wrong number of arguments (given 1, expected 2)\n3\n"
    );
}

// Phase 12.5 -- globals, namespaced constants, compound-assignment and
// multi-assignment completeness, call-site splats. Every test below is
// oracle-verified against real `ruby` first, per this project's established
// convention.

#[test]
fn global_variables_read_write_and_default_to_nil() {
    let result = run_ruby(
        r#"
        $x = 10
        puts $x
        puts $never_set
        $x += 5
        puts $x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n\n15\n");
}

#[test]
fn top_level_constant_read_and_write() {
    let result = run_ruby("MAX = 100\nputs MAX");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "100\n");
}

#[test]
fn constant_declared_in_a_superclass_resolves_from_a_subclass_method() {
    let result = run_ruby(
        r#"
        class Base
          X = 1
        end
        class Sub < Base
          def get
            X
          end
        end
        puts Sub.new.get
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n");
}

#[test]
fn namespaced_constant_read_via_double_colon() {
    let result = run_ruby(
        r#"
        class Foo
          BAR = 42
        end
        puts Foo::BAR
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn a_bare_constant_still_falls_back_to_the_top_level_from_inside_a_class() {
    let result = run_ruby(
        r#"
        MAX_ITEMS = 5
        class Config
          LIMIT = 3
          def show
            puts LIMIT
            puts MAX_ITEMS
          end
        end
        Config.new.show
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n5\n");
}

#[test]
fn unset_constant_raises_a_name_error() {
    let result = run_ruby(
        r#"
        begin
          puts UNDEFINED_CONST
        rescue NameError => e
          puts "caught: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught: uninitialized constant UNDEFINED_CONST\n");
}

#[test]
fn local_or_assign_and_and_assign() {
    let result = run_ruby(
        r#"
        x = nil
        x ||= 5
        puts x
        y = 1
        y &&= 2
        puts y
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\n2\n");
}

#[test]
fn compound_assignment_on_an_attribute_evaluates_the_receiver_exactly_once() {
    let result = run_ruby(
        r#"
        class Box
          attr_accessor :n
          def initialize
            @n = 0
          end
        end
        class Tracker
          attr_reader :calls
          def initialize(box)
            @box = box
            @calls = 0
          end
          def get
            @calls += 1
            @box
          end
        end

        b = Box.new
        t = Tracker.new(b)
        t.get.n += 5
        puts b.n
        puts t.calls
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\n1\n");
}

#[test]
fn or_assign_on_an_attribute_short_circuits_without_calling_the_setter() {
    let result = run_ruby(
        r#"
        class Box
          attr_accessor :n
        end
        b = Box.new
        b.n = 5
        b.n ||= 99
        puts b.n
        b.n = nil
        b.n ||= 42
        puts b.n
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\n42\n");
}

#[test]
fn compound_and_or_assignment_on_an_array_index() {
    let result = run_ruby(
        r#"
        arr = [1, 2, 3]
        arr[0] += 10
        puts arr[0]
        arr[1] ||= 99
        puts arr[1]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "11\n2\n");
}

#[test]
fn nested_destructuring_multi_assign() {
    let result = run_ruby(
        r#"
        (a, b), c = [[1, 2], 3]
        puts a
        puts b
        puts c
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n2\n3\n");
}

#[test]
fn multi_assign_to_ivar_and_global_targets() {
    let result = run_ruby(
        r#"
        class Thing
          attr_reader :x
          def set_both(g)
            @x, $g2 = 10, g
          end
        end
        t = Thing.new
        t.set_both(99)
        puts t.x
        puts $g2
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n99\n");
}

#[test]
fn for_loop_destructures_each_pair() {
    let result = run_ruby(
        r#"
        for aa, bb in [[1, 2], [3, 4]]
          puts aa + bb
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n7\n");
}

#[test]
fn call_site_splat_expands_an_array_into_positional_arguments() {
    let result = run_ruby(
        r#"
        class Adder
          def add3(a, b, c)
            a + b + c
          end
        end
        a = Adder.new
        arr = [1, 2, 3]
        puts a.add3(*arr)
        puts a.add3(1, *[2, 3])
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n6\n");
}

#[test]
fn call_site_splat_on_an_implicit_self_sibling_call() {
    let result = run_ruby(
        r#"
        class Adder
          def add3(a, b, c)
            a + b + c
          end
          def run(arr)
            add3(*arr)
          end
        end
        puts Adder.new.run([1, 2, 3])
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n");
}

#[test]
#[should_panic(expected = "splat argument with keyword arguments isn't supported yet")]
fn kwargs_double_splat_at_a_call_site_is_a_clean_error() {
    // A call-site `**h` double-splat has no Path 2 (`spinel_rt::send`)
    // keyword channel at all (matches this file's other keyword-argument
    // dynamic-dispatch restrictions above) -- a clean codegen-time panic,
    // not silently dropped or misdispatched.
    let _ = spinelc::compile_to_rust(
        r#"
        class Greeter
          def f(x:)
            x
          end
        end
        h = {x: 1}
        puts Greeter.new.f(**h)
        "#,
    );
}

// Phase 12.6 -- a real Hash table (IndexMap-backed, structural keys for
// built-in types, insertion-order preserved) and built-in types
// (Integer/Float/String/Symbol/Array/Hash/Range/NilClass/TrueClass/
// FalseClass/Proc) wired into the same ClassId/ancestors system every
// user-defined class already goes through, so `is_a?`/`kind_of?` resolve
// correctly against a built-in-typed receiver instead of panicking.

#[test]
fn hash_reassigning_an_existing_key_keeps_its_original_insertion_position() {
    let result = run_ruby(
        r#"
        h = {}
        h[:a] = 1
        h[:b] = 2
        h[:c] = 3
        h[:a] = 99
        puts h
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "{a: 99, b: 2, c: 3}\n");
}

#[test]
fn hash_keyed_by_an_array_hashes_structurally_not_by_identity() {
    let result = run_ruby(
        r#"
        h = {}
        h[[1, 2]] = "pair"
        puts h[[1, 2]]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "pair\n");
}

#[test]
fn is_a_and_kind_of_against_a_statically_known_int_local() {
    let result = run_ruby(
        r#"
        x = 5
        puts x.is_a?(Integer)
        puts x.is_a?(String)
        puts x.is_a?(Object)
        puts x.kind_of?(Integer)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\ntrue\ntrue\n");
}

#[test]
fn is_a_against_a_poly_typed_method_parameter() {
    // A method PARAMETER is always `TyKind::Poly` (spinelc never infers a
    // param's type from its call sites) -- this exercises the runtime
    // `spinel_rt::is_a` fallback (via the new universal `RubyValue::class_id`)
    // rather than the static ancestors-constant-fold path.
    let result = run_ruby(
        r#"
        class Checker
          def check(v)
            v.is_a?(Integer)
          end
        end
        c = Checker.new
        puts c.check(5)
        puts c.check("hi")
        puts c.check([1, 2])
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\nfalse\n");
}

#[test]
fn is_a_for_nil_true_and_false_against_their_own_singleton_classes() {
    let result = run_ruby(
        r#"
        puts nil.is_a?(NilClass)
        puts true.is_a?(TrueClass)
        puts false.is_a?(FalseClass)
        puts true.is_a?(Object)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\ntrue\ntrue\n");
}

#[test]
fn is_a_against_statically_typed_array_hash_range_and_symbol_locals() {
    let result = run_ruby(
        r#"
        arr = [1, 2, 3]
        puts arr.is_a?(Array)
        puts arr.is_a?(Object)
        puts arr.is_a?(Hash)
        h = {a: 1}
        puts h.is_a?(Hash)
        r = 1..5
        puts r.is_a?(Range)
        sym = :foo
        puts sym.is_a?(Symbol)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\nfalse\ntrue\ntrue\ntrue\n");
}

#[test]
fn subclassing_a_built_in_type_is_a_clean_error() {
    let err = spinelc::compile_to_rust(
        r#"
        class MyInt < Integer
        end
        "#,
    )
    .unwrap_err();
    assert!(err.contains("subclassing the built-in type"), "{err}");
}

#[test]
fn reopening_object_defines_methods_reachable_everywhere() {
    // An Object reopen is top-level `def` by another name: its methods
    // land on arena slot 0, materialize into every user class, and
    // dispatch on the runtime `main` object at top-level call sites.
    let result = run_ruby(
        r#"
        class Object
          def double(x)
            x * 2
          end
        end
        puts double(21)
        class Widget
          def go
            double(4)
          end
        end
        puts Widget.new.go
        "#,
    );
    assert_eq!(result.stdout, "42\n8\n");
    assert_eq!(result.stderr, "");
}

// --- Phase 12.7: Regexp -----------------------------------------------
//
// Backed by the `regex` crate, not Ruby's own Onigmo engine -- see
// `hir::HirNode::RegexpLit`'s docs for the documented semantic gap (no
// backreferences/lookaround INSIDE a pattern) and `spinel_rt::regexp`'s
// docs for the flag-translation rationale (Ruby's `^`/`$` are ALWAYS
// line-anchored, unlike most other regex flavors -- confirmed against real
// `ruby` in `mline_anchors_are_always_line_based_even_without_the_m_flag`
// below). Every test here was run against real `ruby` first, per this
// project's established convention.

#[test]
fn string_match_operator_returns_char_index_or_nil() {
    let result = run_ruby(
        r#"
        puts("hello world" =~ /world/)
        puts("hello world" =~ /xyz/)
        puts("hello" =~ /l+/)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n\n2\n");
}

#[test]
fn match_p_works_from_both_the_string_and_the_regexp_receiver() {
    let result = run_ruby(
        r#"
        puts "hello".match?(/l+/)
        puts "hello".match?(/xyz/)
        puts(/l+/.match?("hello"))
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\ntrue\n");
}

#[test]
fn not_match_operator_negates_from_both_receivers() {
    let result = run_ruby(
        r#"
        puts("hello" !~ /xyz/)
        puts("hello" !~ /l+/)
        puts(/xyz/ !~ "hello")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\ntrue\n");
}

#[test]
fn matchdata_whole_match_captures_pre_and_post_match() {
    let result = run_ruby(
        r#"
        m = "hello world".match(/(\w+) (\w+)/)
        puts m[0]
        puts m[1]
        puts m[2]
        puts m.pre_match
        puts m.post_match
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hello world\nhello\nworld\n\n\n");
}

#[test]
fn matchdata_to_a_and_captures() {
    let result = run_ruby(
        r#"
        m = "hello world".match(/(\w+) (\w+)/)
        puts m.to_a
        puts "---"
        puts m.captures
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hello world\nhello\nworld\n---\nhello\nworld\n");
}

#[test]
fn matchdata_named_group_access_by_symbol_string_and_named_captures_hash() {
    let result = run_ruby(
        r#"
        m = "John Smith".match(/(?<first>\w+) (?<last>\w+)/)
        puts m[:first]
        puts m["last"]
        h = m.named_captures
        puts h["first"]
        puts h["last"]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "John\nSmith\nJohn\nSmith\n");
}

#[test]
fn ignore_case_flag() {
    let result = run_ruby(
        r#"
        puts "HELLO".match?(/hello/i)
        puts "HELLO".match?(/hello/)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\n");
}

#[test]
fn line_anchors_are_always_line_based_even_without_the_m_flag() {
    // Real Ruby's `^`/`$` ALWAYS match at line boundaries -- there is no
    // separate opt-in the way most other regex flavors need one. `/m`
    // instead makes `.` match a newline too (verified against real `ruby`
    // directly, since this is a common point of confusion between Ruby's
    // flag vocabulary and most other engines').
    let result = run_ruby(
        r#"
        puts("line1\nline2" =~ /^line2/)
        s = "abc\ndef"
        puts(s =~ /c.d/)
        puts(s =~ /c.d/m)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n\n2\n");
}

#[test]
fn to_s_renders_the_canonical_opts_body_form_and_inspect_the_literal_form() {
    let result = run_ruby(
        r#"
        r = /abc/x
        puts r.to_s
        r2 = /a.c/im
        puts r2.to_s
        puts(/abc/x.inspect)
        puts(/abc/mi.inspect)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "(?x-mi:abc)\n(?mi-x:a.c)\n/abc/x\n/abc/mi\n"
    );
}

#[test]
fn scan_without_and_with_capture_groups() {
    let result = run_ruby(
        r#"
        puts "one two three".scan(/\w+/)
        puts "---"
        puts "a1b2c3".scan(/([a-z])(\d)/)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "one\ntwo\nthree\n---\na\n1\nb\n2\nc\n3\n");
}

#[test]
fn split_keeps_leading_and_embedded_empties_but_drops_trailing_ones() {
    let result = run_ruby(
        r#"
        puts "a,b,,c".split(/,/)
        puts "---"
        puts ",a,b".split(/,/)
        puts "---"
        puts "a1b2c3".split(/\d/)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "a\nb\n\nc\n---\n\na\nb\n---\na\nb\nc\n");
}

#[test]
fn split_with_no_match_returns_the_whole_string_unsplit() {
    // The empty-haystack-returns-an-empty-array case is exercised directly
    // at the `spinel_rt::regexp_split` unit-test level instead (see
    // `spinel-rt/src/regexp.rs`) -- `puts`-ing it here would conflate this
    // phase's own behavior with a separate, pre-existing, unrelated gap:
    // `Kernel#puts` on an EMPTY `Array` currently prints a blank line
    // (real Ruby prints nothing at all for `puts []`) -- confirmed via a
    // plain, regex-free `puts []; puts "x"` repro, so not something this
    // phase introduces or should fix as a side effect. `Array#length`
    // doesn't sidestep this cleanly either: `split`'s result has no static
    // `TyKind` seeding (unlike a literal `[]`), so it stays `Poly` and hits
    // the same "no static-array fast path" limitation.
    let result = run_ruby(
        r#"
        puts "abc".split(/x/)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "abc\n");
}

#[test]
fn gsub_and_sub_with_a_string_replacement_including_numbered_backreferences() {
    let result = run_ruby(
        r#"
        puts "hello world".gsub(/o/, "0")
        puts "hello world".sub(/o/, "0")
        puts "John Smith".gsub(/(\w+) (\w+)/, '\2 \1')
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hell0 w0rld\nhell0 world\nSmith John\n");
}

#[test]
fn gsub_and_sub_block_forms() {
    // The block body is a literal replacement (not `m.upcase`) since a
    // block parameter's static type is always `Poly` (this codebase's own
    // established rule -- params are never inferred from call sites), and
    // `String#upcase` itself isn't implemented as a builtin method yet, a
    // separate, pre-existing, unrelated gap this test isn't about.
    let result = run_ruby(
        r#"
        puts "hello".gsub(/l/) { |m| "L" }
        puts "hello".sub(/l/) { "L" }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "heLLo\nheLlo\n");
}

#[test]
fn gsub_block_form_can_capture_and_mutate_an_enclosing_local() {
    // Exercises the same real-`Proc`/capture machinery every other escaping
    // block already uses (`emit_proc_value`) -- not a bespoke code path.
    let result = run_ruby(
        r#"
        count = 0
        "one two three".gsub(/\w+/) { |w| count += 1; w }
        puts count
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n");
}

#[test]
fn regexp_case_eq_and_source() {
    let result = run_ruby(
        r#"
        puts(/abc/ =~ "xxabcxx")
        r = /foo/
        puts r.source
        puts(r === "foobar")
        puts(r === "baz")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\nfoo\ntrue\nfalse\n");
}

#[test]
fn case_when_dispatches_via_real_regexp_case_eq() {
    // Method wrapped in a class, not a top-level `def` -- calling a
    // top-level-defined method is a separate, pre-existing, unrelated gap
    // (confirmed via a plain, regex-free repro), not something this phase
    // introduces or should fix as a side effect.
    let result = run_ruby(
        r#"
        class Checker
          def check(x)
            case x
            when /^\d+$/
              "number"
            when /^[a-z]+$/
              "lower"
            else
              "other"
            end
          end
        end
        c = Checker.new
        puts c.check("123")
        puts c.check("abc")
        puts c.check("ABC")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "number\nlower\nother\n");
}

#[test]
fn case_in_pattern_matching_dispatches_via_real_regexp_case_eq() {
    let result = run_ruby(
        r#"
        class Checker
          def check(x)
            case x
            in /^\d+$/
              "number"
            in /^[a-z]+$/
              "lower"
            else
              "other"
            end
          end
        end
        c = Checker.new
        puts c.check("123")
        puts c.check("abc")
        puts c.check("ABC")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "number\nlower\nother\n");
}

#[test]
fn interpolated_regexp_literal_shares_the_enclosing_scope() {
    let result = run_ruby(
        r#"
        word = "wor"
        r = /#{word}ld/
        puts r.match?("world")
        puts r.source
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nworld\n");
}

#[test]
fn percent_r_literal_syntax() {
    let result = run_ruby(
        r#"
        r = %r{foo/bar}
        puts r.match?("xxfoo/barxx")
        puts r.source
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfoo/bar\n");
}

#[test]
fn extended_mode_ignores_whitespace_and_comments() {
    let result = run_ruby(
        r#"
        r = /
          \d+  # a number
          -
          \d+  # another number
        /x
        puts r.match?("123-456")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\n");
}

#[test]
fn is_a_against_regexp_and_matchdata() {
    let result = run_ruby(
        r#"
        r = /abc/
        puts r.is_a?(Regexp)
        puts r.is_a?(Object)
        m = "abc".match(/a/)
        puts m.is_a?(MatchData)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\ntrue\n");
}

#[test]
fn an_invalid_interpolated_pattern_raises_a_catchable_regexp_error() {
    // A STATIC (non-interpolated) invalid pattern is a real, uncatchable
    // `SyntaxError` at parse time in real Ruby -- this spike defers that
    // check to construction time uniformly (a documented, narrower-timing
    // approximation, see `hir::HirNode::RegexpLit`'s docs), so only the
    // INTERPOLATED case (genuinely runtime-only in real Ruby too) is
    // oracle-verified here as a rescuable exception.
    let result = run_ruby(
        r#"
        bad = "("
        begin
          r = /#{bad}/
          puts "no error"
        rescue RegexpError => e
          puts "regexp error"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "regexp error\n");
}

#[test]
fn a_bare_regexp_literal_used_as_an_implicit_condition_is_a_clean_lowering_error() {
    let err = spinelc::compile_to_rust(
        r#"
        if /foo/
          puts "matched"
        end
        "#,
    )
    .unwrap_err();
    assert!(err.contains("implicit condition"), "{err}");
}

#[test]
fn named_capture_auto_binding_via_match_write_is_a_clean_lowering_error() {
    let err = spinelc::compile_to_rust(
        r#"
        /(?<name>\w+)/ =~ "hello"
        puts name
        "#,
    )
    .unwrap_err();
    assert!(err.contains("auto-binding"), "{err}");
}

/// SEMANTICS FLIP (Phase 17.1-D): String patterns to split/gsub used to be
/// compile-time rejections ("pass a Regexp literal instead"); the String
/// table rows now implement them for real, so the static regexp path falls
/// through to dynamic dispatch instead. Oracle-verified.
#[test]
fn string_patterns_to_split_and_gsub_now_work() {
    let result = support::run_ruby(
        r#"
        puts "a,b".split(",").inspect
        puts "a,b".gsub(",", ";")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[\"a\", \"b\"]\na;b\n");
}

// --- Phase 12.8: alias / class << self ---------------------------------
//
// `alias` is resolved entirely at LOWERING time (`parse::lower_class_body_statement`):
// the aliased name's already-lowered `DefMethod` (params/body/visibility)
// is cloned under the new name, so no new analyze/codegen machinery exists
// at all -- it's indistinguishable from having written the method body
// twice under two names. `class << self` similarly desugars its nested
// `def`s to ordinary `is_class_method: true` `DefMethod`s (reusing
// `ClassInfo::class_methods` materialization Phase 12.6/Part 6 already
// built). Every test here was run against real `ruby` first, per this
// project's established convention.

#[test]
fn alias_creates_a_second_callable_name_for_the_same_method() {
    let result = run_ruby(
        r#"
        class Greeter
          def hello
            "hi"
          end
          alias hola hello
        end
        g = Greeter.new
        puts g.hello
        puts g.hola
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi\nhi\n");
}

#[test]
fn alias_supports_the_symbol_spelling_of_both_names() {
    let result = run_ruby(
        r#"
        class Greeter
          def hello
            "hi"
          end
          alias :bonjour :hello
        end
        puts Greeter.new.bonjour
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi\n");
}

#[test]
fn alias_preserves_the_original_methods_own_parameters() {
    let result = run_ruby(
        r#"
        class Calc
          def add(a, b)
            a + b
          end
          alias sum add
        end
        puts Calc.new.sum(3, 4)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\n");
}

#[test]
fn alias_captures_the_overriding_bodys_own_behavior_in_a_subclass() {
    // `alias` binds to whichever body is ALREADY in effect at the alias
    // statement's own position -- here, `Dog`'s own override (found in
    // `Dog`'s `own_methods`), not `Animal`'s.
    let result = run_ruby(
        r#"
        class Animal
          def speak
            "generic"
          end
        end
        class Dog < Animal
          def speak
            "woof"
          end
          alias original_speak speak
        end
        d = Dog.new
        puts d.speak
        puts d.original_speak
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "woof\nwoof\n");
}

#[test]
fn alias_of_a_method_touching_an_ivar_works_through_either_name() {
    let result = run_ruby(
        r#"
        class Counter
          def initialize
            @count = 0
          end
          def increment
            @count += 1
          end
          alias inc increment
          def value
            @count
          end
        end
        c = Counter.new
        c.inc
        c.inc
        c.increment
        puts c.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n");
}

#[test]
fn class_shift_self_defines_multiple_class_methods_at_once() {
    let result = run_ruby(
        r#"
        class MathUtils
          class << self
            def square(x)
              x * x
            end
            def cube(x)
              x * x * x
            end
          end
        end
        puts MathUtils.square(4)
        puts MathUtils.cube(3)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "16\n27\n");
}

#[test]
fn class_shift_self_works_inside_a_module() {
    let result = run_ruby(
        r#"
        module MyMath
          class << self
            def double(x)
              x * 2
            end
          end
        end
        puts MyMath.double(5)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n");
}

#[test]
fn class_shift_self_methods_can_read_and_write_class_variables() {
    let result = run_ruby(
        r#"
        class Counter
          class << self
            def reset
              @@total = 0
            end
            def bump
              @@total += 1
            end
            def total
              @@total
            end
          end
        end
        Counter.reset
        Counter.bump
        Counter.bump
        puts Counter.total
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n");
}

#[test]
fn aliasing_a_method_not_yet_defined_in_the_same_body_is_a_clean_lowering_error() {
    let err = spinelc::compile_to_rust(
        r#"
        class Foo
          alias bar undefined_method
        end
        "#,
    )
    .unwrap_err();
    assert!(err.contains("must already be defined earlier"), "{err}");
}

#[test]
fn class_shift_an_expression_other_than_self_is_a_clean_lowering_error() {
    let err = spinelc::compile_to_rust(
        r#"
        class Foo
          ANOTHER = Object.new
          class << ANOTHER
            def hi
              "hi"
            end
          end
        end
        "#,
    )
    .unwrap_err();
    assert!(err.contains("per-instance singleton class"), "{err}");
}

#[test]
fn class_shift_self_containing_a_non_def_statement_is_a_clean_lowering_error() {
    let err = spinelc::compile_to_rust(
        r#"
        class Foo
          class << self
            @x = 1
          end
        end
        "#,
    )
    .unwrap_err();
    assert!(err.contains("may only contain `def`s"), "{err}");
}

// --- Phase 12.9: bugs found via a comprehensive sweep (fixed, not deferred) ---
//
// Found by testing constructs adjacent to Phases 12.1-12.8's own work, not
// by design review -- each is a real, previously-undetected defect, not a
// documented scope-cut. Every test here was run against real `ruby` first,
// per this project's established convention.

#[test]
fn implicit_self_call_between_sibling_class_methods_dispatches_correctly() {
    // Found while testing `class << self` (Phase 12.8): `current_class` is
    // `None` inside a class method's own `Ctx` (no concrete `self` receiver
    // exists there), so a no-receiver call to a SIBLING class method
    // (`def self.a; b; end` calling `def self.b`) always panicked --
    // `codegen::call::emit_call`'s implicit-self branch only ever consulted
    // `current_class`. Fixed by also checking `defining_class` against
    // `Compiler::class_method_in_chain`, dispatching as a direct
    // associated-function call, same as `ClassName.foo(...)`.
    let result = run_ruby(
        r#"
        class Widget
          def self.create
            helper
          end
          def self.helper
            "made"
          end
        end
        puts Widget.create

        class MathUtils
          class << self
            def sum_of_squares(a, b)
              square(a) + square(b)
            end
            def square(x)
              x * x
            end
          end
        end
        puts MathUtils.sum_of_squares(3, 4)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "made\n25\n");
}

#[test]
fn multi_assign_into_attr_targets_correctly_calls_the_setter() {
    // A real, previously-undetected bug: `CallTargetNode::name()` is
    // ALREADY the setter name (`:x=`, confirmed via `Prism.parse`), but
    // `parse::lower_multi_target` appended ANOTHER `=`, building an
    // unresolvable `x==` method name -- so ANY multi-assignment into two
    // attr targets (`b.x, b.y = b.y, b.x`, the idiomatic in-place swap)
    // panicked with a confusing "unsupported call `x==`" instead of
    // swapping the values. Array-index multi-assign targets (`arr[0],
    // arr[1] = ...`) were unaffected (a distinct code path, `[]=`, not
    // string-formatted from a name at all).
    let result = run_ruby(
        r#"
        arr = [1, 2, 3, 4]
        arr[0], arr[1] = arr[1], arr[0]
        puts arr[0]
        puts arr[1]

        class Box
          attr_accessor :x, :y
          def initialize(x, y)
            @x = x
            @y = y
          end
        end
        b = Box.new(1, 2)
        b.x, b.y = b.y, b.x
        puts b.x
        puts b.y
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n1\n2\n1\n");
}

#[test]
fn user_defined_operator_methods_can_be_defined_and_dispatched() {
    // A real, previously-undetected bug: `safe_ident` had no escaping at
    // all for a method name that's a bare operator symbol (`+`, `<=>`,
    // `[]`, ...) -- `def +(other)` panicked at CODEGEN time with a raw
    // `proc_macro2` "not a valid Ident" error. This meant user-defined
    // operator overloading -- claimed since Phase 1 to "work for free"
    // once operators became ordinary Calls -- never actually worked for
    // the DEFINING side; every existing operator test exercised only
    // native `Int`/`Float` fast paths, which bypass `safe_ident` entirely.
    // Fixed via a fixed lookup table (`OPERATOR_METHOD_NAMES`) mapping
    // every operator symbol to a valid Rust identifier, consulted by BOTH
    // the method-definition side and the general-dispatch call site (the
    // same function backs both), so they agree automatically.
    let result = run_ruby(
        r#"
        class Vector
          attr_reader :x, :y
          def initialize(x, y)
            @x = x
            @y = y
          end
          def +(other)
            Vector.new(@x + other.x, @y + other.y)
          end
          def <=>(other)
            (@x * @x + @y * @y) <=> (other.x * other.x + other.y * other.y)
          end
          def to_s
            "(#{@x}, #{@y})"
          end
        end
        v1 = Vector.new(1, 2)
        v2 = Vector.new(3, 4)
        v3 = v1 + v2
        puts v3.to_s
        puts(v1 <=> v2)
        puts(v2 <=> v1)
        puts(v1 <=> v1)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "(4, 6)\n-1\n1\n0\n");
}

#[test]
fn reading_the_same_ivar_or_captured_local_twice_in_one_expression_does_not_deadlock() {
    // CRITICAL bug, found via the operator-method test above (`@x * @x`
    // inside `Vector#<=>`): `codegen::expr`'s `IvarRead` and
    // `codegen::hoisting`'s captured-`LocalRead` both emitted a bare,
    // UNNAMED `#expr.lock().clone()` -- Rust's temporary-lifetime rule
    // keeps an unnamed `.lock()` guard alive until the end of the
    // ENCLOSING STATEMENT (confirmed via a minimal, standalone
    // `parking_lot::Mutex` repro), so referencing the SAME ivar or
    // captured local TWICE in one expression/statement (a very common
    // shape: squaring, self-comparison, `total - total`, not just the
    // already-audited read-modify-WRITE case Part 9 covered) silently
    // deadlocked the whole generated program forever -- no panic, no
    // error, just a permanent hang. Fixed by binding the guard to an
    // explicit named local INSIDE its own block (confirmed empirically
    // that a bare `{ }` wrapper alone does NOT change the drop timing --
    // only a named `let` binding does).
    let result = run_ruby(
        r#"
        class Squarer
          def initialize(n)
            @n = n
          end
          def square
            @n * @n
          end
        end
        puts Squarer.new(7).square

        class Once
          def run
            yield
          end
        end
        total = 5
        block_result = 0
        Once.new.run { block_result = total * total }
        puts block_result
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "49\n25\n");
}

#[test]
fn or_assign_on_a_never_before_defined_constant_quietly_defines_it() {
    // A real, previously-undetected bug: `CONST ||= v` on a constant that
    // was NEVER assigned raised `NameError` instead of quietly defining
    // it -- confirmed via real `ruby` that this is a genuine, narrow
    // special case (ONLY `||=` gets this leniency; `+=`/`&&=` on the same
    // undefined constant still raise, see the next test). Fixed via a new
    // `HirNode::ConstReadOrNil`, used only by `||=`'s own desugar.
    let result = run_ruby(
        r#"
        MAX ||= 100
        puts MAX
        MAX ||= 200
        puts MAX
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "100\n100\n");
}

#[test]
fn other_compound_assignment_operators_still_raise_on_an_undefined_constant() {
    // Regression guard for the fix above: `||=`'s new leniency must NOT
    // leak into `+=`/`&&=` -- confirmed against real `ruby` that those
    // still raise on a genuinely undefined constant. An uncaught Ruby
    // exception exits the compiled binary nonzero with a message on
    // stderr (see `uncaught_raise_with_no_rescue_anywhere_exits_with_the_message`),
    // not a Rust-level panic, so this asserts on the process output
    // directly rather than `#[should_panic]`.
    let result = run_ruby("UNDEF += 1");
    assert!(!result.status.success());
    assert!(result.stderr.contains("uninitialized constant UNDEF"), "{}", result.stderr);
}

#[test]
fn compound_assignment_on_a_plain_non_class_constant_dispatches_correctly() {
    // A real, previously-undetected bug, found alongside the `||=` fix
    // above: `codegen::call::emit_call` treated ANY `HirNode::ClassRef`
    // receiver as a `ClassName.foo(...)` class-method call, UNCONDITIONALLY
    // -- but `ClassRef` is also how an ordinary bare constant read lowers
    // (see its own docs). So `MAX += 1` (desugared to `MAX.+(1)`, where
    // `MAX` is a plain `Integer` constant, not a class) panicked with a
    // confusing "unknown class/module `MAX`" instead of reading/writing
    // its actual value -- EVERY compound-assignment operator except `||=`
    // on any non-class constant was completely broken. Fixed by only
    // taking the class-method-call branch when the name is ACTUALLY a
    // registered class/module.
    let result = run_ruby(
        r#"
        MAX = 100
        MAX += 1
        puts MAX
        MAX -= 50
        puts MAX
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "101\n51\n");
}

#[test]
fn integer_division_and_modulo_by_zero_raise_a_catchable_zero_division_error() {
    // A real, previously-undetected bug: `1 / 0` crashed the ENTIRE
    // generated binary with a raw Rust panic (`spinel_rt::int_div`'s own
    // internal `/` panicking) instead of raising a catchable
    // `ZeroDivisionError` -- real Ruby's actual behavior. This affected
    // BOTH the static `Int`-`Int` fast path and the runtime-checked
    // fallback used for method-parameter operands (always `Poly`). Fixed
    // via `emit_int_div_or_mod_checked`, consulted at both call sites.
    let result = run_ruby(
        r#"
        begin
          puts(5 % 0)
        rescue ZeroDivisionError
          puts "mod zero div"
        end
        class Calc
          def divide(a, b)
            a / b
          end
        end
        begin
          puts Calc.new.divide(10, 0)
        rescue ZeroDivisionError
          puts "param zero div"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "mod zero div\nparam zero div\n");
}

#[test]
fn float_division_by_zero_returns_infinity_not_an_error() {
    // Regression guard for the fix above: `ZeroDivisionError` must stay
    // `Int`/`Int`-division-specific -- real Ruby's `Float`/`0` is
    // `Infinity`/`-Infinity`/`NaN` (IEEE semantics), never a raised
    // exception, and `float_div` already gives this for free with no
    // check needed.
    let result = run_ruby("puts(1.0 / 0.0); puts(-1.0 / 0.0)");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Infinity\n-Infinity\n");
}

#[test]
fn bitwise_operators_on_integers() {
    let result = run_ruby(
        r#"
        a = 12
        b = 10
        puts a & b
        puts a | b
        puts a ^ b
        puts a << 2
        puts a >> 2
        puts ~a
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "8\n14\n6\n48\n3\n-13\n");
}

#[test]
fn attr_reader_and_attr_writer_declared_standalone_generate_only_their_own_half() {
    let result = run_ruby(
        r#"
        class Point
          attr_reader :x
          attr_writer :y
          def initialize(x, y)
            @x = x
            @y = y
          end
          def show_y
            @y
          end
        end
        p1 = Point.new(1, 2)
        puts p1.x
        p1.y = 99
        puts p1.show_y
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n99\n");
}

#[test]
fn combined_optional_rest_post_keyword_keyword_rest_and_block_params_all_bind_correctly() {
    let result = run_ruby(
        r#"
        class Widget
          def build(a, b = 10, *rest, c, d: 5, **kw, &blk)
            puts a
            puts b
            puts rest
            puts c
            puts d
            puts kw[:e]
            puts blk.call
          end
        end
        Widget.new.build(1, 2, 3, 4, 5, d: 99, e: 100) { "block!" }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n2\n3\n4\n5\n99\n100\nblock!\n");
}

// ---------------------------------------------------------------------------
// Phase 13.1: .freeze / .frozen? -- every snippet oracle-verified against
// real `ruby` first, per this project's standing convention. Semantics
// grounded in CRuby's actual implementation (see the plan's Part 11
// addendum): freeze is SHALLOW, returns self, no-ops when repeated;
// immediates and Ranges are always frozen; mutation of a frozen value
// raises a catchable FrozenError (a RuntimeError subclass) with the message
// `can't modify frozen <Class>: <inspect>`, checked at the top of every
// mutator after argument evaluation.
// ---------------------------------------------------------------------------

#[test]
fn freezing_an_array_makes_index_assignment_raise_a_catchable_frozen_error() {
    let result = run_ruby(
        r#"
        a = [1, 2, 3]
        a.freeze
        begin
          a[0] = 9
        rescue FrozenError => e
          puts e.send(:message)
        end
        puts a[0]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "can't modify frozen Array: [1, 2, 3]\n1\n");
}

#[test]
fn frozen_predicate_flips_after_freeze_and_freeze_returns_self() {
    let result = run_ruby(
        r#"
        a = [1]
        puts a.frozen?
        b = a.freeze
        puts a.frozen?
        puts b.frozen?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "false\ntrue\ntrue\n");
}

#[test]
fn freeze_is_shallow_so_a_frozen_containers_elements_stay_mutable() {
    let result = run_ruby(
        r#"
        inner = [1, 2]
        b = [inner]
        b.freeze
        inner[0] = 99
        puts inner[0]
        puts b.frozen?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "99\ntrue\n");
}

#[test]
fn immediates_and_ranges_are_always_frozen() {
    let result = run_ruby(
        r#"
        puts 1.frozen?
        puts 1.5.frozen?
        puts :sym.frozen?
        puts nil.frozen?
        puts true.frozen?
        puts (1..5).frozen?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\ntrue\ntrue\ntrue\ntrue\n");
}

#[test]
fn frozen_hash_assignment_raises_with_the_inspect_bearing_message() {
    let result = run_ruby(
        r#"
        h = { a: 1 }
        h.freeze
        begin
          h[:b] = 2
        rescue FrozenError => e
          puts e.send(:message)
        end
        puts h.length
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "can't modify frozen Hash: {a: 1}\n1\n");
}

#[test]
fn frozen_string_assignment_raises_with_the_quoted_inspect_message() {
    let result = run_ruby(
        r#"
        s = "abc"
        s.freeze
        begin
          s[0] = "z"
        rescue FrozenError => e
          puts e.send(:message)
        end
        puts s
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "can't modify frozen String: \"abc\"\nabc\n");
}

#[test]
fn ivar_write_on_a_frozen_object_raises_and_leaves_the_ivar_unchanged() {
    // The message's `#<Pt>` receiver rendering is this compiler's documented
    // static approximation of real Ruby's `#<Pt:0xaddr @x=1>` (no per-object
    // address/ivar reflection exists) -- the SEMANTICS (raises a catchable
    // FrozenError, the ivar keeps its old value, readers still work) are
    // oracle-verified exactly.
    let result = run_ruby(
        r#"
        class Pt
          def initialize(x)
            @x = x
          end
          def set_x(v)
            @x = v
          end
          def x
            @x
          end
        end
        p1 = Pt.new(1)
        p1.freeze
        puts p1.frozen?
        begin
          p1.set_x(5)
        rescue FrozenError => e
          puts e.send(:message)
        end
        puts p1.x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\ncan't modify frozen Pt: #<Pt>\n1\n"
    );
}

#[test]
fn frozen_error_is_caught_by_a_runtime_error_rescue() {
    let result = run_ruby(
        r#"
        g = [1].freeze
        begin
          g[0] = 2
        rescue RuntimeError => e
          puts "runtime-rescued"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "runtime-rescued\n");
}

#[test]
fn repeated_freeze_is_a_silent_no_op() {
    let result = run_ruby(
        r#"
        c = "hi"
        c.freeze
        c.freeze
        puts c.frozen?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\n");
}

#[test]
fn freeze_returns_self_keeping_the_static_collection_type() {
    // Exercises `types.rs`'s `.freeze`-returns-self inference: `names[0]`/
    // `names.length` must still take the static Array fast path (a Poly
    // fallback would panic). A LOCAL, not the classic `NAMES = [...].freeze`
    // constant idiom: a constant READ is always Poly (constants live in a
    // runtime map with no static type tracking) -- a PRE-existing gap that
    // makes `A = [1]; A[0]` fail with or without `.freeze` involved, noted
    // for a later phase, not a freeze regression.
    let result = run_ruby(
        r#"
        names = ["a", "b"].freeze
        puts names[0]
        puts names.length
        puts names.frozen?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "a\n2\ntrue\n");
}

#[test]
fn compound_index_assignment_in_tail_position_compiles() {
    // Regression test for a pre-existing codegen bug found during the
    // freeze work (not freeze-related): `HirNode::Seq` -- the lowering
    // artifact behind `arr[i] += 1` -- emitted a brace-less statement
    // sequence from `emit_expr`, producing invalid Rust (`Ok(stmt; stmt;
    // expr)`) whenever the compound assignment was the LAST statement of a
    // method/`begin` body. Latent since the `Seq` arm was written; every
    // prior test happened to put a statement after it.
    let result = run_ruby(
        r#"
        class Bumper
          def bump
            arr = [10]
            arr[0] += 1
          end
        end
        puts Bumper.new.bump
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "11\n");
}

#[test]
fn compound_assignment_on_a_frozen_array_raises_after_the_read() {
    // `d[0] += 1` desugars to a read (fine on a frozen array) then a `[]=`
    // (raises) -- real Ruby's own order.
    let result = run_ruby(
        r#"
        d = [1]
        d.freeze
        begin
          d[0] += 1
        rescue FrozenError => e
          puts e.send(:message)
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "can't modify frozen Array: [1]\n");
}

#[test]
fn multi_assignment_into_a_frozen_index_target_raises() {
    let result = run_ruby(
        r#"
        f = [1, 2]
        f.freeze
        begin
          f[0], x = 5, 6
        rescue FrozenError => e
          puts e.send(:message)
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "can't modify frozen Array: [1, 2]\n");
}

#[test]
fn a_user_defined_freeze_override_wins_over_the_builtin() {
    // `freeze`/`frozen?` are ordinary overridable Kernel methods in real
    // Ruby -- a class's own definition must win over the universal dispatch.
    let result = run_ruby(
        r#"
        class Custom
          def freeze
            :custom
          end
        end
        puts Custom.new.freeze
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "custom\n");
}

#[test]
fn self_freeze_inside_a_method_freezes_the_receiver() {
    let result = run_ruby(
        r#"
        class Lockable
          def initialize
            @v = 1
          end
          def lock_it
            self.freeze
          end
          def set_v(n)
            @v = n
          end
          def v
            @v
          end
        end
        l = Lockable.new
        l.lock_it
        puts l.frozen?
        begin
          l.set_v(2)
        rescue FrozenError => e
          puts "frozen!"
        end
        puts l.v
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfrozen!\n1\n");
}

#[test]
fn freeze_and_frozen_predicate_work_on_a_poly_typed_receiver() {
    // A method parameter is always statically Poly -- the universal
    // `RubyValue::freeze_value`/`is_frozen` path, not the type-gated ones.
    let result = run_ruby(
        r#"
        class Once
          def run(x)
            x.freeze
            puts x.frozen?
          end
        end
        Once.new.run([1])
        Once.new.run(42)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\n");
}

// ---------------------------------------------------------------------------
// Phase 13.3: Fiber -- corosensei-backed stackful coroutines behind the
// spinel-fiber shim (see that crate's docs for the one quarantined unsafe
// block and its invariants). Every snippet oracle-verified against real
// `ruby` first; error messages are CRuby-verbatim (`cont.c`).
// ---------------------------------------------------------------------------

#[test]
fn fiber_yields_values_in_order_then_returns_the_body_value_and_dies() {
    let result = run_ruby(
        r#"
        f = Fiber.new do
          Fiber.yield 1
          Fiber.yield 2
          3
        end
        puts f.alive?
        puts f.resume
        puts f.resume
        puts f.resume
        puts f.alive?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\n1\n2\n3\nfalse\n");
}

#[test]
fn fiber_resume_and_yield_pass_values_in_both_directions() {
    // resume(v)'s v becomes the suspended Fiber.yield's return value;
    // Fiber.yield(v)'s v becomes resume's return value -- CRuby
    // `make_passing_arg` both ways.
    let result = run_ruby(
        r#"
        g = Fiber.new do |x|
          y = Fiber.yield(x + 1)
          z = Fiber.yield(y + 10)
          z + 100
        end
        puts g.resume(5)
        puts g.resume(6)
        puts g.resume(7)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n16\n107\n");
}

#[test]
fn first_resume_args_bind_to_the_fiber_blocks_params() {
    let result = run_ruby(
        r#"
        h = Fiber.new do |a, b|
          a + b
        end
        puts h.resume(3, 4)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\n");
}

#[test]
fn resuming_a_dead_fiber_raises_a_catchable_fiber_error() {
    let result = run_ruby(
        r#"
        d = Fiber.new { :done }
        d.resume
        begin
          d.resume
        rescue FiberError => e
          puts e.send(:message)
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "attempt to resume a terminated fiber\n");
}

#[test]
fn fiber_yield_at_the_root_raises_a_fiber_error() {
    let result = run_ruby(
        r#"
        begin
          Fiber.yield(1)
        rescue FiberError => e
          puts e.send(:message)
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "attempt to yield on a not resumed fiber\n");
}

#[test]
fn multiple_yield_args_pack_into_an_array_for_the_resumer() {
    // Asserted via `puts` (an Array prints one element per line, matching
    // real ruby) rather than `v.length`/`v[0]`: `resume`'s result is
    // statically Poly, and collection methods on a Poly value are the
    // PRE-existing Poly-dispatch gap, nothing fiber-specific.
    let result = run_ruby(
        r#"
        m = Fiber.new do
          Fiber.yield(1, 2)
          :fin
        end
        v = m.resume
        puts v
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n2\n");
}

#[test]
fn an_uncaught_exception_inside_a_fiber_reraises_at_the_resumer_and_kills_the_fiber() {
    let result = run_ruby(
        r#"
        x = Fiber.new do
          raise "boom in fiber"
        end
        begin
          x.resume
        rescue RuntimeError => e
          puts "caught: #{e.send(:message)}"
        end
        puts x.alive?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught: boom in fiber\nfalse\n");
}

#[test]
fn nested_fibers_each_yield_to_their_own_resumer() {
    // The exact shape the spinel-fiber TLS save/restore discipline exists
    // for: after the inner fiber yields, the OUTER fiber's own Fiber.yield
    // must suspend the outer one, not touch the inner's suspended yielder.
    // The inner fiber is CREATED outside the outer's block (captured via an
    // ordinary local) because a block literal escaping from inside another
    // escaping block is a PRE-existing Phase 6 scope-cut unrelated to
    // fibers; the nested block-LITERAL form is covered at the Rust level by
    // spinel-fiber's own `nested_fibers_yield_to_their_own_resumers` test.
    let result = run_ruby(
        r#"
        inner = Fiber.new do
          Fiber.yield :from_inner
          :inner_done
        end
        outer = Fiber.new do
          got = inner.resume
          Fiber.yield got
          inner.resume
        end
        puts outer.resume
        puts outer.resume
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "from_inner\ninner_done\n");
}

#[test]
fn a_fiber_body_captures_and_mutates_enclosing_locals() {
    // The fiber's block goes through the ordinary escaping-Proc capture
    // machinery (Phase 6's Arc<Mutex> cells), so shared mutation across
    // suspension points works exactly like any other escaping block.
    let result = run_ruby(
        r#"
        count = 0
        c = Fiber.new do
          count += 10
          Fiber.yield
          count += 100
        end
        c.resume
        puts count
        c.resume
        puts count
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n110\n");
}

// ---------------------------------------------------------------------------
// Phase 13.4: the whole top level runs as `may`'s first coroutine, with the
// worker count as the GVL switch (see `spinel_rt::run_main`'s docs). The
// REAL regression test for this change is every other test in this file --
// all of them now execute through the coroutine-wrapped main. These three
// only cover the configuration knobs themselves.
// ---------------------------------------------------------------------------

#[test]
fn scheduler_config_knobs_change_nothing_observable() {
    // The same fiber-exercising program (fibers being the most
    // execution-context-sensitive feature shipped so far) under the default
    // (workers=1, GVL-emulated), an explicit SPINEL_THREADS count, and
    // --no-gvl -- byte-identical output on all three.
    let src = r#"
        f = Fiber.new do |x|
          Fiber.yield(x + 1)
          :done
        end
        puts f.resume(1)
        puts f.resume
        puts f.alive?
    "#;
    let expected = "2\ndone\nfalse\n";
    let default = support::run_ruby(src);
    assert!(default.status.success(), "stderr: {}", default.stderr);
    assert_eq!(default.stdout, expected);

    let threads4 = support::run_ruby_configured(src, &[("SPINEL_THREADS", "4")], &[]);
    assert!(threads4.status.success(), "stderr: {}", threads4.stderr);
    assert_eq!(threads4.stdout, expected);

    let no_gvl = support::run_ruby_configured(src, &[], &["--no-gvl"]);
    assert!(no_gvl.status.success(), "stderr: {}", no_gvl.stderr);
    assert_eq!(no_gvl.stdout, expected);
}

#[test]
fn a_malformed_spinel_threads_value_fails_loudly_at_startup() {
    let result = support::run_ruby_configured(
        "puts 1\n",
        &[("SPINEL_THREADS", "not-a-number")],
        &[],
    );
    assert!(!result.status.success());
    assert!(
        result.stderr.contains("SPINEL_THREADS must be a positive integer"),
        "stderr: {}",
        result.stderr
    );
}

#[test]
fn uncaught_exceptions_still_exit_nonzero_through_the_coroutine_boundary() {
    // The top-level uncaught-raise contract (message on stderr, exit 1)
    // must survive the body now running inside a may coroutine and its
    // Result crossing a join back to the OS main thread.
    let result = run_ruby("raise \"through the boundary\"\n");
    assert!(!result.status.success());
    assert!(
        result.stderr.contains("uncaught exception: through the boundary"),
        "stderr: {}",
        result.stderr
    );
}

// ---------------------------------------------------------------------------
// Phase 13.5: Thread/Mutex/Queue over may's green coroutines (see
// spinel_rt::thread's docs, incl. the documented cooperative-scheduling
// divergence -- these tests only assert SYNCHRONIZED, deterministic
// outcomes). Every snippet oracle-verified against real `ruby`; error
// messages CRuby-verbatim.
// ---------------------------------------------------------------------------

#[test]
fn thread_join_waits_for_the_body_and_returns() {
    // Deterministic under the default single worker: the spawned coroutine
    // first runs when the spawner blocks at join. (Real preemptive ruby
    // agrees on this shape's ordering too -- oracle-verified.)
    let result = run_ruby(
        r#"
        t = Thread.new do
          puts "in thread"
        end
        t.join
        puts "after join"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "in thread\nafter join\n");
}

#[test]
fn thread_value_returns_the_block_result_and_args_bind_to_params() {
    let result = run_ruby(
        r#"
        v = Thread.new(20, 22) { |a, b| a + b }.value
        puts v
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn an_uncaught_exception_in_a_thread_reraises_at_join_and_value() {
    // CRuby stores the exception and re-raises it in whoever joins
    // (`thread.c:1195`). The no-join report_on_exception stderr warning is
    // a documented skip.
    let result = run_ruby(
        r#"
        bad = Thread.new { raise "thread boom" }
        begin
          bad.join
        rescue RuntimeError => e
          puts "joined error: #{e.send(:message)}"
        end
        bad2 = Thread.new { raise "thread boom2" }
        begin
          bad2.value
        rescue RuntimeError => e
          puts "valued error: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "joined error: thread boom\nvalued error: thread boom2\n");
}

#[test]
fn mutex_protected_counter_across_threads_is_exact() {
    // THE canonical threading idiom -- Proc-within-Proc (Thread.new wrapping
    // synchronize), only possible because Phase 13.5 lifted the nested-
    // escaping-block rejection. The sum is deterministic regardless of
    // interleaving; the e2e harness runs this under the default GVL mode,
    // and the same program was manually verified identical under
    // SPINEL_THREADS=4 (real parallelism).
    let result = run_ruby(
        r#"
        m = Mutex.new
        count = 0
        t1 = Thread.new { 1000.times { m.synchronize { count += 1 } } }
        t2 = Thread.new { 1000.times { m.synchronize { count += 1 } } }
        t1.join
        t2.join
        puts count
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2000\n");
}

#[test]
fn mutex_error_semantics_match_cruby() {
    let result = run_ruby(
        r#"
        mu = Mutex.new
        begin
          mu.unlock
        rescue ThreadError => e
          puts e.send(:message)
        end
        mu.lock
        puts mu.locked?
        puts mu.owned?
        begin
          mu.lock
        rescue ThreadError => e
          puts e.send(:message)
        end
        mu.unlock
        puts mu.locked?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Attempt to unlock a mutex which is not locked\ntrue\ntrue\ndeadlock; recursive locking\nfalse\n"
    );
}

#[test]
fn mutex_synchronize_returns_the_block_value_and_always_unlocks() {
    let result = run_ruby(
        r#"
        mu = Mutex.new
        r = mu.synchronize { 42 }
        puts r
        puts mu.locked?
        begin
          mu.synchronize { raise "inside" }
        rescue RuntimeError => e
          puts "rescued: #{e.send(:message)}"
        end
        puts mu.locked?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\nfalse\nrescued: inside\nfalse\n");
}

#[test]
fn queue_producer_consumer_rendezvous_with_close() {
    // pop blocks (coroutine-yielding) until a value or closure arrives; a
    // closed empty queue pops nil (`thread_sync.c:1034`).
    let result = run_ruby(
        r#"
        q = Queue.new
        producer = Thread.new do
          q.push 1
          q.push 2
          q.close
        end
        consumer = Thread.new do
          total = 0
          loop do
            v = q.pop
            break if v.nil?
            total += v
          end
          total
        end
        producer.join
        puts consumer.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n");
}

#[test]
fn push_to_a_closed_queue_raises_closed_queue_error() {
    let result = run_ruby(
        r#"
        qq = Queue.new
        qq.close
        puts qq.closed?
        begin
          qq.push 1
        rescue ClosedQueueError => e
          puts e.send(:message)
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nqueue closed\n");
}

#[test]
fn queue_length_shovel_and_empty_predicate() {
    let result = run_ruby(
        r#"
        q3 = Queue.new
        q3 << 5
        q3 << 6
        puts q3.length
        puts q3.empty?
        puts q3.pop
        puts q3.pop
        puts q3.empty?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\nfalse\n5\n6\ntrue\n");
}

#[test]
fn nil_predicate_works_universally() {
    // Added in this phase (surfaced by the queue-sentinel idiom): `.nil?`
    // on Poly, builtin, and Object receivers, with a user override winning.
    let result = run_ruby(
        r#"
        puts nil.nil?
        puts 1.nil?
        class Once
          def check(x)
            x.nil?
          end
        end
        puts Once.new.check(nil)
        puts Once.new.check(5)
        puts Once.new.nil?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\ntrue\nfalse\nfalse\n");
}

// ---------------------------------------------------------------------------
// Phase 13.6: the `$!`/HANDLING stack is may COROUTINE-local (not
// thread-local -- multiple Ruby Threads share one OS worker under the GVL
// default), and Fiber#resume swaps in each fiber's own saved stack, giving
// fibers the isolated execution context CRuby's per-fiber `saved_ec`
// provides. All snippets oracle-verified against real `ruby`.
// ---------------------------------------------------------------------------

#[test]
fn each_threads_bare_reraise_sees_its_own_handled_exception() {
    // With a thread_local! HANDLING stack this would cross-contaminate the
    // moment two Threads multiplex onto the one default worker.
    let result = run_ruby(
        r#"
        t1 = Thread.new do
          begin
            raise "from t1"
          rescue RuntimeError => e
            begin
              raise
            rescue RuntimeError => inner
              puts "t1 re-raised: #{inner.send(:message)}"
            end
          end
        end
        t1.join
        t2 = Thread.new do
          begin
            raise "from t2"
          rescue RuntimeError => e
            begin
              raise
            rescue RuntimeError => inner
              puts "t2 re-raised: #{inner.send(:message)}"
            end
          end
        end
        t2.join
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "t1 re-raised: from t1\nt2 re-raised: from t2\n");
}

#[test]
fn a_fiber_does_not_see_its_resumers_currently_handled_exception() {
    // CRuby: the fiber has its own errinfo (fresh, nil), so its bare
    // `raise` builds a fresh empty-message RuntimeError instead of
    // re-raising the resumer's in-flight exception -- `fiber saw: []`.
    let result = run_ruby(
        r#"
        f = Fiber.new do
          begin
            raise
          rescue RuntimeError => e
            "fiber saw: [#{e.send(:message)}]"
          end
        end
        begin
          raise "resumer's exception"
        rescue RuntimeError
          puts f.resume
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "fiber saw: []\n");
}

#[test]
fn a_fibers_rescue_state_survives_suspension_isolated_from_the_resumer() {
    // The fiber suspends MID-rescue; the resumer then handles (and
    // finishes handling) its own exception; on re-entry the fiber's bare
    // re-raise must still see the FIBER's exception -- the save/restore
    // swap around every switch, exercised in both directions.
    let result = run_ruby(
        r#"
        g = Fiber.new do
          begin
            raise "fiber's own"
          rescue RuntimeError
            Fiber.yield :suspended_mid_rescue
            begin
              raise
            rescue RuntimeError => again
              again.send(:message)
            end
          end
        end
        puts g.resume
        begin
          raise "resumer noise"
        rescue RuntimeError
          x = 1
        end
        puts g.resume
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "suspended_mid_rescue\nfiber's own\n");
}

// ---------------------------------------------------------------------------
// Phase 13.7: `send`'s missing-method fallback raises a real, catchable
// NoMethodError (via the factory generated main() installs) instead of the
// original whole-process `exit(1)` -- which would have killed every OTHER
// running Thread over one bad dispatch. Oracle-verified.
// ---------------------------------------------------------------------------

#[test]
fn a_missing_method_via_send_raises_a_rescuable_no_method_error() {
    let result = run_ruby(
        r#"
        class Plain
          def real
            :real
          end
        end
        p1 = Plain.new
        begin
          p1.send(:nope)
        rescue NoMethodError => e
          puts "caught nope"
        end
        begin
          p1.send(:nope2)
        rescue NameError => e
          puts "caught via NameError"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught nope\ncaught via NameError\n");
}

#[test]
fn an_uncaught_no_method_error_exits_via_the_ordinary_top_level_handler() {
    let result = run_ruby(
        r#"
        class Plain
        end
        Plain.new.send(:missing)
        "#,
    );
    assert!(!result.status.success());
    assert!(
        result.stderr.contains("uncaught exception: undefined method 'missing'"),
        "stderr: {}",
        result.stderr
    );
}

#[test]
fn one_threads_bad_dispatch_no_longer_kills_the_other_threads() {
    // THE motivating scenario for this phase: the failure surfaces at the
    // bad thread's own join; the healthy worker completes normally.
    let result = run_ruby(
        r#"
        class Plain
        end
        worker = Thread.new { 21 * 2 }
        bad = Thread.new { Plain.new.send(:missing_in_thread) }
        begin
          bad.join
        rescue NoMethodError => e
          puts "joined the failure"
        end
        puts worker.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "joined the failure\n42\n");
}

// ---------------------------------------------------------------------------
// Phase 13.8: Ractor -- real OS threads sharing the global heap, with the
// frozen-or-copy boundary discipline (see spinel_rt::ractor's docs, incl.
// the documented divergences: no Ractor::RemoteError wrapper, RactorError
// standing in for Ractor::Error, process-shared globals). Oracle-verified.
// ---------------------------------------------------------------------------

#[test]
fn ractor_runs_in_parallel_and_returns_its_value() {
    let result = run_ruby(
        r#"
        r = Ractor.new(20, 22) do |a, b|
          a + b
        end
        puts r.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn ractor_message_passing_send_and_receive() {
    let result = run_ruby(
        r#"
        worker = Ractor.new do
          msg = Ractor.receive
          msg * 10
        end
        worker.send(7)
        puts worker.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "70\n");
}

#[test]
fn ractor_shareable_tiering_and_make_shareable() {
    let result = run_ruby(
        r#"
        puts Ractor.shareable?(1)
        puts Ractor.shareable?(:sym)
        puts Ractor.shareable?("mutable")
        frozen_str = "frozen".freeze
        puts Ractor.shareable?(frozen_str)
        arr = [1, 2]
        puts Ractor.shareable?(arr)
        Ractor.make_shareable(arr)
        puts Ractor.shareable?(arr)
        puts arr.frozen?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\nfalse\ntrue\nfalse\ntrue\ntrue\n");
}

#[test]
fn an_unshareable_message_is_deep_copied_across_the_boundary() {
    // Mutating the original AFTER send must not affect the ractor's copy --
    // the whole point of the isolation discipline.
    // Asserted via `puts` on the round-tripped copy (collection methods on
    // the Poly-typed receive result are the pre-existing Poly-dispatch
    // gap, nothing Ractor-specific).
    let result = run_ruby(
        r#"
        echo = Ractor.new do
          Ractor.receive
        end
        payload = [100, 200]
        echo.send(payload)
        payload[0] = 999
        puts echo.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "100\n200\n");
}

#[test]
fn an_uncaught_exception_in_a_ractor_reraises_at_value() {
    // Documented divergence: the ORIGINAL exception re-raises directly
    // (like Thread#join), not wrapped in Ractor::RemoteError (which needs
    // nested class names + .cause chaining, both documented gaps).
    let result = run_ruby(
        r#"
        bad = Ractor.new do
          raise "ractor boom"
        end
        begin
          bad.value
        rescue RuntimeError => e
          puts "rescued: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "rescued: ractor boom\n");
}

#[test]
#[should_panic(expected = "can not isolate a Proc because it accesses outer variables (x)")]
fn a_ractor_block_capturing_an_outer_local_is_rejected_at_compile_time() {
    // CRuby raises Ractor::IsolationError at Proc-creation time; the AOT
    // compiler knows the capture set statically and rejects at COMPILE
    // time, with CRuby's own message wording.
    let _ = spinelc::compile_to_rust(
        r#"
        x = 5
        Ractor.new { x + 1 }
        "#,
    );
}

#[test]
fn an_unfrozen_object_sent_across_a_ractor_boundary_raises_ractor_error() {
    // Documented narrower-than-CRuby divergence: real Ruby deep-copies an
    // unfrozen object; this runtime has no by-name ivar-setting reflection
    // to rebuild one, so it raises a catchable RactorError instead.
    let result = run_ruby(
        r#"
        class Box
          def initialize(v)
            @v = v
          end
        end
        sink = Ractor.new do
          Ractor.receive
        end
        begin
          sink.send(Box.new(1))
        rescue RactorError => e
          puts "rejected: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert!(
        result.stdout.starts_with("rejected: an unfrozen Object can't cross a Ractor boundary"),
        "stdout: {}",
        result.stdout
    );
}

#[test]
fn a_deeply_frozen_object_crosses_a_ractor_boundary_by_reference() {
    let result = run_ruby(
        r#"
        class Box
          def initialize(v)
            @v = v
          end
          def v
            @v
          end
        end
        b = Box.new(41)
        Ractor.make_shareable(b)
        puts b.frozen?
        sink = Ractor.new do
          got = Ractor.receive
          got.send(:v)
        end
        sink.send(b)
        puts sink.value + 1
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\n42\n");
}

// ---------------------------------------------------------------------------
// Phase 13.9: the comprehensive cross-feature sweep. Every scenario was
// FIRST run as one combined program, oracle-verified byte-for-byte against
// real `ruby`, then confirmed byte-identical under all three scheduler
// modes (default GVL, SPINEL_THREADS=4, --no-gvl) and stable across
// repeated 8-worker runs. Individual tests below keep failures localized;
// the composite mode-invariance test at the end is the plan's headline
// "the toggle changes nothing observable" check.
// ---------------------------------------------------------------------------

#[test]
fn fiber_yield_from_a_deep_method_call_stack() {
    // The spinel-fiber shim's raison d'etre: suspension from inside a chain
    // of ordinary compiled method frames that never saw a yielder.
    let result = run_ruby(
        r#"
        class Chain
          def a; b; end
          def b; c; end
          def c
            Fiber.yield :from_deep
            :done
          end
        end
        ch = Chain.new
        f = Fiber.new { ch.a }
        puts f.resume
        puts f.resume
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "from_deep\ndone\n");
}

#[test]
fn fiber_yield_inside_an_inline_times_block() {
    let result = run_ruby(
        r#"
        f = Fiber.new do
          3.times do |i|
            Fiber.yield i
          end
          :end
        end
        puts f.resume
        puts f.resume
        puts f.resume
        puts f.resume
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "0\n1\n2\nend\n");
}

#[test]
fn fiber_yield_through_a_real_escaping_block() {
    // The hardest suspension shape: Fiber.yield fires inside a real Proc
    // (the block each_twice invokes), unwinding through the Proc's closure
    // frame AND each_twice's own method frame to the resumer.
    let result = run_ruby(
        r#"
        class Iter
          def each_twice
            yield 1
            yield 2
          end
        end
        it = Iter.new
        f = Fiber.new do
          it.each_twice do |n|
            Fiber.yield n
          end
          :done
        end
        puts f.resume
        puts f.resume
        puts f.resume
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n2\ndone\n");
}

#[test]
fn a_fiber_driven_entirely_inside_a_thread() {
    // The fiber table is per-OS-thread; a fiber created and resumed inside
    // one Thread's coroutine works because both operations run on the same
    // worker.
    let result = run_ruby(
        r#"
        t = Thread.new do
          ff = Fiber.new do |x|
            Fiber.yield x + 1
            99
          end
          a = ff.resume(1)
          b = ff.resume
          a + b
        end
        puts t.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "101\n");
}

#[test]
fn zero_arg_resume_binds_nil_block_params() {
    let result = run_ruby(
        r#"
        f = Fiber.new { |x| x.nil? }
        puts f.resume
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\n");
}

#[test]
fn an_exception_from_deep_inside_a_fibers_method_stack_reraises_at_the_resumer() {
    let result = run_ruby(
        r#"
        class Deep
          def go; boom; end
          def boom
            raise "deep boom"
          end
        end
        d = Deep.new
        f = Fiber.new { d.go }
        begin
          f.resume
        rescue RuntimeError => e
          puts "resumer caught: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "resumer caught: deep boom\n");
}

#[test]
fn a_fiber_rebinding_a_captured_local_propagates_to_the_enclosing_scope() {
    // From the reference project's own regression corpus
    // (fiber_reassign_capture.rb): REBINDING (not just mutating) a captured
    // name inside the fiber must write through the shared cell.
    let result = run_ruby(
        r#"
        s = "old"
        f = Fiber.new do
          s = "new"
          Fiber.yield
        end
        f.resume
        puts s
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "new\n");
}

#[test]
fn three_threads_join_out_of_creation_order() {
    let result = run_ruby(
        r#"
        t1 = Thread.new { 1 }
        t2 = Thread.new { 2 }
        t3 = Thread.new { 3 }
        puts t3.value + t1.value + t2.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n");
}

#[test]
fn mutex_synchronize_exits_with_a_break_value_and_unlocks() {
    let result = run_ruby(
        r#"
        mu = Mutex.new
        r = mu.synchronize { break 5 }
        puts r
        puts mu.locked?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\nfalse\n");
}

#[test]
fn a_blocked_consumer_is_woken_by_a_producer_thread() {
    // Under the default single worker the consumer runs first (at
    // main's `.value` yield), genuinely BLOCKS on the empty pop
    // (a coroutine-yielding Condvar wait), and is woken by the producer --
    // exercising the real wakeup path, deterministically.
    let result = run_ruby(
        r#"
        q = Queue.new
        consumer = Thread.new { q.pop }
        producer = Thread.new { q.push 42 }
        puts consumer.value
        producer.join
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn closing_a_queue_wakes_a_blocked_consumer_with_nil() {
    let result = run_ruby(
        r#"
        q = Queue.new
        consumer = Thread.new do
          v = q.pop
          if v.nil?
            :got_nil
          else
            :got_value
          end
        end
        closer = Thread.new { q.close }
        puts consumer.value
        closer.join
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "got_nil\n");
}

#[test]
fn multiple_producers_one_consumer_sum_is_exact() {
    let result = run_ruby(
        r#"
        q = Queue.new
        p1 = Thread.new { 10.times { q.push 1 } }
        p2 = Thread.new { 10.times { q.push 1 } }
        consumer = Thread.new do
          total = 0
          loop do
            v = q.pop
            break if v.nil?
            total += v
          end
          total
        end
        p1.join
        p2.join
        q.close
        puts consumer.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "20\n");
}

#[test]
fn globals_and_cvars_are_mutex_protectable_across_threads() {
    let result = run_ruby(
        r#"
        $total = 0
        class Counter
          @@n = 0
          def self.bump
            @@n = @@n + 1
          end
          def self.n
            @@n
          end
        end
        m = Mutex.new
        t1 = Thread.new { 100.times { m.synchronize { $total += 1 } } }
        t2 = Thread.new { 100.times { m.synchronize { Counter.bump } } }
        t1.join
        t2.join
        puts $total
        puts Counter.n
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "100\n100\n");
}

#[test]
fn begin_rescue_ensure_and_retry_work_inside_threads() {
    let result = run_ruby(
        r#"
        t1 = Thread.new do
          begin
            raise "in thread"
          rescue RuntimeError => e
            "rescued"
          ensure
            x = 1
          end
        end
        puts t1.value
        t2 = Thread.new do
          attempts = 0
          begin
            attempts += 1
            raise "flaky" if attempts < 3
            attempts
          rescue RuntimeError
            retry
          end
        end
        puts t2.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "rescued\n3\n");
}

#[test]
fn a_thread_and_the_fiber_it_resumes_have_isolated_handling() {
    // The plan's own listed 13.9 scenario: a Thread mid-rescue resumes a
    // fiber whose bare raise must see an EMPTY $!, not the thread's.
    let result = run_ruby(
        r#"
        t = Thread.new do
          f = Fiber.new do
            begin
              raise
            rescue RuntimeError => e
              "fiber saw: [#{e.send(:message)}]"
            end
          end
          begin
            raise "thread's exception"
          rescue RuntimeError
            f.resume
          end
        end
        puts t.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "fiber saw: []\n");
}

#[test]
fn no_method_error_propagates_out_of_fibers_and_ractors() {
    let result = run_ruby(
        r#"
        class Bare; end
        f = Fiber.new { Bare.new.send(:nope) }
        begin
          f.resume
        rescue NoMethodError
          puts "rescued in resumer"
        end
        puts f.alive?
        r = Ractor.new { Bare.new.send(:nope) }
        begin
          r.value
        rescue NoMethodError
          puts "rescued at value"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "rescued in resumer\nfalse\nrescued at value\n");
}

#[test]
fn make_shareable_deep_freezes_a_nested_object_graph_that_then_crosses() {
    // The plan's own listed scenario: object -> array -> object, every node
    // frozen by one make_shareable, the whole graph then crossing a Ractor
    // boundary by reference and dispatching on the far side.
    let result = run_ruby(
        r#"
        class Node
          def initialize(v, child)
            @v = v
            @child = child
          end
          def v; @v; end
        end
        leaf = Node.new(3, nil)
        root = Node.new(1, [leaf])
        Ractor.make_shareable(root)
        puts root.frozen?
        puts leaf.frozen?
        puts Ractor.shareable?(root)
        sink = Ractor.new do
          got = Ractor.receive
          got.send(:v)
        end
        sink.send(root)
        puts sink.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\ntrue\n1\n");
}

#[test]
fn several_ractors_run_in_parallel_and_all_values_collect() {
    let result = run_ruby(
        r#"
        r1 = Ractor.new(1) { |n| n * 10 }
        r2 = Ractor.new(2) { |n| n * 10 }
        r3 = Ractor.new(3) { |n| n * 10 }
        puts r1.value + r2.value + r3.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "60\n");
}

#[test]
fn ractor_messages_are_received_in_fifo_order() {
    let result = run_ruby(
        r#"
        r = Ractor.new do
          a = Ractor.receive
          b = Ractor.receive
          c = Ractor.receive
          a * 100 + b * 10 + c
        end
        r.send(1)
        r.send(2)
        r.send(3)
        puts r.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "123\n");
}

#[test]
fn a_fiber_works_inside_a_ractor() {
    // The fiber table is thread-pinned and a Ractor is its own OS thread --
    // create and drive entirely within it.
    let result = run_ruby(
        r#"
        r = Ractor.new do
          f = Fiber.new do
            Fiber.yield 1
            2
          end
          a = f.resume
          b = f.resume
          a + b
        end
        puts r.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n");
}

#[test]
fn the_shareable_predicate_is_deep() {
    // A frozen container holding an UNFROZEN element is not shareable --
    // CRuby's frozen-and-everything-reachable-shareable rule.
    let result = run_ruby(
        r#"
        arr = ["mut"]
        arr.freeze
        puts Ractor.shareable?(arr)
        Ractor.make_shareable(arr)
        puts Ractor.shareable?(arr)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "false\ntrue\n");
}

#[test]
fn a_frozen_error_raised_inside_a_thread_surfaces_at_join() {
    let result = run_ruby(
        r#"
        a = [1]
        a.freeze
        t = Thread.new { a[0] = 9 }
        begin
          t.join
        rescue FrozenError => e
          puts "at join: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "at join: can't modify frozen Array: [1]\n");
}

#[test]
fn freezing_in_one_thread_is_visible_after_join() {
    // Threads share the heap (no Ractor boundary): a freeze in one is the
    // same AtomicBool every other execution context reads.
    let result = run_ruby(
        r#"
        s = "x"
        t = Thread.new { s.freeze }
        t.join
        puts s.frozen?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\n");
}

#[test]
fn index_compound_assignment_works_inside_a_thread_block() {
    // From the reference corpus (index_opassign_in_thread_block.rb): the
    // `Seq` desugar's hidden temps declared inside an escaping Proc's own
    // prelude, against a captured-cell array.
    let result = run_ruby(
        r#"
        arr = [10]
        t = Thread.new do
          50.times { arr[0] += 1 }
        end
        t.join
        puts arr[0]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "60\n");
}

#[test]
fn the_concurrency_composite_is_invariant_across_scheduler_modes() {
    // The plan's headline 13.9 check: a well-synchronized program mixing
    // Threads, a Mutex counter, a Queue rendezvous, and parallel Ractors
    // produces byte-identical output under the GVL default, an explicit
    // worker count, and --no-gvl.
    let src = r#"
        m = Mutex.new
        count = 0
        t1 = Thread.new { 200.times { m.synchronize { count += 1 } } }
        t2 = Thread.new { 200.times { m.synchronize { count += 1 } } }
        q = Queue.new
        consumer = Thread.new do
          total = 0
          loop do
            v = q.pop
            break if v.nil?
            total += v
          end
          total
        end
        producer = Thread.new do
          25.times { |i| q.push i }
          q.close
        end
        r1 = Ractor.new(5) { |n| n * n }
        r2 = Ractor.new(6) { |n| n * n }
        t1.join
        t2.join
        producer.join
        puts count
        puts consumer.value
        puts r1.value + r2.value
    "#;
    let expected = "400\n300\n61\n";
    let default = support::run_ruby(src);
    assert!(default.status.success(), "stderr: {}", default.stderr);
    assert_eq!(default.stdout, expected);
    let threads4 = support::run_ruby_configured(src, &[("SPINEL_THREADS", "4")], &[]);
    assert!(threads4.status.success(), "stderr: {}", threads4.stderr);
    assert_eq!(threads4.stdout, expected);
    let no_gvl = support::run_ruby_configured(src, &[], &["--no-gvl"]);
    assert!(no_gvl.status.success(), "stderr: {}", no_gvl.stderr);
    assert_eq!(no_gvl.stdout, expected);
}

// ---- Phase 14.1: require/require_relative/load compile-time splicing ----
// Every positive-path expectation below was oracle-verified against real
// `ruby` (4.0.5) first, per this project's standing convention.

#[test]
fn require_relative_splices_in_document_order_and_shares_the_global_namespace() {
    let result = support::run_ruby_project(
        &[
            // Top-level `def` in ANY file (main or required) is a separate,
            // pre-existing gap ("unexpected top-level-only node"), routed
            // around with classes/module functions here exactly like every
            // prior phase's tests.
            (
                "greeter.rb",
                r##"
                    GREETING = "hello"
                    class Greeter
                      def greet(name)
                        "#{GREETING}, #{name}!"
                      end
                    end
                    puts "greeter loaded"
                "##,
            ),
            (
                "main.rb",
                r#"
                    puts "before require"
                    require_relative "greeter"
                    puts "after require"
                    g = Greeter.new
                    puts g.greet("world")
                    puts GREETING
                "#,
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "before require\ngreeter loaded\nafter require\nhello, world!\nhello\n"
    );
}

#[test]
fn diamond_requires_execute_the_shared_file_exactly_once() {
    let result = support::run_ruby_project(
        &[
            ("shared.rb", "puts \"shared executed\"\nSHARED = 7\n"),
            (
                "liba.rb",
                "require_relative \"shared\"\nputs \"liba loaded\"\n",
            ),
            (
                "libb.rb",
                "require_relative \"shared\"\nputs \"libb loaded\"\n",
            ),
            (
                "main.rb",
                r#"
                    require_relative "liba"
                    require_relative "libb"
                    require_relative "shared"
                    puts SHARED
                "#,
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "shared executed\nliba loaded\nlibb loaded\n7\n"
    );
}

#[test]
fn circular_requires_compose_in_rubys_execution_order() {
    // CRuby registers a feature as loading BEFORE executing it, so the
    // inner require of an in-progress file is a no-op and execution order
    // is ca-start, all of cb, ca-end -- the dedup-before-lowering rule
    // reproduces this exactly.
    let result = support::run_ruby_project(
        &[
            (
                "ca.rb",
                "puts \"ca start\"\nrequire_relative \"cb\"\nputs \"ca end\"\n",
            ),
            (
                "cb.rb",
                "puts \"cb start\"\nrequire_relative \"ca\"\nputs \"cb end\"\n",
            ),
            (
                "main.rb",
                "require_relative \"ca\"\nputs \"main done\"\n",
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "ca start\ncb start\ncb end\nca end\nmain done\n"
    );
}

#[test]
fn plain_require_resolves_against_search_roots_including_nested_features() {
    let result = support::run_ruby_project(
        &[
            (
                "vendorlib/util.rb",
                r#"
                    require "util/strings"
                    UTIL = "util root"
                    puts "util loaded"
                "#,
            ),
            (
                "vendorlib/util/strings.rb",
                r#"
                    module Spacer
                      def self.doubled(n)
                        n * 2
                      end
                    end
                    puts "util/strings loaded"
                "#,
            ),
            (
                "main.rb",
                r#"
                    require "util"
                    puts UTIL
                    puts Spacer.doubled(21)
                "#,
            ),
        ],
        "main.rb",
        &["vendorlib"],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "util/strings loaded\nutil loaded\nutil root\n42\n"
    );
}

#[test]
fn a_required_files_top_level_locals_are_isolated_from_the_main_file() {
    // Real Ruby gives every file its own top-level local scope: main's
    // `count` and the lib's `count` (mutated through a block, exercising
    // the rename pass inside shared-scope block bodies) never touch. The
    // lib hands its result out through a global -- the only channel real
    // Ruby shares.
    let result = support::run_ruby_project(
        &[
            (
                "counterlib.rb",
                r#"
                    count = 100
                    3.times do
                      count += 1
                    end
                    $lib_count = count
                    class CounterBox
                      def initialize
                        @n = 0
                      end
                      def bump
                        @n += 1
                      end
                      def n
                        @n
                      end
                    end
                "#,
            ),
            (
                "main.rb",
                r#"
                    count = 5
                    require_relative "counterlib"
                    puts count
                    puts $lib_count
                    b = CounterBox.new
                    b.bump
                    b.bump
                    puts b.n
                "#,
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\n103\n2\n");
}

#[test]
fn load_reexecutes_every_time_with_fresh_locals_and_never_registers_the_feature() {
    // Three executions: two `load`s plus a `require` -- load never adds to
    // the feature table in real Ruby, so the require still fires. `ticks`
    // restarts at 0 each execution (fresh local scope per load), which the
    // per-splice-instance gensym reproduces.
    let result = support::run_ruby_project(
        &[
            (
                "tick.rb",
                r#"
                    ticks = 0
                    ticks += 1
                    $total = ($total || 0) + ticks
                    puts "tick! total=#{$total}"
                "#,
            ),
            (
                "main.rb",
                r#"
                    load "./tick.rb"
                    load "./tick.rb"
                    require_relative "tick"
                    puts "end total=#{$total}"
                "#,
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "tick! total=1\ntick! total=2\ntick! total=3\nend total=3\n"
    );
}

#[test]
fn missing_require_is_a_compile_error_with_crubys_message() {
    let err = support::compile_project(
        &[("main.rb", "require \"definitely_missing\"\n")],
        "main.rb",
        &[],
    )
    .unwrap_err();
    assert!(
        err.contains("cannot load such file -- definitely_missing"),
        "unexpected error: {err}"
    );
}

#[test]
fn missing_require_relative_reports_the_absolutized_path() {
    let err = support::compile_project(
        &[("main.rb", "require_relative \"nope\"\n")],
        "main.rb",
        &[],
    )
    .unwrap_err();
    assert!(
        err.contains("cannot load such file -- ") && err.contains("nope.rb"),
        "unexpected error: {err}"
    );
}

#[test]
fn non_literal_and_non_top_level_requires_are_clean_compile_errors() {
    let err = support::compile_project(
        &[("main.rb", "name = \"x\"\nrequire name\n")],
        "main.rb",
        &[],
    )
    .unwrap_err();
    assert!(err.contains("non-literal"), "unexpected error: {err}");

    // Inside a method body -- reaches lower_node's rejection.
    let err = spinelc::compile_to_rust("def m\n  require \"x\"\nend\n").unwrap_err();
    assert!(
        err.contains("only supported as a top-level statement"),
        "unexpected error: {err}"
    );

    // Inside begin/rescue -- the optional-dependency idiom is a LOUD
    // compile error under compile-time resolution, never a silent skip.
    let err = spinelc::compile_to_rust(
        "begin\n  require \"optional_dep\"\nrescue LoadError\nend\n",
    )
    .unwrap_err();
    assert!(
        err.contains("only supported as a top-level statement"),
        "unexpected error: {err}"
    );
}

#[test]
fn pathless_require_relative_cannot_infer_basepath() {
    // `compile_to_rust` (no input path) mirrors CRuby's eval/irb context:
    // require_relative has no requiring-file directory to resolve against.
    let err = spinelc::compile_to_rust("require_relative \"x\"\n").unwrap_err();
    assert!(err.contains("cannot infer basepath"), "unexpected error: {err}");
}

#[test]
fn autoload_and_load_wrap_are_clean_rejections() {
    let err = spinelc::compile_to_rust("autoload :Foo, \"foo\"\n").unwrap_err();
    assert!(err.contains("autoload"), "unexpected error: {err}");

    let err = support::compile_project(
        &[
            ("w.rb", "puts 1\n"),
            ("main.rb", "load \"./w.rb\", true\n"),
        ],
        "main.rb",
        &[],
    )
    .unwrap_err();
    assert!(err.contains("wrap"), "unexpected error: {err}");
}

#[test]
fn load_cycles_are_detected_at_compile_time() {
    // Real Ruby would recurse forever at runtime (load has no dedup); a
    // compile-time resolver rejects the cycle loudly instead.
    let err = support::compile_project(
        &[("selfload.rb", "load \"./selfload.rb\"\n"), ("main.rb", "load \"./selfload.rb\"\n")],
        "main.rb",
        &[],
    )
    .unwrap_err();
    assert!(err.contains("cycle"), "unexpected error: {err}");
}

// ---- Phase 14.2: spin.toml packages + search-path resolution ----
// Positive-path output oracle-verified by simulating package roots with
// real `ruby -I <pkg>/lib` (the package layer IS just ordered roots).

#[test]
fn packages_resolve_with_nested_features_and_cross_package_requires() {
    let result = support::run_ruby_packages(
        &[
            (
                "packages/greet/spin.toml",
                "[package]\nname = \"greet\"\n",
            ),
            (
                "packages/greet/lib/greet.rb",
                r##"
                    require "greet/upper"
                    require "farewell"
                    module Greet
                      def self.hi(name)
                        Upper.dashed("hi, #{name}")
                      end
                    end
                    puts "greet loaded"
                "##,
            ),
            (
                "packages/greet/lib/greet/upper.rb",
                r##"
                    module Upper
                      def self.dashed(s)
                        "-- #{s} --"
                      end
                    end
                    puts "upper loaded"
                "##,
            ),
            (
                "packages/farewell/spin.toml",
                "[package]\nname = \"farewell\"\n",
            ),
            (
                "packages/farewell/lib/farewell.rb",
                r#"
                    module Farewell
                      def self.bye
                        "bye!"
                      end
                    end
                    puts "farewell loaded"
                "#,
            ),
            (
                "main.rb",
                r#"
                    require "greet"
                    puts Greet.hi("ann")
                    puts Farewell.bye
                "#,
            ),
        ],
        "main.rb",
        &[],
        &["packages"],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "upper loaded\nfarewell loaded\ngreet loaded\n-- hi, ann --\nbye!\n"
    );
}

#[test]
fn a_package_can_override_require_paths_reference_style_flat_layout() {
    let result = support::run_ruby_packages(
        &[
            (
                "packages/flat/spin.toml",
                "[package]\nname = \"flat\"\nrequire_paths = [\".\"]\n",
            ),
            ("packages/flat/flat.rb", "FLAT = \"flat pkg\"\n"),
            ("main.rb", "require \"flat\"\nputs FLAT\n"),
        ],
        "main.rb",
        &[],
        &["packages"],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "flat pkg\n");
}

#[test]
fn dash_i_roots_shadow_packages_and_earlier_package_dirs_shadow_later_ones() {
    // CRuby's own ordering: -I beats even default gems; and one package
    // NAME resolves to exactly one package, nearest package-dir first.
    let result = support::run_ruby_packages(
        &[
            ("override/dual.rb", "puts \"from -I root\"\n"),
            (
                "projpkgs/thing/spin.toml",
                "[package]\nname = \"thing\"\n",
            ),
            (
                "projpkgs/thing/lib/thing.rb",
                "puts \"thing from projpkgs\"\n",
            ),
            (
                "bundledpkgs/thing/spin.toml",
                "[package]\nname = \"thing\"\n",
            ),
            (
                "bundledpkgs/thing/lib/thing.rb",
                "puts \"thing from bundledpkgs\"\n",
            ),
            (
                "bundledpkgs/dual/spin.toml",
                "[package]\nname = \"dual\"\n",
            ),
            (
                "bundledpkgs/dual/lib/dual.rb",
                "puts \"dual from package\"\n",
            ),
            (
                "main.rb",
                "require \"dual\"\nrequire \"thing\"\n",
            ),
        ],
        "main.rb",
        &["override"],
        &["projpkgs", "bundledpkgs"],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "from -I root\nthing from projpkgs\n");
}

#[test]
fn a_feature_provided_by_two_packages_is_a_loud_ambiguity_error() {
    // Mirrors RubyGems' own `Gem::LoadError "found in multiple gems"` --
    // stricter than silent $LOAD_PATH-order shadowing.
    let err = support::compile_packages(
        &[
            ("packages/alpha/spin.toml", "[package]\nname = \"alpha\"\n"),
            ("packages/alpha/lib/common.rb", "puts 1\n"),
            ("packages/beta/spin.toml", "[package]\nname = \"beta\"\n"),
            ("packages/beta/lib/common.rb", "puts 2\n"),
            ("main.rb", "require \"common\"\n"),
        ],
        "main.rb",
        &[],
        &["packages"],
    )
    .unwrap_err();
    assert!(
        err.contains("found in multiple packages") && err.contains("alpha") && err.contains("beta"),
        "unexpected error: {err}"
    );
}

#[test]
fn bad_manifests_are_loud_configuration_errors() {
    // Name/directory mismatch.
    let err = support::compile_packages(
        &[
            ("packages/aaa/spin.toml", "[package]\nname = \"bbb\"\n"),
            ("packages/aaa/lib/aaa.rb", "puts 1\n"),
            ("main.rb", "puts :ok\n"),
        ],
        "main.rb",
        &[],
        &["packages"],
    )
    .unwrap_err();
    assert!(err.contains("doesn't match its directory name"), "unexpected error: {err}");

    // Missing [package] name.
    let err = support::compile_packages(
        &[
            ("packages/aaa/spin.toml", "[package]\n"),
            ("main.rb", "puts :ok\n"),
        ],
        "main.rb",
        &[],
        &["packages"],
    )
    .unwrap_err();
    assert!(err.contains("needs a string `name`"), "unexpected error: {err}");

    // Default require_paths (["lib"]) pointing at a missing lib/.
    let err = support::compile_packages(
        &[
            ("packages/aaa/spin.toml", "[package]\nname = \"aaa\"\n"),
            ("packages/aaa/aaa.rb", "puts 1\n"),
            ("main.rb", "puts :ok\n"),
        ],
        "main.rb",
        &[],
        &["packages"],
    )
    .unwrap_err();
    assert!(err.contains("doesn't exist under"), "unexpected error: {err}");
}

#[test]
fn a_directory_without_a_manifest_is_not_a_package() {
    // No spin.toml -> ignored entirely; the feature is simply not found.
    let err = support::compile_packages(
        &[
            ("packages/plain/lib/plain.rb", "puts 1\n"),
            ("main.rb", "require \"plain\"\n"),
        ],
        "main.rb",
        &[],
        &["packages"],
    )
    .unwrap_err();
    assert!(
        err.contains("cannot load such file -- plain"),
        "unexpected error: {err}"
    );
}

#[test]
fn a_module_function_can_call_another_modules_function() {
    // Pre-existing gap found by 14.2's cross-package test: module-function
    // bodies live inside a generated `pub mod` (a real child module), so a
    // reference to a sibling top-level module didn't resolve. One file, no
    // packages needed.
    let result = run_ruby(
        r#"
            module A
              def self.f
                41
              end
            end
            module B
              def self.g
                A.f + 1
              end
            end
            puts B.g
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

// ---- Phase 14.3: base64 native-Rust package ----

/// The compiler repo's own bundled packages/ dir, as the test-project
/// harness's package-dir argument (absolute, so the temp-dir join is a
/// no-op replacement).
const REPO_PACKAGES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packages");

#[test]
fn base64_package_matches_real_ruby() {
    // Oracle: real ruby's own bundled base64 gem (all expectations below
    // are its literal outputs, incl. encode64's 60-char line wrapping).
    let result = support::run_ruby_packages(
        &[(
            "main.rb",
            r#"
                require "base64"
                puts Base64.encode64("hello world")
                puts Base64.encode64("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                puts Base64.strict_encode64("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                puts Base64.decode64(Base64.encode64("round trip"))
                puts Base64.urlsafe_encode64("ab")
                puts Base64.urlsafe_decode64("YWI")
                puts Base64.strict_decode64("aGVsbG8=")
            "#,
        )],
        "main.rb",
        &[],
        &[REPO_PACKAGES],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "aGVsbG8gd29ybGQ=\n\
         YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFh\nYWFhYWE=\n\
         YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWE=\n\
         round trip\n\
         YWI=\n\
         ab\n\
         hello\n"
    );
}

#[test]
fn native_crate_is_linked_only_when_its_package_is_required() {
    // The reference project's ".o rule": the [native] crate appears in the
    // link manifest iff the require actually fired.
    let (with_require, dir) = support::compile_packages(
        &[("main.rb", "require \"base64\"\nputs Base64.strict_encode64(\"x\")\n")],
        "main.rb",
        &[],
        &[REPO_PACKAGES],
    )
    .expect("compiles");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(with_require.native_deps, vec!["spinelc-base64".to_string()]);

    let (without_require, dir) = support::compile_packages(
        &[("main.rb", "puts :no_base64\n")],
        "main.rb",
        &[],
        &[REPO_PACKAGES],
    )
    .expect("compiles");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(without_require.native_deps.is_empty());
}

#[test]
#[should_panic(expected = "wrong number of arguments for native `Base64.encode64`")]
fn native_func_arity_is_checked_at_compile_time() {
    let _ = support::compile_packages(
        &[("main.rb", "require \"base64\"\nputs Base64.encode64(\"a\", \"b\")\n")],
        "main.rb",
        &[],
        &[REPO_PACKAGES],
    );
}

#[test]
fn native_dsl_misuse_is_a_clean_compile_error() {
    // native_func without a native_crate naming the backing crate.
    let err = spinelc::compile_to_rust(
        "module M\n  native_func :f, [String], String\nend\n",
    )
    .unwrap_err();
    assert!(err.contains("no `native_crate`"), "unexpected error: {err}");

    // Malformed native_func shapes.
    let err = spinelc::compile_to_rust("module M\n  native_func :f\nend\n").unwrap_err();
    assert!(err.contains("`native_func` takes"), "unexpected error: {err}");
    let err = spinelc::compile_to_rust("module M\n  native_crate :not_a_string\nend\n").unwrap_err();
    assert!(err.contains("`native_crate` takes"), "unexpected error: {err}");
}

// ---- Phase 14.4: the set pure-Ruby package + the dispatch fixes it forced ----

#[test]
fn set_package_matches_real_rubys_core_set() {
    // The flagship acceptance: every expectation below is real ruby's core
    // Set's literal output for the same program (oracle-verified) --
    // exercising include+MRO through a package, &blk forwarding through
    // nested escaping Procs, operator-method definitions, and the dynamic
    // builtin dispatch layer end to end.
    let result = support::run_ruby_packages(
        &[(
            "main.rb",
            r#"
                require "set"
                s = Set.new([1, 2, 3, 2, 1])
                puts s.size
                puts s.include?(2)
                s.add(4)
                s << 5
                s.delete(3)
                puts s.to_a.length
                puts s.member?(3)
                a = Set.new([1, 2, 3])
                b = Set.new([3, 4])
                puts (a | b).size
                puts (a & b).to_a.length
                puts (a - b).size
                puts (a ^ b).size
                puts a.subset?(Set.new([1, 2, 3, 9]))
                puts Set.new([3, 2, 1]) == Set.new([1, 2, 3])
                puts a == b
                doubled = a.map { |x| x * 2 }
                puts doubled.length
                puts a.select { |x| x > 1 }.length
                puts a.any? { |x| x > 2 }
                puts a.all? { |x| x > 0 }
                puts a.count
                puts Set.new.empty?
                copy = Set.new(a)
                puts copy.size
            "#,
        )],
        "main.rb",
        &[],
        &[REPO_PACKAGES],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "3\ntrue\n4\nfalse\n4\n1\n2\n3\ntrue\ntrue\nfalse\n3\n2\ntrue\ntrue\n3\ntrue\n3\n"
    );
}

#[test]
fn a_param_referenced_only_inside_an_escaping_block_is_captured() {
    // The Phase 14.4 capture fix: `n` (a method param) appears ONLY inside
    // the escaping block -- previously misclassified as a block-own local
    // and silently re-declared Nil. Oracle: 11, 12.
    let result = run_ruby(
        r#"
            class Pair
              def each(&blk)
                blk.call(1)
                blk.call(2)
                self
              end
              def add_all(n)
                each { |x| puts x + n }
              end
              def mapped(&blk)
                result = []
                each { |x| result << blk.call(x) }
                result
              end
            end
            Pair.new.add_all(10)
            puts Pair.new.mapped { |x| x * 3 }.length
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "11\n12\n2\n");
}

#[test]
fn builtin_receivers_dispatch_dynamically() {
    // send_value's builtin table (oracle-verified): Array#each on a
    // literal, Hash#each, send(:length) on an Array, hash/array ops on a
    // Poly ivar, and a rescuable NoMethodError from a builtin receiver.
    let result = support::run_ruby_packages(
        &[(
            "main.rb",
            r##"
                [10, 20, 30].each { |x| puts x }
                { a: 1, b: 2 }.each { |k, v| puts "#{k}=#{v}" }
                puts [1, 2].send(:length)
                class Box
                  def initialize
                    @hash = {}
                    @items = []
                  end
                  def put(k, v)
                    @hash[k] = v
                    @items << k
                    self
                  end
                  def get(k)
                    @hash[k]
                  end
                  def order
                    @items
                  end
                end
                b = Box.new
                b.put(:x, 1).put(:y, 2)
                puts b.get(:y)
                puts b.order.length
                begin
                  [1, 2].no_such_method
                rescue NoMethodError
                  puts "caught NoMethodError"
                end
            "##,
        )],
        "main.rb",
        &[],
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "10\n20\n30\na=1\nb=2\n2\n2\n2\ncaught NoMethodError\n"
    );
}

#[test]
fn operators_on_dynamic_operands_fall_through_to_dispatch() {
    // The old "isn't supported yet for non-Int/Float operands" panic is now
    // dynamic dispatch: String#+ via send_value's table, and a user class's
    // own operator method chained through a Poly intermediate (`a << 1`
    // returns self as Poly; the second `<<` dispatches dynamically).
    let result = run_ruby(
        r#"
            module Cat
              def self.concat2(a, b)
                a + b
              end
            end
            puts Cat.concat2("foo", "bar")
            class Acc
              def initialize
                @items = []
              end
              def <<(item)
                @items << item
                self
              end
              def size
                @items.length
              end
            end
            a = Acc.new
            a << 1 << 2 << 3
            puts a.size
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "foobar\n3\n");
}

#[test]
fn new_binds_optional_initialize_params() {
    // The emit_new fix: one `initialize(items = nil)` serving both
    // `Bag.new` and `Bag.new([1])` (oracle: 0, 1).
    let result = run_ruby(
        r#"
            class Bag
              def initialize(items = nil)
                @n = items.nil? ? 0 : 1
              end
              def n
                @n
              end
            end
            puts Bag.new.n
            puts Bag.new([1]).n
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "0\n1\n");
}

#[test]
#[should_panic(expected = "wrong number of arguments for `Bag.new`")]
fn new_arity_is_still_checked() {
    let _ = spinelc::compile_to_rust(
        "class Bag\n  def initialize(a, b = 1)\n  end\nend\nBag.new(1, 2, 3)\n",
    );
}

// ---- Phase 14.4 rev.2: Enumerable implemented in Rust (spinel_rt::enumerable) ----

#[test]
fn rust_enumerable_matches_real_ruby_across_all_receiver_kinds() {
    // One oracle-verified sweep (real ruby 4.0.5, byte-for-byte) covering
    // the Rust Enumerable against every receiver kind: Array/Hash/Range
    // literals (the builtin `include Enumerable` set), and a user class
    // (Set) reached through `send`'s ancestor-checked fallback -- plus
    // reduce's all three CRuby forms, empty-collection edge cases
    // ([].all? true, [].reduce nil), each_with_index's (elem, index)
    // 2-arg yield + returns-self, count's size-vs-block split, min/max
    // seeding, sum's Int->Float ladder, and is_a?(Enumerable).
    let result = support::run_ruby_packages(
        &[(
            "main.rb",
            r##"
                require "set"
                puts [1, 2, 3, 4].map { |x| x * x }.to_a.length
                puts [1, 2, 3, 4].select { |x| x > 2 }.first
                puts [1, 2, 3].reduce { |a, b| a + b }
                puts [1, 2, 3].reduce(10) { |a, b| a + b }
                puts [1, 2, 3].reduce(:+)
                puts [1, 2, 3].reduce(100, :+)
                puts [].reduce { |a, b| a + b }.nil?
                puts [1, 2, 3].sum
                puts [1, 2.5].sum
                puts [5, 1, 9].min
                puts [5, 1, 9].max
                puts ["b", "a", "c"].max
                puts [1, 2, 3].find { |x| x > 1 }
                puts [1, 2, 3].first
                puts [1, 2, 3].first(2).length
                puts [1, 2, 3].count { |x| x > 1 }
                puts [1, 2, 2, 3].count(2)
                puts [].all?
                puts [].any?
                puts [].none?
                puts [nil, false].any?
                puts [1, false].one?
                [10, 20].each_with_index { |v, i| puts "#{i}:#{v}" }
                puts({ a: 1, b: 2 }.map { |k, v| v }.sum)
                puts({ a: 1, b: 2 }.to_a.length)
                puts({ a: 1 }.any?)
                puts (1..4).to_a.length
                puts (1...4).to_a.length
                puts (1..10).select { |x| x > 7 }.length
                puts (1..5).reduce(:+)
                puts (1..5).include?(3)
                s = Set.new([4, 5, 6])
                puts s.find { |x| x > 4 }
                puts s.reduce(:+)
                puts s.first
                puts s.each_with_index { |v, i| }.size
                puts s.min
                puts s.max
                puts [].is_a?(Enumerable)
                puts({}.is_a?(Enumerable))
                puts (1..2).is_a?(Enumerable)
                puts s.is_a?(Enumerable)
                puts 5.is_a?(Enumerable)
            "##,
        )],
        "main.rb",
        &[],
        &[REPO_PACKAGES],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "4\n3\n6\n16\n6\n106\ntrue\n6\n3.5\n1\n9\nc\n2\n1\n2\n2\n2\ntrue\nfalse\ntrue\nfalse\ntrue\n0:10\n1:20\n3\n2\ntrue\n4\n3\n3\n15\ntrue\n5\n15\n4\n3\n4\n6\ntrue\ntrue\ntrue\ntrue\nfalse\n"
    );
}

#[test]
fn blockless_forms_return_real_enumerators() {
    // The Phase 14.4 "no Enumerator (spike scope)" posture retired by
    // Phase 17.2: a blockless map returns a real Enumerator whose `each`
    // re-invokes the captured method. Oracle-verified.
    let result = run_ruby(
        "e = [1, 2].map\n\
         p e.class\n\
         p e\n\
         p e.each { |x| x * 3 }\n\
         p e.size\n",
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Enumerator\n#<Enumerator: [1, 2]:map>\n[3, 6]\n2\n"
    );
}

#[test]
fn redefining_enumerable_in_ruby_is_rejected() {
    // Enumerable is a RUST-implemented builtin now (the whole point of
    // rev.2) -- and builtin MODULES stayed un-reopenable through Phase 16.3
    // (patching one would need re-materialization onto every includer,
    // including the Rust-backed builtin fallbacks).
    let err = spinelc::compile_to_rust("module Enumerable\n  def map\n  end\nend\n").unwrap_err();
    assert!(
        err.contains("built-in module `Enumerable`"),
        "unexpected error: {err}"
    );
}

// -- Phase 15.2: correctness fixes (reopening, bare-super forwarding, cycle
// guards, dup/clone). Every positive expectation below is oracle-verified
// against real ruby 4.0.5.

#[test]
fn reopening_a_user_class_adds_and_replaces_methods() {
    // Pre-15.2 this was the codebase's one SILENT-wrongness bug: the second
    // `class Foo` registered a shadowed duplicate, so `b` never dispatched
    // and the original `a` kept winning.
    let result = support::run_ruby(
        r#"
        class Foo
          def a
            "first"
          end
        end

        class Foo
          def b
            "added"
          end

          def a
            "replaced"
          end
        end

        puts Foo.new.a
        puts Foo.new.b
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "replaced\nadded\n");
}

#[test]
fn redefining_a_method_in_one_class_body_last_def_wins() {
    let result = support::run_ruby(
        r#"
        class Foo
          def a
            "first"
          end

          def a
            "second"
          end
        end

        puts Foo.new.a
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "second\n");
}

#[test]
fn reopening_via_load_twice_merges_cleanly() {
    let result = support::run_ruby_project(
        &[
            (
                "counter.rb",
                r#"
                    class Counter
                      def bump
                        1
                      end
                    end
                "#,
            ),
            (
                "main.rb",
                r#"
                    load "./counter.rb"
                    load "./counter.rb"
                    puts Counter.new.bump
                "#,
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n");
}

#[test]
fn reopen_guards_mirror_rubys_type_errors() {
    // `TypeError: superclass mismatch for class Sub` in real Ruby.
    let err = spinelc::compile_to_rust(
        "class Base\nend\nclass Other\nend\nclass Sub < Base\nend\nclass Sub < Other\nend\n",
    )
    .unwrap_err();
    assert!(
        err.contains("superclass mismatch for class Sub"),
        "unexpected error: {err}"
    );

    // Restating the ORIGINAL superclass is allowed (real Ruby).
    let result = support::run_ruby(
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

    // `TypeError: Foo is not a module` / `Bar is not a class` in real Ruby.
    let err = spinelc::compile_to_rust("class Foo\nend\nmodule Foo\nend\n").unwrap_err();
    assert!(err.contains("Foo is not a module"), "unexpected error: {err}");
    let err = spinelc::compile_to_rust("module Bar\nend\nclass Bar\nend\n").unwrap_err();
    assert!(err.contains("Bar is not a class"), "unexpected error: {err}");
}

#[test]
fn bare_super_forwards_current_arguments() {
    // Forwards the CURRENT bindings (the reassigned `name`), through
    // required and optional params alike.
    let result = support::run_ruby(
        r#"
        class A
          def greet(name, punct = ".")
            "hi #{name}#{punct}"
          end
        end

        class B < A
          def greet(name, punct = ".")
            name = name + "!"
            super
          end
        end

        puts B.new.greet("bob")
        puts B.new.greet("bob", "?")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi bob!.\nhi bob!?\n");
}

#[test]
fn bare_super_forwards_rest_and_keyword_params() {
    let result = support::run_ruby(
        r#"
        class C
          def count(*nums)
            nums.length
          end
        end

        class D < C
          def count(*nums)
            super * 10
          end
        end

        puts D.new.count(1, 2, 3)

        class E
          def kw(a:, b: 2)
            "a=#{a} b=#{b}"
          end
        end

        class F < E
          def kw(a:, b: 2)
            "got " + super
          end
        end

        puts F.new.kw(a: 1, b: 9)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "30\ngot a=1 b=9\n");
}

#[test]
fn super_with_empty_parens_passes_no_arguments() {
    // `super()` and bare `super` mean OPPOSITE things: the parens form
    // passes nothing, so the parent's optional takes its default.
    let result = support::run_ruby(
        r#"
        class G
          def greet(name = "anon")
            "hi #{name}"
          end
        end

        class H < G
          def greet(name)
            super() + "!"
          end
        end

        puts H.new.greet("bob")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi anon!\n");
}

#[test]
fn self_referential_collections_print_recursion_markers() {
    // Pre-15.2 both of these self-deadlocked on the collection's own
    // non-reentrant payload Mutex.
    let result = support::run_ruby(
        r#"
        a = [1, 2]
        a << a
        puts a

        h = { k: 1 }
        h[:me] = h
        puts h
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n2\n[...]\n{k: 1, me: {...}}\n");
}

#[test]
fn dup_and_clone_on_collections_follow_the_frozen_rule() {
    // dup: fresh unfrozen payload (mutable even when the source is
    // frozen, and mutating it leaves the source untouched); clone:
    // carries the frozen flag.
    let result = support::run_ruby(
        r#"
        arr = [1].freeze
        d = arr.dup
        d << 2
        puts d.length
        puts arr.length
        puts d.frozen?
        puts arr.clone.frozen?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n1\nfalse\ntrue\n");
}

#[test]
fn dup_and_clone_on_user_objects_copy_ivars_shallowly() {
    let result = support::run_ruby(
        r#"
        class Point
          attr_accessor :x, :y

          def initialize(x, y)
            @x = x
            @y = y
          end
        end

        p1 = Point.new(1, 2)
        p2 = p1.dup
        p2.x = 99
        puts p1.x
        puts p2.x
        puts p2.y
        p1.freeze
        puts p1.clone.frozen?
        puts p1.dup.frozen?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n99\n2\ntrue\nfalse\n");
}

#[test]
fn a_user_defined_dup_override_wins() {
    // `dup`/`clone` are ordinary overridable Kernel methods in real Ruby --
    // the static arm must fall through to Path 1 dispatch when the
    // receiver's class defines its own.
    let result = support::run_ruby(
        r#"
        class W
          def dup
            "custom"
          end
        end

        puts W.new.dup
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "custom\n");
}

#[test]
fn dup_dispatches_dynamically_through_send() {
    let result = support::run_ruby(
        r#"
        puts [1, 2].send(:dup).length
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n");
}

// -- Phase 15.3: nested classes/modules + constant paths (namespacing).
// Every positive expectation oracle-verified against real ruby 4.0.5.

#[test]
fn nested_classes_define_dispatch_and_resolve_lexical_constants() {
    let result = support::run_ruby(
        r#"
        module Store
          DEFAULT = 10

          class Item
            def price
              DEFAULT
            end
          end

          class Errors
            class NotFound
              def msg
                "not found"
              end
            end
          end
        end

        puts Store::Item.new.price
        puts Store::Errors::NotFound.new.msg
        puts Store::DEFAULT
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\nnot found\n10\n");
}

#[test]
fn same_leaf_class_name_in_two_namespaces_stays_distinct() {
    let result = support::run_ruby(
        r#"
        module A1
          class Widget
            def tag
              "a"
            end
          end
        end

        module B1
          class Widget
            def tag
              "b"
            end
          end
        end

        puts A1::Widget.new.tag
        puts B1::Widget.new.tag
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "a\nb\n");
}

#[test]
fn bare_constant_resolution_walks_the_lexical_chain_innermost_first() {
    let result = support::run_ruby(
        r#"
        X = "top"
        module Outer
          X = "outer"
          module Inner
            X = "inner"
            def self.probe
              X
            end
          end
          def self.probe
            X
          end
        end
        puts Outer::Inner.probe
        puts Outer.probe
        puts X
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "inner\nouter\ntop\n");
}

#[test]
fn qualified_definition_form_does_not_see_the_namespace_lexically() {
    // `class Store::Cart`'s cref is just [Cart] -- real Ruby raises
    // NameError for `DEFAULT`, naming the cref head's qualified path.
    let result = support::run_ruby(
        r#"
        module Store
          DEFAULT = 10
        end

        class Store::Cart
          def d
            DEFAULT
          rescue NameError => e
            "NameError: #{e.message}"
          end
        end

        puts Store::Cart.new.d
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "NameError: uninitialized constant Store::Cart::DEFAULT\n"
    );
}

#[test]
fn constants_resolve_through_the_superclass_chain() {
    let result = support::run_ruby(
        r#"
        class Base2
          LIMIT = 5
        end

        class Sub2 < Base2
          def l
            LIMIT
          end
        end

        puts Sub2.new.l
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\n");
}

#[test]
fn missing_qualified_constant_raises_name_error_with_the_full_path() {
    let result = support::run_ruby(
        r#"
        module Store
        end

        begin
          puts Store::MISSING
        rescue NameError => e
          puts "NameError: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "NameError: uninitialized constant Store::MISSING\n"
    );
}

#[test]
fn nested_classes_reopen_through_both_definition_forms() {
    let result = support::run_ruby(
        r#"
        module Store
          class Item
            def tag
              "tagged"
            end
          end
        end

        module Store
          class Item
            def more
              "reopened nested"
            end
          end
        end

        class Store::Item
          def qual
            "qualified reopen"
          end
        end

        i = Store::Item.new
        puts i.tag
        puts i.more
        puts i.qual
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "tagged\nreopened nested\nqualified reopen\n");
}

#[test]
fn exception_classes_work_across_namespaces() {
    // Subclassing a nested error class from outside, raising with a
    // qualified path, and rescue-matching through the shared ancestor.
    let result = support::run_ruby(
        r#"
        module Errs
          class Base3 < StandardError
          end
        end

        class Deep < Errs::Base3
        end

        begin
          raise Deep, "boom"
        rescue Errs::Base3 => e
          puts "caught #{e.message}"
        end

        begin
          raise Errs::Base3, "direct"
        rescue StandardError => e
          puts "caught #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught boom\ncaught direct\n");
}

#[test]
fn qualified_class_paths_work_in_patterns_and_is_a() {
    let result = support::run_ruby(
        r#"
        module Store
          class Item
            def initialize(n)
              @n = n
            end

            def deconstruct_keys(keys)
              { n: @n }
            end
          end
        end

        case Store::Item.new(5)
        in Store::Item
          puts "matched class pattern"
        end

        case Store::Item.new(7)
        in Store::Item(n:)
          puts "matched with capture #{n}"
        end

        puts Store::Item.new(1).is_a?(Store::Item)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "matched class pattern\nmatched with capture 7\ntrue\n"
    );
}

#[test]
fn including_a_nested_module_materializes_its_methods() {
    let result = support::run_ruby(
        r#"
        module Util
          module Greet
            def hello
              "hello from nested module"
            end
          end
        end

        class Greeter2
          include Util::Greet
        end

        puts Greeter2.new.hello
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hello from nested module\n");
}

#[test]
fn qualified_constant_writes_target_the_named_namespace() {
    let result = support::run_ruby(
        r#"
        module Cfg
        end

        Cfg::LIMIT = 99
        puts Cfg::LIMIT
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "99\n");
}

// -- Phase 16.1: first-class Class/Module values. Every expectation
// oracle-verified against real ruby 4.0.5.

#[test]
fn class_values_print_and_compare_by_identity() {
    let result = support::run_ruby(
        r#"
        class Widget
        end

        module Store
          class Item
          end
        end

        puts Widget
        p Widget
        puts Store::Item
        puts Widget == Widget
        puts Widget == Store::Item
        puts Widget != Store::Item
        arr = [Widget, Store::Item]
        puts arr.include?(Widget)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Widget\nWidget\nStore::Item\ntrue\nfalse\ntrue\ntrue\n"
    );
}

#[test]
fn a_class_stored_in_a_variable_constructs_and_dispatches() {
    let result = support::run_ruby(
        r#"
        class Widget
          def initialize(n)
            @n = n
          end

          def n
            @n
          end
        end

        x = Widget
        puts x.new(5).n
        puts x.name
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\nWidget\n");
}

#[test]
fn dot_class_reflects_every_receiver_kind() {
    let result = support::run_ruby(
        r#"
        class Widget
        end
        module Helper
        end

        w = Widget.new
        puts w.class
        puts w.class == Widget
        puts w.class.name
        puts 5.class
        puts "s".class
        puts [].class
        puts nil.class
        puts true.class
        puts 1.5.class
        puts :sym.class
        puts (1..2).class
        puts({}.class)
        puts Helper.class
        puts Widget.class
        puts Class.class
        puts Widget.class == Class
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Widget\ntrue\nWidget\nInteger\nString\nArray\nNilClass\nTrueClass\nFloat\nSymbol\nRange\nHash\nModule\nClass\nClass\ntrue\n"
    );
}

#[test]
fn dot_class_works_on_rescue_bindings_and_poly_receivers() {
    let result = support::run_ruby(
        r#"
        begin
          raise "boom"
        rescue => e
          puts e.class
        end

        mixed = [1, "two", nil]
        mixed.each do |v|
          puts v.class
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "RuntimeError\nInteger\nString\nNilClass\n"
    );
}

#[test]
fn no_method_error_messages_name_the_real_class() {
    let result = support::run_ruby(
        r#"
        class Widget
        end

        w = [Widget.new].first
        begin
          w.nope
        rescue NoMethodError => e
          puts e.message
        end

        module Helper
        end

        begin
          Helper.new
        rescue NoMethodError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "undefined method 'nope' for an instance of Widget\nundefined method 'new' for module Helper\n"
    );
}

#[test]
fn is_a_and_instance_of_answer_through_class_values() {
    let result = support::run_ruby(
        r#"
        class Widget
        end
        module Helper
        end

        w = Widget.new
        puts w.instance_of?(Widget)
        puts w.instance_of?(Object)
        puts 5.instance_of?(Integer)
        puts Widget.is_a?(Class)
        puts Widget.is_a?(Module)
        puts Helper.is_a?(Module)
        puts Helper.is_a?(Class)
        puts Widget.ancestors.first == Widget
        puts Widget.ancestors.include?(Object)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nfalse\ntrue\ntrue\ntrue\ntrue\nfalse\ntrue\ntrue\n"
    );
}

#[test]
fn case_when_with_class_candidates_checks_instance_ancestry() {
    // `Module#===` -- previously a compile-time rejection (`when Integer`
    // never worked); now real Ruby's instance-of check.
    let result = support::run_ruby(
        r#"
        class Widget
        end

        [5, "s", Widget.new, nil].each do |v|
          case v
          when Integer then puts "int"
          when String then puts "str"
          when Widget then puts "widget"
          else puts "other"
          end
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "int\nstr\nwidget\nother\n");
}

// -- Phase 16.2: user-overridable object protocols + Comparable. Every
// expectation oracle-verified against real ruby 4.0.5.

#[test]
fn user_defined_equality_dispatches_everywhere() {
    let result = support::run_ruby(
        r#"
        class Point
          attr_reader :x

          def initialize(x)
            @x = x
          end

          def ==(other)
            other.is_a?(Point) && x == other.x
          end
        end

        puts Point.new(1) == Point.new(1)
        puts Point.new(1) == Point.new(2)
        puts Point.new(1) == 5
        puts Point.new(1) != Point.new(2)
        puts [Point.new(1), Point.new(2)] == [Point.new(1), Point.new(2)]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\nfalse\ntrue\ntrue\n");
}

#[test]
fn collections_compare_element_wise_and_objects_default_to_identity() {
    let result = support::run_ruby(
        r#"
        puts [1, [2, 3]] == [1, [2, 3]]
        puts({ a: 1, b: [2] } == { a: 1, b: [2] })
        puts({ a: 1 } == { a: 2 })

        class Blank
        end

        b1 = Blank.new
        b2 = Blank.new
        puts b1 == b1
        puts b1 == b2
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\nfalse\ntrue\nfalse\n");
}

#[test]
fn user_to_s_and_inspect_drive_puts_interpolation_and_p() {
    let result = support::run_ruby(
        r##"
        class Named
          def initialize(n)
            @n = n
          end

          def to_s
            "Named<#{@n}>"
          end

          def inspect
            "#<Named n=#{@n}>"
          end
        end

        n = Named.new(7)
        puts n
        puts "in a string: #{n}"
        p n
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Named<7>\nin a string: Named<7>\n#<Named n=7>\n"
    );
}

#[test]
fn user_hash_protocol_keys_hashes_by_value() {
    let result = support::run_ruby(
        r#"
        class Key
          attr_reader :k

          def initialize(k)
            @k = k
          end

          def hash
            k.hash
          end

          def eql?(other)
            other.is_a?(Key) && k == other.k
          end
        end

        h = {}
        h[Key.new("a")] = 1
        puts h[Key.new("a")]
        puts h[Key.new("b")].inspect
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\nnil\n");
}

#[test]
fn comparable_drives_the_includers_spaceship() {
    let result = support::run_ruby(
        r#"
        class Temp
          include Comparable
          attr_reader :deg

          def initialize(d)
            @deg = d
          end

          def <=>(other)
            deg <=> other.deg
          end
        end

        a = Temp.new(50)
        b = Temp.new(70)
        puts a < b
        puts a > b
        puts a <= b
        puts b >= a
        puts a == Temp.new(50)
        puts a.between?(Temp.new(40), Temp.new(60))
        puts a.clamp(Temp.new(55), Temp.new(80)).deg
        puts a.clamp(Temp.new(20), Temp.new(30)).deg
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nfalse\ntrue\ntrue\ntrue\ntrue\n55\n30\n"
    );
}

#[test]
fn enumerable_min_max_dispatch_a_user_spaceship() {
    let result = support::run_ruby(
        r#"
        class Temp
          include Comparable
          attr_reader :deg

          def initialize(d)
            @deg = d
          end

          def <=>(other)
            deg <=> other.deg
          end
        end

        temps = [Temp.new(3), Temp.new(9), Temp.new(5)]
        puts temps.min.deg
        puts temps.max.deg
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n9\n");
}

#[test]
fn explicit_triple_equals_on_class_values_checks_ancestry() {
    let result = support::run_ruby(
        r#"
        class Widget
        end

        puts Integer === 5
        puts Widget === Widget.new
        puts Widget === 5
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\nfalse\n");
}

// ---------------------------------------------------------------------------
// Phase 16.3 -- builtin-class reopening (root). Every expectation below was
// oracle-verified against real `ruby` (4.0.5) before being written down.
// ---------------------------------------------------------------------------

/// The Box docs' motivating example shape: a fresh method on `class String`,
/// dispatching statically ("".blank?), dynamically through a Poly ivar
/// (@s.blank?), and calling a NATIVE builtin method (`length`) implicitly on
/// self from inside the reopen.
#[test]
fn builtin_reopen_string_blank_dispatches_everywhere() {
    let result = support::run_ruby(
        r#"
        class String
          def blank?
            length == 0
          end
        end

        class Foo
          def initialize(s)
            @s = s
          end

          def foo_is_blank?
            @s.blank?
          end
        end

        puts "".blank?
        puts "  hi".blank?
        puts Foo.new("").foo_is_blank?
        puts Foo.new("x").foo_is_blank?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\ntrue\nfalse\n");
}

/// Overriding an EXISTING native method: the user `length` wins at a static
/// call site (where `try_collection_dispatch` would answer 3), at a dynamic
/// Poly site, and `size` -- a separate method in real Ruby, NOT an alias of
/// the override -- stays native.
#[test]
fn builtin_reopen_override_beats_native_length() {
    let result = support::run_ruby(
        r#"
        class String
          def length
            42
          end
        end

        s = "abc"
        puts s.length
        puts [s].first.length
        puts "xy".size
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n42\n2\n");
}

/// Blocks through a builtin reopen: `yield` + an optional parameter, and an
/// escaping block that captures both a mutated local (a Captured cell) and
/// `self` (the `__self: RubyValue` clone path).
#[test]
fn builtin_reopen_methods_take_blocks_and_capture_self() {
    let result = support::run_ruby(
        r#"
        class Integer
          def repeat(sep = "-")
            out = ""
            i = 0
            while i < self
              out = out + yield(i).to_s
              out = out + sep if i < self - 1
              i = i + 1
            end
            out
          end

          def add_each(arr)
            total = 0
            arr.each do |x|
              total = total + x + self
            end
            total
          end
        end

        puts 3.repeat { |i| i * 2 }
        puts 2.repeat("+") { |i| i + 1 }
        puts 10.add_each([1, 2, 3])
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "0-2-4\n1+2\n36\n");
}

/// Class-level state on a builtin: `@@cvar` + `CONST` in the body (ordinary
/// ownership machinery, owner = the builtin's ClassId), `def self.x` via the
/// generated `__bm_Array` container, and `Array::LIMIT` readable externally.
#[test]
fn builtin_reopen_cvars_consts_and_class_methods() {
    let result = support::run_ruby(
        r#"
        class Array
          @@made = 0
          LIMIT = 3

          def self.tally_up
            @@made = @@made + 1
            @@made
          end

          def under_limit?
            length < LIMIT
          end
        end

        puts Array.tally_up
        puts Array.tally_up
        puts [1, 2].under_limit?
        puts [1, 2, 3, 4].under_limit?
        puts Array::LIMIT
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n2\ntrue\nfalse\n3\n");
}

/// A patch calling another patch (implicit self -> direct free-fn call, the
/// result's `+` going back through dynamic String dispatch), and an unknown
/// method still raising real Ruby's exact NoMethodError.
#[test]
fn builtin_reopen_chained_patches_and_nme() {
    let result = support::run_ruby(
        r#"
        class String
          def shout
            exclaim + "?"
          end

          def exclaim
            self + "!"
          end
        end

        puts "hey".shout
        begin
          "hey".nope
        rescue NoMethodError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "hey!?\nundefined method 'nope' for an instance of String\n"
    );
}

/// The receiver-kind breadth: NilClass (no static TyKind -- fully dynamic),
/// Hash (implicit native `size`), and Range (static `self.last`/`self.first`
/// fast paths inside the reopen body).
#[test]
fn builtin_reopen_nil_hash_and_range() {
    let result = support::run_ruby(
        r#"
        class NilClass
          def describe
            "nothing"
          end
        end

        class Hash
          def pair_count
            size
          end
        end

        class Range
          def span
            self.last - self.first
          end
        end

        puts nil.describe
        puts({ a: 1, b: 2 }.pair_count)
        puts (3..9).span
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "nothing\n2\n6\n");
}

/// Full `Params` support on a reopen method (required + rest), plus
/// `respond_to?` seeing value methods through the widened registry probe.
#[test]
fn builtin_reopen_rest_params_and_respond_to() {
    let result = support::run_ruby(
        r#"
        class String
          def tag(first, *rest)
            label = first.to_s
            rest.each do |r|
              label = label + "|" + r.to_s
            end
            label + ":" + self
          end
        end

        puts "v".tag("a", "b", "c")
        puts "w".tag("z")
        puts "x".respond_to?(:tag)
        puts "x".respond_to?(:zzz)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "a|b|c:v\nz:w\ntrue\nfalse\n");
}

/// `to_s`/`inspect` overrides on a builtin drive `puts`, interpolation,
/// `p`, CONTAINER inspect (real Ruby's rb_inspect dispatches per element --
/// oracle-verified `[5].inspect` -> `[I]`), and explicit `.to_s`.
#[test]
fn builtin_reopen_to_s_and_inspect_drive_rendering() {
    let result = support::run_ruby(
        r#"
        class Integer
          def to_s
            "int"
          end

          def inspect
            "I"
          end
        end

        puts 5
        puts "v=#{5}"
        p 7
        puts [5].inspect
        puts 6.to_s
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "int\nv=int\nI\n[I]\nint\n");
}

/// A `dup` override beats universal `Kernel#dup` on both the static String
/// site and a Poly site (the any-builtin-overrides fall-through); `clone`
/// -- not overridden -- stays the universal shallow copy.
#[test]
fn builtin_reopen_dup_override_beats_kernel_dup() {
    let result = support::run_ruby(
        r#"
        class String
          def dup
            "dupped"
          end
        end

        s = "orig"
        puts s.dup
        puts [s].first.dup
        puts s.clone
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "dupped\ndupped\norig\n");
}

/// `send` with a literal symbol reaches a reopen method through
/// `send_value`'s value-method-first probe (the generated arity-checked
/// trampoline), same result as the direct static call.
#[test]
fn builtin_reopen_methods_reachable_via_send() {
    let result = support::run_ruby(
        r#"
        class String
          def echo(a, b)
            a.to_s + b.to_s + self
          end
        end

        puts "s".send(:echo, 1, 2)
        puts "s".echo(9, 8)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "12s\n98s\n");
}

// ---------------------------------------------------------------------------
// Phase 17.1-A -- the CRuby-exact builtin hierarchy (BasicObject/Kernel/
// Numeric/Rational/Complex/Math/Struct/Enumerator in the ABI; declarative
// superclass/includes seeding). Oracle: ruby 4.0.5.
// ---------------------------------------------------------------------------

/// THE keystone parity test: `.ancestors` for every core class, byte-
/// identical to real ruby. (Mutex/Queue excluded: real Ruby names them
/// `Thread::Mutex`/`Thread::Queue` -- a documented naming divergence.)
#[test]
fn builtin_ancestors_are_cruby_exact() {
    let result = support::run_ruby(
        r##"
        [BasicObject, Object, Kernel, Comparable, Enumerable,
         Numeric, Integer, Float, Rational, Complex,
         String, Symbol, Array, Hash, Range,
         NilClass, TrueClass, FalseClass,
         Proc, Regexp, MatchData, Struct, Enumerator,
         Class, Module, Math, Fiber, Thread, Ractor].each do |c|
          puts "#{c}: #{c.ancestors.inspect}"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "BasicObject: [BasicObject]\n\
         Object: [Object, Kernel, BasicObject]\n\
         Kernel: [Kernel]\n\
         Comparable: [Comparable]\n\
         Enumerable: [Enumerable]\n\
         Numeric: [Numeric, Comparable, Object, Kernel, BasicObject]\n\
         Integer: [Integer, Numeric, Comparable, Object, Kernel, BasicObject]\n\
         Float: [Float, Numeric, Comparable, Object, Kernel, BasicObject]\n\
         Rational: [Rational, Numeric, Comparable, Object, Kernel, BasicObject]\n\
         Complex: [Complex, Numeric, Comparable, Object, Kernel, BasicObject]\n\
         String: [String, Comparable, Object, Kernel, BasicObject]\n\
         Symbol: [Symbol, Comparable, Object, Kernel, BasicObject]\n\
         Array: [Array, Enumerable, Object, Kernel, BasicObject]\n\
         Hash: [Hash, Enumerable, Object, Kernel, BasicObject]\n\
         Range: [Range, Enumerable, Object, Kernel, BasicObject]\n\
         NilClass: [NilClass, Object, Kernel, BasicObject]\n\
         TrueClass: [TrueClass, Object, Kernel, BasicObject]\n\
         FalseClass: [FalseClass, Object, Kernel, BasicObject]\n\
         Proc: [Proc, Object, Kernel, BasicObject]\n\
         Regexp: [Regexp, Object, Kernel, BasicObject]\n\
         MatchData: [MatchData, Object, Kernel, BasicObject]\n\
         Struct: [Struct, Enumerable, Object, Kernel, BasicObject]\n\
         Enumerator: [Enumerator, Enumerable, Object, Kernel, BasicObject]\n\
         Class: [Class, Module, Object, Kernel, BasicObject]\n\
         Module: [Module, Object, Kernel, BasicObject]\n\
         Math: [Math]\n\
         Fiber: [Fiber, Object, Kernel, BasicObject]\n\
         Thread: [Thread, Object, Kernel, BasicObject]\n\
         Ractor: [Ractor, Object, Kernel, BasicObject]\n"
    );
}

/// The hierarchy is live in `is_a?`/`kind_of?`/`instance_of?` -- statically
/// folded sites AND the runtime path through a Poly receiver, plus a user
/// class inheriting the full Object tail. Every line oracle-verified.
#[test]
fn is_a_walks_the_cruby_chains() {
    let result = support::run_ruby(
        r#"
        class Widget; end

        puts 5.is_a?(Comparable)
        puts 5.is_a?(Numeric)
        puts 5.is_a?(BasicObject)
        puts 3.14.is_a?(Numeric)
        puts "s".is_a?(Comparable)
        puts [].is_a?(Kernel)
        puts nil.is_a?(BasicObject)
        puts 5.kind_of?(Comparable)
        puts 5.instance_of?(Numeric)
        puts [5].first.is_a?(Numeric)
        puts Widget.new.is_a?(Kernel)
        puts Widget.ancestors.inspect
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\nfalse\ntrue\ntrue\n[Widget, Object, Kernel, BasicObject]\n"
    );
}

// ---------------------------------------------------------------------------
// Phase 17.1-B -- the MRO-walking builtin method tables: Kernel/BasicObject/
// Comparable resolve as real ancestors on VALUE receivers, reopens are found
// per-ancestor, Range#=== is real, and arg-type mismatches raise CRuby's
// TypeError/ArgumentError shapes. Oracle: ruby 4.0.5.
// ---------------------------------------------------------------------------

/// Comparable's operators and Kernel's universals reach builtin values
/// dynamically: `"abc" < "abd"` resolves String(no `<`) -> Comparable(`<`
/// drives `<=>`) -> `String#<=>`; itself/tap/then/frozen?/eql?/equal? are
/// Kernel/BasicObject rows found through every value's chain.
#[test]
fn comparable_and_kernel_rows_reach_builtin_values() {
    let result = support::run_ruby(
        r##"
        x = "abc"
        puts x < "abd"
        puts x.between?("aaa", "b")
        puts "m".clamp("a", "f")
        puts 5.itself
        r = 5.tap { |v| puts "saw #{v}" }
        puts r
        puts 5.then { |v| v + 1 }
        puts 5.equal?(5)
        puts 5.send("itself")
        puts 5.frozen?
        puts "x".frozen?
        puts 5.eql?(5.0)
        puts 5.eql?(5)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\ntrue\nf\n5\nsaw 5\n5\n6\ntrue\n5\ntrue\nfalse\nfalse\ntrue\n"
    );
}

/// A `class Numeric` reopen materializes onto Integer AND Float (the mro
/// machinery) and is found on both static receivers and dynamic Poly ones
/// (the per-ancestor value-method walk).
#[test]
fn numeric_reopen_resolves_down_the_mro() {
    let result = support::run_ruby(
        r#"
        class Numeric
          def double
            self * 2
          end
        end

        puts 5.double
        puts 2.5.double
        puts [7].first.double
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n5.0\n14\n");
}

/// `Range#===` IS `#cover?` -- what makes `case x when 1..50` actually
/// match (previously it fell to structural equality and silently never
/// did). Endpoint semantics oracle-verified incl. exclusive ends, floats,
/// and incomparable-subject false.
#[test]
fn range_case_equality_is_real() {
    let result = support::run_ruby(
        r#"
        case 42
        when 1..50 then puts "hit"
        else puts "miss"
        end

        x = [5.5].first
        puts (1..5) === x
        puts (1..6) === x
        puts (1...5) === 5
        puts (1..5).cover?(3)
        puts (1..5).include?("x")
        case "c"
        when "a".."f" then puts "letter"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "hit\nfalse\ntrue\nfalse\ntrue\nfalse\nletter\n"
    );
}

/// The table rows validate argument TYPES with CRuby's exact TypeError/
/// ArgumentError messages (previously an arg-type mismatch degraded to
/// NoMethodError) -- all real, rescuable exceptions now.
#[test]
fn builtin_arg_mismatches_raise_cruby_error_shapes() {
    let result = support::run_ruby(
        r##"
        a = ["a"].first
        begin
          a + 1
        rescue TypeError => e
          puts "TypeError: #{e.message}"
        end
        arr = [[1]].first
        begin
          arr[:x]
        rescue TypeError => e
          puts "TypeError: #{e.message}"
        end
        one = [1].first
        begin
          one < "a"
        rescue ArgumentError => e
          puts "ArgumentError: #{e.message}"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "TypeError: no implicit conversion of Integer into String\n\
         TypeError: no implicit conversion of Symbol into Integer\n\
         ArgumentError: comparison of Integer with String failed\n"
    );
}

/// The send-ladder fidelity fix: a REAL module method (Enumerable's `map`,
/// via `include Enumerable` + `each`) resolves BEFORE `method_missing` --
/// previously method_missing fired first, the opposite of real Ruby.
#[test]
fn enumerable_resolves_before_method_missing() {
    let result = support::run_ruby(
        r#"
        class Sack
          include Enumerable
          def initialize(items)
            @items = items
          end
          def each(&b)
            @items.each(&b)
            self
          end
          def method_missing(name, *a)
            "mm:#{name}"
          end
        end

        s = Sack.new([1, 2, 3])
        puts s.map { |x| x * 10 }.inspect
        puts s.nope
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[10, 20, 30]\nmm:nope\n");
}

/// `respond_to?` walks the real MRO now: Enumerable names answer true on
/// arrays, Comparable names on strings, and Kernel privates stay invisible.
#[test]
fn respond_to_walks_the_mro() {
    let result = support::run_ruby(
        r#"
        puts [1, 2].respond_to?(:map)
        puts "s".respond_to?(:between?)
        puts 5.respond_to?(:puts)
        puts 5.respond_to?(:itself)
        puts "s".respond_to?(:nope)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\nfalse\ntrue\nfalse\n");
}

// ---------------------------------------------------------------------------
// Phase 17.1-C -- the numeric tower: full-bignum Integer, Rational, Complex,
// the coercion matrix, literals, Math, Kernel conversions, Float/Math
// constants. Oracle: ruby 4.0.5, byte-identical.
// ---------------------------------------------------------------------------

/// Bignum end-to-end: overflow promotion + demotion round trips, big
/// literals (decimal/hex/binary/underscored), interpolation, hash keys,
/// Enumerable, is_a?, and the i64::MIN / -1 overflow edge.
#[test]
fn bignum_integers_promote_demote_and_interoperate() {
    let result = support::run_ruby(
        r##"
        r = 1
        i = 2
        while i <= 25
          r = r * i
          i += 1
        end
        puts r
        puts 2 ** 100
        puts 100000000000000000000 + 1
        puts 0xff
        puts 0b1010
        puts 1_000_000
        big = 9_223_372_036_854_775_807
        puts big + 1
        puts big + 1 - 1
        puts (big + 1) > big
        puts (big + 1).class
        puts "v=#{2 ** 70}"
        h = { 2 ** 70 => :big }
        puts h[2 ** 70]
        puts [2 ** 70, 1, 2 ** 65].min
        puts (2 ** 70).is_a?(Numeric)
        puts 2 ** 70 == 2 ** 70
        puts(-9223372036854775808 / -1)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "15511210043330985984000000\n\
         1267650600228229401496703205376\n\
         100000000000000000001\n\
         255\n10\n1000000\n\
         9223372036854775808\n\
         9223372036854775807\n\
         true\n\
         Integer\n\
         v=1180591620717411303424\n\
         big\n1\ntrue\ntrue\n\
         9223372036854775808\n"
    );
}

/// Rational: literals, reduction, exact arithmetic across the Int lane,
/// `2 ** -2`, `quo`, Float promotion, rounding family, and the
/// ZeroDivisionError channel.
#[test]
fn rationals_are_exact_and_oracle_faithful() {
    let result = support::run_ruby(
        r##"
        r = 3r
        puts r.class
        p r
        puts r
        p 1.5r
        p Rational(4, 8)
        p Rational(1, 2) + Rational(1, 3)
        p Rational(1, 2) * 3
        p Rational(1, 2) / Rational(3, 4)
        p Rational(3, 4) ** 2
        p 2 ** -2
        p 1.quo(3)
        p Rational(1, 2) + 0.5
        puts Rational(1, 2) < Rational(2, 3)
        puts Rational(1, 2) == 0.5
        puts Rational(1, 3).to_f
        puts Rational(7, 2).to_i
        p Rational(-7, 2).floor
        p Rational(-7, 2).ceil
        p Rational(7, 2).round
        p Rational(3, 4).numerator
        p Rational(3, 4).denominator
        begin
          Rational(1, 0)
        rescue ZeroDivisionError => e
          puts "ZeroDivisionError: #{e.message}"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Rational\n(3/1)\n3/1\n(3/2)\n(1/2)\n(5/6)\n(3/2)\n(2/3)\n(9/16)\n(1/4)\n(1/3)\n1.0\n\
         true\ntrue\n0.3333333333333333\n3\n-4\n-3\n4\n3\n4\n\
         ZeroDivisionError: divided by 0\n"
    );
}

/// Complex: imaginary literals, component-class preservation (exact
/// Integer/Rational components incl. the den==1 demotion inside complex
/// division), formatting (`(2/25)*i`), polar surface, and coercion errors.
#[test]
fn complexes_keep_component_classes() {
    let result = support::run_ruby(
        r##"
        c = 4i
        puts c.class
        p c
        p 3 + 4i
        p Complex(1, 2) * Complex(3, 4)
        p Complex(1, 2) / Complex(3, 4)
        p Complex(1, 2) / 2
        p Complex(1, 2) ** 2
        p Complex(1.5, -2.5)
        puts Complex(1.5, -2.5)
        puts Complex(3, 4).abs
        p Complex(3, 4).abs2
        p Complex(3, 4).rect
        p Complex(1, -2).conjugate
        p Complex(2, 0) == 2
        p Complex(1, 2).real
        p Complex(1, 2).imaginary
        p 5.to_c
        begin
          Complex(1, 2) + "x"
        rescue TypeError => e
          puts "TypeError: #{e.message}"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Complex\n(0+4i)\n(3+4i)\n(-5+10i)\n((11/25)+(2/25)*i)\n((1/2)+1i)\n(-3+4i)\n\
         (1.5-2.5i)\n1.5-2.5i\n5.0\n25\n[3, 4]\n(1+2i)\ntrue\n1\n2\n(5+0i)\n\
         TypeError: String can't be coerced into Complex\n"
    );
}

/// The Numeric/Integer/Float Tier A breadth: divmod matrices, the rounding
/// families, gcd/lcm/digits/chr/ord, predicates, step/times/upto/downto,
/// exact Float#to_r, and bignum-capable Float#to_i.
#[test]
fn numeric_breadth_matches_the_oracle() {
    let result = support::run_ruby(
        r#"
        puts 7.divmod(3).inspect
        puts (-7).divmod(3).inspect
        puts 7.divmod(2.5).inspect
        puts (-7).abs
        puts 2.5.abs
        puts 4.even?
        puts 3.odd?
        puts 5.succ
        puts 5.pred
        puts 65.chr
        puts "A".ord
        puts 10.digits.inspect
        puts 255.digits(16).inspect
        puts 4.gcd(6)
        puts 4.lcm(6)
        puts 4.gcdlcm(6).inspect
        puts 255.to_s(16)
        puts 10.to_s(2)
        puts 25.round(-1)
        puts 1234.round(-2)
        puts (-15).round(-1)
        puts 1234.floor(-2)
        puts 1234.ceil(-2)
        puts 7.fdiv(2)
        puts 3.7.round
        puts 3.14159.round(2)
        puts (-2.7).floor
        puts 2.2.ceil
        puts 5.9.truncate
        puts 1e20.to_i
        puts 0.125.to_r.inspect
        puts (1.0 / 0).infinite?
        puts 1.5.nan?
        puts 2.5.finite?
        puts 5.zero?
        puts 0.zero?
        puts 5.positive?
        puts (-5).negative?
        puts 5.numerator
        puts 5.denominator
        puts 0.5.numerator
        puts 0.5.denominator
        puts (-7).remainder(3)
        acc = []
        1.step(10, 3) { |i| acc << i }
        puts acc.inspect
        acc2 = []
        3.times { |i| acc2 << i }
        2.upto(4) { |i| acc2 << i }
        3.downto(1) { |i| acc2 << i }
        puts acc2.inspect
        puts 255.bit_length
        puts 5.to_f
        puts 5.to_r.inspect
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[2, 1]\n[-3, 2]\n[2, 2.0]\n7\n2.5\ntrue\ntrue\n6\n4\nA\n65\n\
         [0, 1]\n[15, 15]\n2\n12\n[2, 12]\nff\n1010\n\
         30\n1200\n-20\n1200\n1300\n3.5\n4\n3.14\n-3\n3\n5\n\
         100000000000000000000\n(1/8)\n1\nfalse\ntrue\n\
         false\ntrue\ntrue\ntrue\n5\n1\n1\n2\n-1\n\
         [1, 4, 7, 10]\n[0, 1, 2, 2, 3, 4, 3, 2, 1]\n8\n5.0\n(5/1)\n"
    );
}

/// Math module functions + Math::DomainError + the Float constants, and
/// the Kernel conversion functions with CRuby's exact failure shapes.
#[test]
fn math_constants_and_kernel_conversions() {
    let result = support::run_ruby(
        r##"
        puts Math::PI
        puts Math::E
        puts Math.sqrt(16)
        puts Math.sqrt(2)
        puts Math.cbrt(27)
        puts Math.log2(8)
        puts Math.log(Math::E)
        puts Math.log(8, 2)
        puts Math.hypot(3, 4)
        puts Math.atan2(1, 1)
        begin
          Math.sqrt(-1)
        rescue Math::DomainError => e
          puts "Math::DomainError: #{e.message}"
        end
        puts Float::INFINITY
        puts Float::EPSILON
        puts Float::MAX
        puts Float::DIG
        puts Float::RADIX
        puts Integer("42")
        puts Integer("ff", 16)
        puts Integer("0x1A")
        puts Integer(" -4_2 ")
        puts Integer(3.9)
        puts Float("1.5e3")
        puts Float(2)
        puts String(42)
        puts Array(nil).inspect
        puts Array(1..3).inspect
        puts Array(5).inspect
        puts Hash(nil).inspect
        p Rational(3, 4)
        p Complex(1, 2)
        begin
          Integer("nope")
        rescue ArgumentError => e
          puts "ArgumentError: #{e.message}"
        end
        begin
          Integer(nil)
        rescue TypeError => e
          puts "TypeError: #{e.message}"
        end
        begin
          Float("x")
        rescue ArgumentError => e
          puts "ArgumentError: #{e.message}"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "3.141592653589793\n2.718281828459045\n4.0\n1.4142135623730951\n3.0\n3.0\n1.0\n3.0\n\
         5.0\n0.7853981633974483\nMath::DomainError: Numerical argument is out of domain - sqrt\n\
         Infinity\n2.220446049250313e-16\n1.7976931348623157e+308\n15\n2\n\
         42\n255\n26\n-42\n3\n1500.0\n2.0\n42\n[]\n[1, 2, 3]\n[5]\n{}\n(3/4)\n(1+2i)\n\
         ArgumentError: invalid value for Integer(): \"nope\"\n\
         TypeError: can't convert nil into Integer\n\
         ArgumentError: invalid value for Float(): \"x\"\n"
    );
}

// ---------------------------------------------------------------------------
// Phase 17.1-D -- String + Symbol Tier A breadth. Oracle: ruby 4.0.5.
// ---------------------------------------------------------------------------

/// The String Tier A surface: case/strip families, split shapes, chomp,
/// indexing forms, sub/gsub (String + block), tr/delete/squeeze/count,
/// lenient conversions, succ carry, padding, and `%` formatting.
#[test]
fn string_breadth_matches_the_oracle() {
    let result = support::run_ruby(
        r#"
        p "hello world".capitalize
        p "HeLLo".swapcase
        p "hello".upcase
        p "HELLO".downcase
        p "  hi  ".strip
        p "  hi".lstrip
        p "hi  ".rstrip
        p "hello".chars
        p "a,b,,c".split(",")
        p "a b  c".split
        p "hello".split("l")
        p "hello".chomp("lo")
        p "hello".chop
        p "abc" * 3
        p "abc".reverse
        p "hello".index("l")
        p "hello".rindex("l")
        p "hello".index("x")
        p "hello"[1]
        p "hello"[1, 3]
        p "hello"[1..3]
        p "hello".sub("l", "L")
        p "hello".gsub("l", "L")
        p "hello".gsub("l") { |m| m.upcase }
        p "hello".start_with?("he")
        p "hello".end_with?("lo", "x")
        p "hello".tr("el", "ip")
        p "hello".tr("a-y", "b-z")
        p "42abc".to_i
        p "abc".to_i
        p "ff".to_i(16)
        p "42.5xyz".to_f
        p "hello".to_sym
        p "az".succ
        p "zz".succ
        p "a\nb\nc".lines
        p "Hello %s, you are %d" % ["Bob", 42]
        p "%05.1f|%x|%o|%b|%e|%g|%%" % [3.14159, 255, 8, 5, 12345.678, 0.00001]
        p "hi".center(7, "*")
        p "hi".ljust(5, ".")
        p "hi".rjust(5, ".")
        p "hello".delete("l")
        p "aabbcc".squeeze
        p "aabbcc".squeeze("a")
        p "hello world".count("lo")
        s = "orig"
        s.replace("xyz")
        p s
        s << "!"
        p s
        s.prepend("ab")
        p s
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"Hello world\"\n\"hEllO\"\n\"HELLO\"\n\"hello\"\n\"hi\"\n\"hi\"\n\"hi\"\n\
         [\"h\", \"e\", \"l\", \"l\", \"o\"]\n[\"a\", \"b\", \"\", \"c\"]\n[\"a\", \"b\", \"c\"]\n\
         [\"he\", \"\", \"o\"]\n\"hel\"\n\"hell\"\n\"abcabcabc\"\n\"cba\"\n2\n3\nnil\n\
         \"e\"\n\"ell\"\n\"ell\"\n\"heLlo\"\n\"heLLo\"\n\"heLLo\"\ntrue\ntrue\n\
         \"hippo\"\n\"ifmmp\"\n42\n0\n255\n42.5\n:hello\n\"ba\"\n\"aaa\"\n\
         [\"a\\n\", \"b\\n\", \"c\"]\n\"Hello Bob, you are 42\"\n\
         \"003.1|ff|10|101|1.234568e+04|1e-05|%\"\n\"**hi***\"\n\"hi...\"\n\"...hi\"\n\
         \"heo\"\n\"abc\"\n\"abbcc\"\n5\n\"xyz\"\n\"xyz!\"\n\"abxyz!\"\n"
    );
}

/// Symbol Tier A + the `&:sym` block-argument conversion
/// (`Symbol#to_proc`), previously unsupported.
#[test]
fn symbol_breadth_and_to_proc() {
    let result = support::run_ruby(
        r#"
        p :b <=> :a
        p :hello.length
        p :a.succ
        p :HeLLo.downcase
        p :he.to_s
        p :he.inspect
        p :he.upcase
        p :he.capitalize
        p :he.empty?
        p :upcase.to_proc.call("hi")
        p [3, 1, 2].map(&:to_s)
        p ["b", "a"].map(&:upcase)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "1\n5\n:b\n:hello\n\"he\"\n\":he\"\n:HE\n:He\nfalse\n\"HI\"\n\
         [\"3\", \"1\", \"2\"]\n[\"B\", \"A\"]\n"
    );
}

// ---------------------------------------------------------------------------
// Phase 17.1-E -- Array/Hash/Range Tier A breadth. Oracle: ruby 4.0.5.
// ---------------------------------------------------------------------------

/// The Array Tier A surface: set ops, mutators, sort family, flatten/
/// compact/uniq, join/to_h, fetch/dig/zip/rotate/values_at, and the
/// in-place filters' self-or-nil contract.
#[test]
fn array_breadth_matches_the_oracle() {
    let result = support::run_ruby(
        r#"
        p [1, 2] + [3]
        p [1, 2, 3] - [2]
        p [1, 2] * 2
        p [1, 2] * ","
        p([1, 2, 3] & [2, 3, 4])
        p([1, 2] | [2, 3])
        p([1, 2, 3] <=> [1, 2, 4])
        p [3, 1, 2].sort
        p [3, 1, 2].sort { |a, b| b <=> a }
        a = [1, 2, 3]
        p a.pop
        p a.shift
        a.unshift(9)
        a.push(8, 7)
        p a
        p [1, [2, [3]]].flatten
        p [1, [2, [3]]].flatten(1)
        p [1, nil, 2, nil].compact
        p [1, 2, 2, 3, 1].uniq
        p [1, 2, 3].reverse
        p [[1, :a], [2, :b]].to_h
        p [1, 2, 3].join
        p [1, 2, 3].join("-")
        p [1, 2, 3].index(2)
        p [1, 2, 3].index(9)
        p [1, 2, 1].rindex(1)
        p [[1, [2, 3]]].dig(0, 1, 0)
        p [1, 2].fetch(0)
        p [1, 2].fetch(9, :fallback)
        begin
          [1, 2].fetch(9)
        rescue IndexError => e
          puts "IndexError"
        end
        p [1, 2, 3, 4].take(2)
        p [1, 2, 3, 4].drop(2)
        p [1, 2, 3].zip([4, 5, 6], [7, 8, 9])
        p [1, 2, 3].rotate
        p [1, 2, 3].rotate(2)
        p [1, 2, 3, 4, 5].values_at(0, 2, 4)
        p [1, 2, 3].at(-1)
        p [0, 1, 2, 3, 4][1..3]
        p [1, 2, 3].delete(2)
        p [1, 2, 3].delete_at(0)
        p [1, 2, 3].insert(1, :x)
        p [1, 2].concat([3, 4])
        p [1, 2, 3].fill(0)
        p [1, 2, 3].clear
        b = [3, 1, 2]
        b.sort!
        p b
        c = [1, 2, 3]
        c.map! { |x| x * 10 }
        p c
        p [1, 2, 3, 4].select! { |x| x > 2 }
        p [1, 2, 3, 4].reject! { |x| x > 2 }
        p [1, 2].select! { |x| x > 0 }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2, 3]\n[1, 3]\n[1, 2, 1, 2]\n\"1,2\"\n[2, 3]\n[1, 2, 3]\n-1\n\
         [1, 2, 3]\n[3, 2, 1]\n3\n1\n[9, 2, 8, 7]\n\
         [1, 2, 3]\n[1, 2, [3]]\n[1, 2]\n[1, 2, 3]\n[3, 2, 1]\n\
         {1 => :a, 2 => :b}\n\"123\"\n\"1-2-3\"\n1\nnil\n2\n2\n1\n:fallback\nIndexError\n\
         [1, 2]\n[3, 4]\n[[1, 4, 7], [2, 5, 8], [3, 6, 9]]\n[2, 3, 1]\n[3, 1, 2]\n\
         [1, 3, 5]\n3\n[1, 2, 3]\n2\n1\n[1, :x, 2, 3]\n[1, 2, 3, 4]\n[0, 0, 0]\n[]\n\
         [1, 2, 3]\n[10, 20, 30]\n[3, 4]\n[1, 2]\nnil\n"
    );
}

/// The Hash + Range Tier A surface: merge (with conflict block), fetch
/// shapes, dig, invert/key/value?, filters + transforms, each_key/value,
/// Range size/step/last(n), and String-range iteration via succ.
#[test]
fn hash_and_range_breadth_match_the_oracle() {
    let result = support::run_ruby(
        r##"
        h = { a: 1, b: 2 }
        p h.merge({ c: 3 })
        p h.merge({ a: 9 }) { |k, old, new| old + new }
        p h.to_a
        p h.invert
        p h.key(2)
        p h.key(9)
        p h.fetch(:a)
        p h.fetch(:x, 0)
        begin
          h.fetch(:x)
        rescue KeyError => e
          puts "KeyError: #{e.message}"
        end
        p(h.fetch(:x) { |k| "no #{k}" })
        p(h.select { |k, v| v > 1 })
        p(h.reject { |k, v| v > 1 })
        p(h.transform_values { |v| v * 10 })
        p(h.any? { |k, v| v > 1 })
        p h.count
        p(h.min_by { |k, v| v })
        p h.value?(2)
        p h.value?(9)
        p({ x: { y: 5 } }.dig(:x, :y))
        h2 = { a: 1 }
        h2.update({ b: 2 })
        p h2
        acc = []
        h.each_key { |k| acc << k }
        h.each_value { |v| acc << v }
        p acc
        p h == { b: 2, a: 1 }
        p h == { a: 1 }
        r = (1..10)
        p r.sum
        p r.min
        p r.max
        p r.count
        p r.size
        p r.first(3)
        p r.last(3)
        p (1...5).size
        acc2 = []
        (1..10).step(3) { |i| acc2 << i }
        p acc2
        p ("a".."e").to_a
        p ("a".."e").include?("c")
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "{a: 1, b: 2, c: 3}\n{a: 10, b: 2}\n[[:a, 1], [:b, 2]]\n{1 => :a, 2 => :b}\n\
         :b\nnil\n1\n0\nKeyError: key not found: :x\n\"no x\"\n\
         {b: 2}\n{a: 1}\n{a: 10, b: 20}\ntrue\n2\n[:a, 1]\ntrue\nfalse\n5\n\
         {a: 1, b: 2}\n[:a, :b, 1, 2]\ntrue\nfalse\n\
         55\n1\n10\n10\n10\n[1, 2, 3]\n[8, 9, 10]\n4\n[1, 4, 7, 10]\n\
         [\"a\", \"b\", \"c\", \"d\", \"e\"]\ntrue\n"
    );
}

// ---------------------------------------------------------------------------
// Phase 17.1-F -- Enumerable Tier A breadth (the enum.c architecture: every
// method drives the receiver's own #each). Oracle: ruby 4.0.5.
// ---------------------------------------------------------------------------

#[test]
fn enumerable_breadth_matches_the_oracle() {
    let result = support::run_ruby(
        r#"
        p [3, 1, 2].sort_by { |x| -x }
        p [1, 2, 3, 4].min_by { |x| (x - 3).abs }
        p [1, 2, 3, 4].max_by { |x| (x % 3) }
        p [3, 1, 2].minmax
        p (1..6).group_by { |x| x % 3 }
        p [1, 2, 3, 4].partition { |x| x.even? }
        p [[1, 2], [3, 4]].flat_map { |a| a }
        p [1, 2, 3, 4, 5].filter_map { |x| x * 2 if x.odd? }
        acc = []
        (1..7).each_slice(3) { |s| acc << s }
        p acc
        acc2 = []
        (1..4).each_cons(2) { |c| acc2 << c }
        p acc2
        p [1, 2, 3].each_with_object([]) { |x, memo| memo << x * 10 }
        p [1, 2, 3, 4].take_while { |x| x < 3 }
        p [1, 2, 3, 4].drop_while { |x| x < 3 }
        p ["a", "b", "a", "c", "a"].tally
        p [1, 2, 2, 3].uniq
        p({ a: 1, b: 2 }.sort_by { |k, v| -v })
        acc3 = []
        [1, 2, 3].reverse_each { |x| acc3 << x }
        p acc3
        p [[:a, 1], [:b, 2]].to_h
        p({ a: 1 }.flat_map { |k, v| [k, v] })
        p (1..4).find_index { |x| x > 2 }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[3, 2, 1]\n3\n2\n[1, 3]\n{1 => [1, 4], 2 => [2, 5], 0 => [3, 6]}\n\
         [[2, 4], [1, 3]]\n[1, 2, 3, 4]\n[2, 6, 10]\n\
         [[1, 2, 3], [4, 5, 6], [7]]\n[[1, 2], [2, 3], [3, 4]]\n[10, 20, 30]\n\
         [1, 2]\n[3, 4]\n{\"a\" => 3, \"b\" => 1, \"c\" => 1}\n[1, 2, 3]\n\
         [[:b, 2], [:a, 1]]\n[3, 2, 1]\n{a: 1, b: 2}\n[:a, 1]\n2\n"
    );
}

// ---------------------------------------------------------------------------
// Phase 17.1-G -- Kernel breadth: the multi-arg print family, the sprintf
// engine, rand/srand (property-asserted: our PRNG is deliberately not
// MT19937), catch/throw, and the user-def-wins interception order fix.
// Oracle: ruby 4.0.5.
// ---------------------------------------------------------------------------

#[test]
fn kernel_breadth_matches_the_oracle() {
    let result = support::run_ruby(
        r#"
        puts [1, [2, nil]], "x"
        puts
        r = p 1, "two"
        p r
        r2 = p 5
        p r2
        print "a", 1, "\n"
        puts format("%s scored %05.1f%%", "Bob", 92.5)
        printf("%d-%x\n", 255, 255)
        srand(42)
        v = rand(10)
        puts v.between?(0, 9)
        puts rand.between?(0.0, 1.0)
        puts rand(10).class
        old = srand(7)
        puts old
        caught = catch(:done) do
          [1, 2, 3].each { |i| throw :done, i * 10 if i == 2 }
          :never
        end
        p caught
        p(catch(:t) { 5 })
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "1\n2\n\nx\n\n1\n\"two\"\n[1, \"two\"]\n5\n5\na1\n\
         Bob scored 092.5%\n255-ff\n\
         true\ntrue\nInteger\n42\n20\n5\n"
    );
}

/// The interception-order fix: a user-defined sibling `puts` now WINS over
/// the Kernel function (real Ruby's rule; the old intercept-first order
/// was a latent bug).
#[test]
fn a_user_defined_puts_wins_over_the_kernel_function() {
    let result = support::run_ruby(
        r#"
        class Logger
          def puts(msg)
            $stdout_lines = 1
            "logged: #{msg}"
          end
          def run
            puts("hi")
          end
        end

        v = Logger.new.run
        p v
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"logged: hi\"\n");
}

// ---------------------------------------------------------------------------
// Phase 17.1-H -- Struct: compile-time class synthesis. Oracle: ruby 4.0.5.
// ---------------------------------------------------------------------------

/// `Point = Struct.new(:x, :y)` synthesizes an ordinary `class Point <
/// Struct` at lowering time: accessors, positional init (nil-filled),
/// members/to_a/to_h/==/[]/[]=/each_pair/inspect, and Enumerable through
/// the real ancestor chain. Plus keyword_init and the block-with-methods
/// form.
#[test]
fn struct_synthesis_matches_the_oracle() {
    let result = support::run_ruby(
        r#"
        Point = Struct.new(:x, :y)
        pt = Point.new(1, 2)
        p pt
        puts pt.x
        pt.y = 9
        p pt.to_a
        p pt.to_h
        p pt.members
        p pt == Point.new(1, 9)
        p pt == Point.new(1, 2)
        p pt[0]
        p pt[:y]
        p pt["x"]
        p pt[-1]
        pt[1] = 20
        p pt.y
        p pt.length
        p pt.map { |v| v }
        p pt.select { |v| v.is_a?(Integer) }
        acc = []
        pt.each_pair { |k, v| acc << [k, v] }
        p acc
        p Point.ancestors.include?(Struct)
        p Point.ancestors.include?(Enumerable)
        Label = Struct.new(:text, keyword_init: true)
        l = Label.new(text: "hi")
        p l
        Pair = Struct.new(:a, :b) do
          def total
            a + b
          end
        end
        p Pair.new(3, 4).total
        p Point.new(5).y
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "#<struct Point x=1, y=2>\n1\n[1, 9]\n{x: 1, y: 9}\n[:x, :y]\ntrue\nfalse\n\
         1\n9\n1\n9\n20\n2\n[1, 20]\n[1, 20]\n[[:x, 1], [:y, 20]]\ntrue\ntrue\n\
         #<struct Label text=\"hi\">\n7\nnil\n"
    );
}

/// The clean rejections: `Struct.new` outside a constant assignment, and
/// non-symbol members (both surface as lowering errors).
#[test]
fn struct_new_rejections_are_clean_errors() {
    let err = spinelc::compile_to_rust("s = Struct.new(:a)\n").unwrap_err();
    assert!(err.contains("outside a constant assignment"), "{err}");
    let err = spinelc::compile_to_rust("P = Struct.new(\"Name\", :a)\n").unwrap_err();
    assert!(err.contains("must be literal symbols"), "{err}");
}

// -- Phase 17.2: the fiber-backed Enumerator (per CRuby's enumerator.c).
// Every positive expectation below is oracle-verified against ruby 4.0.5.

/// The keystone: external iteration over a method-backed enumerator --
/// `next` advances a real fiber, `peek` caches without consuming, the
/// classic `loop { e.next }` idiom terminates via StopIteration and
/// returns the underlying `each`'s result, and a rescued StopIteration
/// exposes `message`/`result`.
#[test]
fn external_iteration_drives_a_real_fiber() {
    let result = run_ruby(
        r#"
        e = [10, 20, 30].each
        r = loop do
          puts e.next
        end
        p r
        e2 = [1].each
        e2.next
        begin
          e2.next
        rescue StopIteration => ex
          puts "rescued: #{ex.message}"
          p ex.result
        end
        g = [7, 8].each
        p g.peek
        p g.peek
        p g.next
        p g.next
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "10\n20\n30\n[10, 20, 30]\nrescued: iteration reached an end\n[1]\n7\n7\n7\n8\n"
    );
}

/// `loop`'s full StopIteration contract (CRuby kernel.rb:151): a manual
/// `raise StopIteration` returns nil (no result set), `break value` still
/// carries its value out, and every OTHER exception propagates.
#[test]
fn loop_swallows_stop_iteration_and_returns_its_result() {
    let result = run_ruby(
        r#"
        p(loop { raise StopIteration })
        p(loop { break 42 })
        begin
          loop { raise ArgumentError, "boom" }
        rescue ArgumentError => e
          puts "arg: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "nil\n42\narg: boom\n");
}

/// `Enumerator.new { |y| ... }` -- the generator/Yielder pair, and the
/// packing truth table: `next_values` preserves yield arity exactly,
/// `next` collapses through ary2sv (`yield`->nil, `yield nil`->nil,
/// `yield 1,2`->[1,2], `yield [3,4]`->[3,4]); the block's return value is
/// the StopIteration result; `rewind` restarts from the top.
#[test]
fn enumerator_new_yields_through_a_yielder_with_cruby_packing() {
    let result = run_ruby(
        r#"
        g = Enumerator.new do |y|
          y.yield
          y.yield nil
          y.yield 1, 2
          y << [3, 4]
          :fin
        end
        p g.next_values
        p g.next_values
        p g.next_values
        p g.next_values
        g.rewind
        p g.next
        p g.next
        p g.next
        p g.next
        begin
          g.next
        rescue StopIteration => e
          p e.result
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[]\n[nil]\n[1, 2]\n[[3, 4]]\nnil\nnil\n[1, 2]\n[3, 4]\n:fin\n"
    );
}

/// An INFINITE generator proves the fiber suspend is real: `next`/`peek`
/// advance lazily, and `take`/`first` terminate via the Break-based early
/// exit (internal iteration restarts from scratch each time -- CRuby).
#[test]
fn infinite_generators_iterate_lazily() {
    let result = run_ruby(
        r#"
        inf = Enumerator.new do |y|
          n = 0
          loop do
            y << n
            n += 1
          end
        end
        p inf.next
        p inf.next
        p inf.peek
        p inf.next
        p inf.take(3)
        p inf.first(2)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "0\n1\n2\n2\n[0, 1, 2]\n[0, 1]\n");
}

/// A mid-iteration exception propagates out of `next`; the NEXT `next`
/// re-inits the dead fiber and RESTARTS the iteration (CRuby's
/// get_next_values rule, oracle-verified).
#[test]
fn a_failed_iteration_propagates_then_restarts() {
    let result = run_ruby(
        r#"
        e = Enumerator.new do |y|
          y << 1
          raise ArgumentError, "mid"
        end
        p e.next
        begin
          e.next
        rescue ArgumentError => ex
          puts "propagated: #{ex.message}"
        end
        p e.next
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\npropagated: mid\n1\n");
}

/// The blockless breadth: iteration methods across
/// Integer/Range/Hash/String/Array answer real Enumerators (inspect shows
/// the captured receiver/method/args; `each` re-invokes the source).
#[test]
fn blockless_breadth_returns_enumerators_everywhere() {
    let result = run_ruby(
        r#"
        p 5.times.to_a
        p 2.upto(5).to_a
        p 5.downto(2).inspect
        p (1..10).step(3).to_a
        p({ a: 1, b: 2 }.each_value.to_a)
        p "hey".each_char.to_a
        p [3, 1].sort_by
        p 1.step(2.0, 0.5).to_a
        p 5.then.next
        p({ x: 1 }.each.next)
        p [1, 2, 3].each_slice(2).to_a
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[0, 1, 2, 3, 4]\n[2, 3, 4, 5]\n\"#<Enumerator: 5:downto(2)>\"\n[1, 4, 7, 10]\n\
         [1, 2]\n[\"h\", \"e\", \"y\"]\n#<Enumerator: [3, 1]:sort_by>\n[1.0, 1.5, 2.0]\n\
         5\n[:x, 1]\n[[1, 2], [3]]\n"
    );
}

/// Chaining: with_index/each_with_index wrap (blockless) and drive
/// (block-given), an Enumerator is itself Enumerable (reduce/sort/select
/// arrive via the real ancestor chain), and with_object threads its memo.
#[test]
fn enumerators_chain_and_are_enumerable() {
    let result = run_ruby(
        r#"
        e = ["a", "b", "c"].each_with_index
        p e.class
        p e.to_a
        p [10, 20].map.with_index { |x, i| x * i }
        p [10, 20].each.with_index(5).to_a
        p [4, 2, 6].each.reduce { |a, b| a + b }
        p [4, 2, 6].each.sort
        p [1, 2, 3, 4].each.select { |x| x.even? }
        p [1, 2].each.with_object([]) { |x, memo| memo << x * 2 }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Enumerator\n[[\"a\", 0], [\"b\", 1], [\"c\", 2]]\n[0, 20]\n\
         [[10, 5], [20, 6]]\n12\n[2, 4, 6]\n[2, 4]\n[2, 4]\n"
    );
}

/// `size` never iterates: receiver-derived for the same-size set, computed
/// for times/upto/each_slice, the stored hint for Enumerator.new, nil when
/// unknowable.
#[test]
fn enumerator_size_is_lazy_and_oracle_faithful() {
    let result = run_ruby(
        r#"
        p [1, 2, 3].each.size
        p [1, 2, 3].select.size
        p 5.times.size
        p 2.upto(9).size
        p [1, 2, 3].each_slice(2).size
        p "abc".each_char.size
        p Enumerator.new { |y| y << 1 }.size
        p Enumerator.new(4) { |y| y << 1 }.size
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n3\n5\n8\n2\n3\nnil\n4\n");
}

/// `to_enum`/`enum_for` on a user class -- the real-Ruby
/// `return to_enum(:each) unless block_given?` pattern -- and the
/// synthesized Struct `each` using exactly that pattern.
#[test]
fn to_enum_works_on_user_classes_and_structs() {
    let result = run_ruby(
        r##"
        class Deck
          include Enumerable
          def initialize(cards)
            @cards = cards
          end
          def each
            return to_enum(:each) unless block_given?
            i = 0
            while i < @cards.length
              yield @cards[i]
              i += 1
            end
            self
          end
        end
        d = Deck.new([5, 3, 9])
        e = d.each
        p e.class
        p e.next
        p d.each.sort
        p d.map.with_index { |c, i| "#{i}:#{c}" }
        p 7.to_enum(:upto, 9).to_a
        P17 = Struct.new(:x, :y)
        pt = P17.new(1, 2)
        en = pt.each
        p en.class
        p en.next
        p en.to_a
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Enumerator\n5\n[3, 5, 9]\n[\"0:5\", \"1:3\", \"2:9\"]\n[7, 8, 9]\n\
         Enumerator\n1\n[1, 2]\n"
    );
}

/// Enumerator identity/copy semantics: blockless `each` returns SELF,
/// pre-iteration dup is a fresh enumerator over the same source, breaking
/// out of an external loop leaves the enumerator resumable.
#[test]
fn enumerator_identity_and_resumability() {
    let result = run_ruby(
        r#"
        e = [1, 2].each
        p e.each.equal?(e)
        d = e.dup
        p d.class
        p e.next
        p d.next
        g = [1, 2, 3].each
        loop do
          v = g.next
          break if v == 2
        end
        p g.next
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nEnumerator\n1\n1\n3\n");
}

// -- Phase 18: Ruby::Box. Oracle: `RUBY_BOX=1 ruby -W:no-experimental`
// (verified per the plan's research contract; where our AOT model
// deliberately diverges -- handle inspect suffix, compile-time rejections
// -- the expectation below states OUR documented behavior).

/// The keystone: a box-required file's classes are DISTINCT from main's
/// (same file, different class objects), builtins are SHARED
/// (`box::String == String`), a box top-level constant is readable
/// externally, and box gvars are invisible in main -- all per the CRuby
/// box model.
#[test]
fn box_isolation_and_shared_builtins() {
    let result = run_ruby_project(
        &[
            (
                "widget.rb",
                "class Widget\n  def hi\n    \"box widget\"\n  end\nend\nWIDGET_CONST = 99\n$box_g = 5\n",
            ),
            (
                "main.rb",
                "class Widget\n  def hi\n    \"main widget\"\n  end\nend\n\
                 box = Ruby::Box.new\n\
                 box.require_relative \"widget\"\n\
                 p Widget.new.hi\n\
                 w = box::Widget.new\n\
                 p w.hi\n\
                 p box::Widget == Widget\n\
                 p box::String == String\n\
                 p box::WIDGET_CONST\n\
                 p $box_g\n\
                 p box\n",
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"main widget\"\n\"box widget\"\nfalse\ntrue\n99\nnil\n#<Ruby::Box:1>\n"
    );
}

/// Per-box re-execution (the same file `box.require`d into two boxes runs
/// twice, with independent class-variable state per box), and per-box
/// builtin MONKEYPATCHES: a box's `String#blank?` resolves from that box's
/// code while main's `"foo".blank?` stays a NoMethodError -- the docs'
/// motivating example, dispatch by DEFINING box.
#[test]
fn boxes_reexecute_files_and_patch_builtins_privately() {
    let result = run_ruby_project(
        &[
            (
                "blank.rb",
                "class String\n  def blank?\n    strip.empty?\n  end\nend\n\
                 class Foo\n  def self.blank_one?\n    \"   \".blank?\n  end\nend\n\
                 class Counter\n  @@count = 0\n  def self.bump\n    @@count += 1\n  end\n  def self.count\n    @@count\n  end\nend\n\
                 puts \"loaded\"\n",
            ),
            (
                "main.rb",
                "box = Ruby::Box.new\n\
                 box.require_relative \"blank\"\n\
                 box2 = Ruby::Box.new\n\
                 box2.require_relative \"blank\"\n\
                 p box::Foo.blank_one?\n\
                 begin\n  \"foo\".blank?\nrescue NoMethodError => e\n  puts \"main: #{e.message}\"\nend\n\
                 box::Counter.bump\n\
                 box::Counter.bump\n\
                 box2::Counter.bump\n\
                 p box::Counter.count\n\
                 p box2::Counter.count\n",
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "loaded\nloaded\ntrue\nmain: undefined method 'blank?' for an instance of String\n2\n1\n"
    );
}

/// `Box#eval`: a statement-position eval may DEFINE classes in the box; an
/// expression-position eval returns a value whose static type is the box's
/// own class (Path 1 dispatch on a cross-boundary instance); two boxes
/// eval'ing the same class name get distinct classes.
#[test]
fn box_eval_defines_and_returns_across_the_boundary() {
    let result = run_ruby(
        r#"
        box = Ruby::Box.new
        box.eval("class Gadget; def spin; 'spinning'; end; end")
        g = box.eval("Gadget.new")
        p g.spin
        p box::Gadget.new.spin
        box2 = Ruby::Box.new
        box2.eval("class Gadget; def spin; 'other'; end; end")
        p box2::Gadget.new.spin
        p(box::Gadget == box2::Gadget)
        p box.eval("1 + 2")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"spinning\"\n\"spinning\"\n\"other\"\nfalse\n3\n"
    );
}

/// Exceptions cross the boundary as plain references: a box-defined
/// `BoxError < StandardError` raised from box code is rescuable in main
/// through the SHARED bootstrap superclass chain, and `e.class` names it.
#[test]
fn box_exceptions_are_rescuable_in_main() {
    let result = run_ruby_project(
        &[
            (
                "thrower.rb",
                "class BoxError < StandardError\nend\n\
                 class Thrower\n  def self.go\n    raise BoxError, \"from the box\"\n  end\nend\n",
            ),
            (
                "main.rb",
                "box = Ruby::Box.new\n\
                 box.require_relative \"thrower\"\n\
                 begin\n  box::Thrower.go\nrescue StandardError => e\n  puts \"rescued: #{e.message} (#{e.class})\"\nend\n",
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "rescued: from the box (BoxError)\n");
}

/// Gvar isolation is BIDIRECTIONAL (separate per-box tables, no fallback):
/// a box reads nil for main's `$g`, and a box's write never reaches main.
#[test]
fn box_globals_are_fully_separate() {
    let result = run_ruby(
        r#"
        $g = "main value"
        box = Ruby::Box.new
        p box.eval("$g")
        box.eval("$g = 'box value'")
        p $g
        p box.eval("$g")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "nil\n\"main value\"\n\"box value\"\n");
}

/// The clean rejections: `Ruby::Box.current`-family reflection, box
/// operations outside their recognized positions, and expression-position
/// eval defining classes.
#[test]
fn ruby_box_rejections_are_clean_errors() {
    let err = spinelc::compile_to_rust("p Ruby::Box.current\n").unwrap_err();
    assert!(err.contains("no compile-time meaning"), "{err}");
    let err = spinelc::compile_to_rust("box = Ruby::Box.new\nx = [box.require(\"f\")]\n")
        .unwrap_err();
    assert!(err.contains("top-level statement"), "{err}");
    let err =
        spinelc::compile_to_rust("box = Ruby::Box.new\nv = box.eval(\"class X; end\")\n")
            .unwrap_err();
    assert!(err.contains("class"), "{err}");
    let err = spinelc::compile_to_rust("box = Ruby::Box.new\nbox.eval(1)\n").unwrap_err();
    assert!(err.contains("non-literal"), "{err}");
}

/// A main-only class/constant is INVISIBLE inside a box (boxes dup from
/// MASTER, not main): resolving it inside box.eval raises NameError-shaped
/// failures rather than leaking main's definitions.
#[test]
fn main_definitions_are_invisible_inside_a_box() {
    let result = run_ruby(
        r#"
        MAIN_ONLY = 42
        box = Ruby::Box.new
        box.eval("begin; p MAIN_ONLY; rescue NameError => e; puts 'invisible'; end")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "invisible\n");
}

// --- Top-level method definitions (G0) ---------------------------------
//
// A top-level `def` is a PRIVATE instance method on `Object` (real Ruby's
// rule): registered on arena slot 0, materialized by `analyze::mro` into
// every user class (so any method body reaches it via implicit self, with
// `@ivar`s landing on the calling class's own struct), and emitted through
// the builtin-reopen container (`__bm_Object`) whose copies dispatch on
// the runtime `main` object at top-level call sites.

#[test]
fn top_level_def_defines_and_calls() {
    let result = run_ruby(
        r#"
        def greet(name, punct = "!")
          "hi #{name}#{punct}"
        end
        puts greet("world")
        puts greet("you", "?")
        "#,
    );
    assert_eq!(result.stdout, "hi world!\nhi you?\n");
    assert_eq!(result.stderr, "");
}

#[test]
fn top_level_endless_def() {
    let result = run_ruby("def double(x) = x * 2\nputs double(21)\n");
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn top_level_def_recursion_and_mutual_calls() {
    let result = run_ruby(
        r#"
        def fib(n)
          n < 2 ? n : fib(n - 1) + fib(n - 2)
        end
        def announce(n)
          puts "fib(#{n}) = #{fib(n)}"
        end
        announce(10)
        "#,
    );
    assert_eq!(result.stdout, "fib(10) = 55\n");
}

#[test]
fn top_level_def_reachable_from_instance_and_class_methods() {
    let result = run_ruby(
        r#"
        def helper(x)
          x + 1
        end
        class Widget
          def go
            helper(4)
          end
          def self.direct
            helper(10)
          end
        end
        puts Widget.new.go
        puts Widget.direct
        "#,
    );
    assert_eq!(result.stdout, "5\n11\n");
    assert_eq!(result.stderr, "");
}

#[test]
fn top_level_def_with_block_and_yield() {
    let result = run_ruby(
        r#"
        def twice
          yield 1
          yield 2
        end
        twice { |i| puts "got #{i}" }
        "#,
    );
    assert_eq!(result.stdout, "got 1\ngot 2\n");
}

#[test]
fn top_level_def_called_from_a_lambda() {
    let result = run_ruby(
        r#"
        def base
          40
        end
        f = ->(x) { base + x }
        puts f.call(2)
        "#,
    );
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn top_level_def_overrides_a_kernel_function() {
    // Sibling/top-level resolution runs BEFORE the Kernel function set --
    // real Ruby's rule (a user `def rand` wins over Kernel#rand).
    let result = run_ruby(
        r#"
        def rand
          7
        end
        puts rand
        "#,
    );
    assert_eq!(result.stdout, "7\n");
}

#[test]
fn top_level_def_last_def_wins() {
    let result = run_ruby(
        r#"
        def v
          1
        end
        def v
          2
        end
        puts v
        "#,
    );
    assert_eq!(result.stdout, "2\n");
}

#[test]
fn top_level_self_is_the_main_object() {
    let result = run_ruby("puts self.is_a?(Object)\n");
    assert_eq!(result.stdout, "true\n");
}

#[test]
fn top_level_def_self_is_a_clean_error() {
    let err = spinelc::compile_to_rust("def self.x\n  1\nend\n").unwrap_err();
    assert!(err.contains("`def self.name` at the top level"), "{err}");
}

#[test]
fn top_level_def_with_ivar_is_a_clean_error() {
    // `Object`'s own copy dispatches on the ivar-less `main` object; the
    // clean rejection names the top-level shape (spike scope).
    let err = spinelc::compile_to_rust("def bump\n  @count = 1\nend\nbump\n").unwrap_err();
    assert!(err.contains("top-level method"), "{err}");
}

#[test]
fn shift_operator_boxes_an_object_typed_argument() {
    // `junk << Trash.new(1)` on a Poly receiver: the numeric-op fallback's
    // match scrutinee must box the unboxed `Arc<Concrete>` argument
    // (found by the conformance corpus's argv_gc as a FAIL_RUSTC).
    let result = run_ruby(
        r#"
        class Trash
          def initialize(n)
            @n = n
          end
          attr_reader :n
        end
        def build
          junk = []
          junk << Trash.new(7)
          junk
        end
        puts build.length
        puts build[0].n
        "#,
    );
    assert_eq!(result.stdout, "1\n7\n");
    assert_eq!(result.stderr, "");
}

#[test]
fn argv_is_seeded_from_the_command_line() {
    // No args in this harness: ARGV exists and is empty (CRuby startup
    // parity; argv-carrying coverage lives in the conformance corpus).
    let result = run_ruby("p ARGV\nputs ARGV.length\n");
    assert_eq!(result.stdout, "[]\n0\n");
}

// --- G0 FAIL_RUSTC sweep: Object-boxing + emission fixes ---------------
//
// Each of these reproduces a generated-Rust compile failure (FAIL_RUSTC)
// found by the conformance corpus: spinelc accepted the program but
// emitted ill-typed Rust.

#[test]
fn if_arms_with_different_object_classes_box_to_ruby_value() {
    let result = run_ruby(
        r#"
        class A
          def tag = "a"
        end
        class B
          def tag = "b"
        end
        pick = true
        x = pick ? A.new : B.new
        puts x.send(:tag)
        "#,
    );
    assert_eq!(result.stdout, "a\n");
}

#[test]
fn return_of_an_object_typed_value_boxes() {
    let result = run_ruby(
        r#"
        class Box
          def initialize(tag)
            @tag = tag
          end
          attr_reader :tag
        end
        def find_or_nil(want)
          if want == "yes"
            return Box.new("found")
          end
          nil
        end
        r = find_or_nil("yes")
        puts r.send(:tag)
        puts find_or_nil("no").inspect
        "#,
    );
    assert_eq!(result.stdout, "found\nnil\n");
}

#[test]
fn boolean_operators_box_object_typed_operands() {
    let result = run_ruby(
        r#"
        class Flag; end
        f = Flag.new
        puts (f && 1).inspect
        puts (nil || Flag.new).class
        puts f ? "truthy" : "falsy"
        puts (!f).inspect
        "#,
    );
    assert_eq!(result.stdout, "1\nFlag\ntruthy\nfalse\n");
}

#[test]
fn yield_boxes_an_object_typed_argument() {
    // (`Builder.new { }` can't be used here: `.new` doesn't forward its
    // block to `initialize` yet -- a separate, pre-existing gap.)
    let result = run_ruby(
        r#"
        class Builder
          def run
            yield self
          end
          def ping = "pong"
        end
        Builder.new.run { |b| puts b.send(:ping) }
        "#,
    );
    assert_eq!(result.stdout, "pong\n");
}

#[test]
fn block_params_are_reassignable() {
    let result = run_ruby(
        r#"
        1.upto(2) { |n| n = n + 10; puts n }
        3.times { |i| i = i * 2; print i }
        puts
        "#,
    );
    assert_eq!(result.stdout, "11\n12\n024\n");
}

#[test]
fn rescue_matches_an_included_module() {
    let result = run_ruby(
        r#"
        module Alertable; end
        class AppError < StandardError
          include Alertable
        end
        begin
          raise AppError, "direct"
        rescue Alertable => e
          puts "alertable: #{e.message}"
        end
        "#,
    );
    assert_eq!(result.stdout, "alertable: direct\n");
}

#[test]
fn case_in_matches_float_and_builtin_classes_on_poly_values() {
    let result = run_ruby(
        r#"
        def describe(val)
          case val
          in Integer then "int"
          in Float then "float"
          in String then "str"
          else "other"
          end
        end
        puts describe(1)
        puts describe(2.5)
        puts describe("s")
        puts describe(:sym)
        "#,
    );
    assert_eq!(result.stdout, "int\nfloat\nstr\nother\n");
}

#[test]
fn fallible_class_body_constant_and_method_default_args() {
    // `CONST = <fallible expr>` in a class body and `def m(a = <fallible>)`
    // both previously emitted `?` outside a Result context.
    let result = run_ruby(
        r#"
        def source
          41
        end
        class Config
          LIMIT = [1, 2, 3].sum
        end
        def bump(a = source + 1)
          a
        end
        puts Config::LIMIT
        puts bump
        puts bump(5)
        "#,
    );
    assert_eq!(result.stdout, "6\n42\n5\n");
}

#[test]
fn kernel_proc_warn_method_name_and_bare_new() {
    let result = run_ruby(
        r#"
        sq = proc { |x| x * x }
        puts sq.call(5)
        warn "to stderr"
        def who = __method__
        p who
        GC.start
        class F
          def self.create
            new
          end
        end
        puts F.create.class
        "#,
    );
    assert_eq!(result.stdout, "25\n:who\nF\n");
    assert_eq!(result.stderr, "to stderr\n");
}

#[test]
fn stdout_stderr_constants_and_globals() {
    let result = run_ruby(
        r#"
        STDOUT.puts "via const"
        $stdout.puts "via global"
        STDERR.puts "err const"
        $stderr.print "err global\n"
        puts STDOUT.write("abc\n")
        $stdout = STDERR
        puts "redirected"
        $stdout = STDOUT
        puts "back"
        puts STDOUT.inspect
        "#,
    );
    assert_eq!(result.stdout, "via const\nvia global\nabc\n4\nback\n#<IO:<STDOUT>>\n");
    assert_eq!(result.stderr, "err const\nerr global\nredirected\n");
}

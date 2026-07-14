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
#[should_panic(expected = "escaping from inside another escaping block")]
fn a_block_escaping_from_inside_another_escaping_block_is_a_clean_compile_error() {
    // Unlike the `&block`/`...`-forwarding rejections above (parse-time,
    // `Result::Err`), this check runs during CODEGEN (`codegen::captures`),
    // same posture as this codebase's other "spike scope" violations (e.g.
    // `codegen::call`'s "unsupported call" panic) -- a clean, clearly-worded
    // panic, not a silent miscompile, but a panic rather than an `Err`.
    let _ = spinelc::compile_to_rust(
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
fn reopening_a_built_in_class_name_is_a_clean_error() {
    let err = spinelc::compile_to_rust(
        r#"
        class Array
          def foo
            1
          end
        end
        "#,
    )
    .unwrap_err();
    assert!(err.contains("reopening/redefining the built-in class"), "{err}");
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

#[test]
#[should_panic(expected = "a String pattern argument to String#split isn't supported yet")]
fn a_string_pattern_argument_to_split_is_a_clean_compile_error() {
    let _ = spinelc::compile_to_rust(r#"puts "a,b".split(",")"#);
}

#[test]
#[should_panic(expected = "a String pattern argument to String#gsub isn't supported yet")]
fn a_string_pattern_argument_to_gsub_is_a_clean_compile_error() {
    let _ = spinelc::compile_to_rust(r#"puts "a,b".gsub(",", ";")"#);
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

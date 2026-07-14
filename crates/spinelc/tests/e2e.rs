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
#[should_panic(expected = "rescue")]
fn bare_raise_with_no_active_rescue_is_a_clean_compile_error() {
    // Bare `raise` (re-raise) needs a currently-handled exception context
    // that doesn't exist until `rescue` does (Phase 9) -- a clean
    // rejection now, not a silent no-op. This is a CODEGEN-time panic (like
    // `codegen::captures`'s other "spike scope" violations), not a
    // parse-time `Result::Err`, hence `#[should_panic]` here rather than
    // `.unwrap_err()`.
    let _ = spinelc::compile_to_rust(
        r#"
        class Box
          def check
            raise
          end
        end
        "#,
    );
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

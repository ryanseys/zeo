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
fn class_shift_self_at_top_level_is_a_clean_error() {
    // `class << obj` on a non-`self` receiver now works (#97 F3), but
    // `class << self` at an EXPRESSION/statement position (top level, method
    // body) still isn't supported -- there's no compile-time class to attach
    // the reopened singleton to. A clean compile error, never a panic.
    let err = spinelc::compile_to_rust(
        "class << self\n  def hi; 1; end\nend\n",
    )
    .unwrap_err();
    assert!(
        err.contains("class << self"),
        "expected a `class << self` scope-cut error, got: {err}"
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
fn eval_of_invalid_literal_source_raises_a_catchable_syntax_error() {
    // A literal `eval("...")` whose source doesn't parse no longer fails the
    // COMPILE (#97 stage 2): it falls through to the runtime eval VM and raises
    // a catchable SyntaxError, exactly as CRuby does.
    let result = run_ruby(r#"begin; eval("1 +"); rescue SyntaxError; puts "caught"; end"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught\n");
}

#[test]
fn eval_of_a_non_literal_argument_runs_in_the_vm() {
    // A non-literal source is no longer rejected at compile time; it runs
    // through the eval VM. (`y` is interpolated INTO the source string, not
    // referenced inside the eval.)
    let result = run_ruby("y = 40\nputs eval(\"#{y} + 2\")\n");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn eval_of_a_class_definition_is_not_a_compile_error() {
    // The inline path can't express a top-level `class`/`def`, so a literal
    // `eval("class Foo; end")` falls through to the runtime VM instead of
    // failing the compile. (Actually DEFINING a class inside eval is a later
    // increment; the point here is that it compiles.)
    assert!(spinelc::compile_to_rust(r#"eval("class Foo; end")"#).is_ok());
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
fn forwarding_params_forward_positionals_keywords_and_block() {
    // `...` is real now (G3): it desugars to internal `*__fwd_rest,
    // **__fwd_kw, &__fwd_blk` params referenced by the call-site `...`.
    let result = run_ruby(
        r##"
        def target(a, b, mode: "m")
          r = yield if block_given?
          "#{a}/#{b}/#{mode}/#{r.inspect}"
        end
        def fwd(...)
          target(...)
        end
        puts fwd(1, 2, mode: "z") { "blk" }
        "##,
    );
    assert_eq!(result.stdout, "1/2/z/\"blk\"\n");
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
fn runtime_define_method_in_a_class_body_loop() {
    // #97 F2: a class-body `each` loop (F2a: class-body statements now run)
    // whose `define_method` block captures the loop variable (F2b: nested
    // block capturing the outer block's own local) installs each method into
    // the runtime overlay, reachable on every instance and by `respond_to?`.
    let result = run_ruby(
        r##"
        class Robot
          [:beep, :boop].each do |sound|
            define_method(sound) { "#{sound}!" }
          end
        end
        r = Robot.new
        puts r.beep
        puts r.boop
        puts r.respond_to?(:beep)
        puts r.respond_to?(:whir)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "beep!\nboop!\ntrue\nfalse\n");
}

#[test]
fn define_singleton_method_on_object_and_class() {
    // #97 F2: a per-object singleton (only that object responds) and a
    // class-level singleton method (a class method).
    let result = run_ruby(
        r#"
        class Widget; end
        a = Widget.new
        a.define_singleton_method(:special) { "just me" }
        puts a.special
        puts Widget.new.respond_to?(:special)
        Widget.define_singleton_method(:factory) { "built" }
        puts Widget.factory
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "just me\nfalse\nbuilt\n");
}

#[test]
fn class_new_creates_an_anonymous_runtime_class() {
    // #97 F4: `Class.new(Super) { ... }` mints a runtime class. The block body
    // runs against the new class (`define_method` and `def` both install on
    // it); a constant binding names it; is_a?/instance_of?/superclass and a
    // runtime superclass chain all resolve.
    let result = run_ruby(
        r#"
        Widget = Class.new do
          define_method(:kind) { "widget" }
          def size
            10
          end
        end
        w = Widget.new
        puts w.kind
        puts w.size
        puts w.is_a?(Widget)
        puts Widget.name
        Gadget = Class.new(Widget) do
          define_method(:extra) { "gadget" }
        end
        g = Gadget.new
        puts g.kind
        puts g.extra
        puts g.is_a?(Widget)
        puts g.instance_of?(Widget)
        puts Gadget.superclass.name
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "widget\n10\ntrue\nWidget\nwidget\ngadget\ntrue\nfalse\nWidget\n"
    );
}

#[test]
fn def_on_an_object_defines_a_per_object_singleton() {
    // #97 F3: `def obj.name` and `class << obj` install per-object singleton
    // methods (identity-keyed overlay), reaching `@ivar`/`self`/params and
    // answering `respond_to?` only on that object.
    let result = run_ruby(
        r##"
        obj = Object.new
        obj.instance_variable_set(:@n, 10)
        def obj.double
          @n * 2
        end
        class << obj
          def plus(k)
            @n + k
          end
        end
        puts obj.double
        puts obj.plus(5)
        puts obj.respond_to?(:double)
        puts obj.respond_to?(:plus)
        puts Object.new.respond_to?(:double)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "20\n15\ntrue\ntrue\nfalse\n");
}

#[test]
fn a_nested_block_captures_the_outer_blocks_own_local() {
    // #97 F2b: the INNER block reads the OUTER block's own param `n`. The outer
    // block promotes `n` to a shared `Arc<Mutex>` cell (see
    // `codegen::call::emit_proc_or_lambda_value`'s `nested_captured`), so the
    // inner `move` closure clone-captures it -- previously a clean rejection,
    // now the real Ruby behavior. Oracle: n=1 -> 3+1,4+1; n=2 -> 3+2,4+2.
    let result = run_ruby(
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
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "4\n5\n5\n6\n");
}

#[test]
fn yield_inside_a_nested_block_literal_drives_the_enclosing_methods_block() {
    // `yield`/`block_given?` lexically inside a block literal refers to the
    // ENCLOSING METHOD's own block (a block has no implicit block of its
    // own): `analyze::scan_bare_block_use` counts it, so the method gets its
    // `__blk` parameter, and the emitted closure clone-captures it (see
    // `codegen::call::emit_proc_or_lambda_value`). Oracle-verified.
    let result = run_ruby(
        r#"
        class Foo
          def helper(x)
            yield x
          end
          def bar
            helper(1) { yield }
          end
          def baz
            [1, 2].map { |v| yield v }
          end
          def has_block
            [1].each { return block_given? }
          end
        end
        p(Foo.new.bar { "from-outer" })
        p(Foo.new.baz { |v| v * 10 })
        p Foo.new.has_block
        p(Foo.new.has_block {})
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"from-outer\"\n[10, 20]\nfalse\ntrue\n");
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
fn data_define_constructs_deconstructs_and_is_immutable() {
    // Data.define synthesizes an immutable value class with keyword
    // construction, deconstruct_keys (hash patterns), to_h, with, ==, inspect.
    let result = run_ruby(
        r#"
        Coord = Data.define(:x, :y)
        c = Coord.new(x: 1, y: 2)
        p c
        p c.x
        p c.to_h
        p c.with(y: 9)
        p c
        puts(c == Coord.new(x: 1, y: 2))
        def take(d); case d; in {x:, y:}; x + y; end; end
        p take(c)
        begin
          Coord.new(x: 1)
        rescue ArgumentError => e
          puts "err: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "#<data Coord x=1, y=2>\n1\n{x: 1, y: 2}\n#<data Coord x=1, y=9>\n#<data Coord x=1, y=2>\ntrue\n3\nerr: missing keyword: :y\n"
    );
}

#[test]
fn struct_custom_initialize_supers_into_the_member_setter() {
    // A custom `initialize` in the block calls `super` (bare or explicit,
    // positional) into the synthesized member-setter, reached via a two-level
    // base/leaf hierarchy -- not the old "no initialize above" panic.
    let result = run_ruby(
        r#"
        Trip = Struct.new(:x, :y, :z) do
          def initialize(x, y)
            super
          end
        end
        p Trip.new(1, 2).to_a
        V = Struct.new(:a, :b, :c) do
          def initialize(a, b)
            super(a, b, a + b)
          end
        end
        p V.new(3, 4).to_a
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[1, 2, nil]\n[3, 4, 7]\n");
}

#[test]
fn raising_a_non_exception_is_a_type_error() {
    // `raise <non-exception>` coerces at runtime -- a String becomes a
    // RuntimeError, everything else is CRuby's TypeError -- instead of
    // panicking when the raise machinery unwraps a non-Object.
    let result = run_ruby(
        r#"
        def try
          yield
        rescue TypeError => e
          puts "TypeError: #{e.message}"
        end
        try { raise 42 }
        try { raise nil }
        try { raise :sym }
        begin
          raise "boom"
        rescue RuntimeError => e
          puts "RuntimeError: #{e.message}"
        end
        mixed = [Object.new, "msg"]
        begin
          raise mixed[1]
        rescue RuntimeError => e
          puts "poly string: #{e.message}"
        end
        begin
          raise mixed[0]
        rescue TypeError => e
          puts "poly object: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "TypeError: exception class/object expected\n\
         TypeError: exception class/object expected\n\
         TypeError: exception class/object expected\n\
         RuntimeError: boom\n\
         poly string: msg\n\
         poly object: exception class/object expected\n"
    );
}

#[test]
fn clone_runs_user_initialize_copy_hook_with_super() {
    // clone/dup dispatch the user's initialize_copy (deep-copying a
    // shared member), and its bare `super` resolves Object's default no-op
    // hook instead of panicking.
    let result = run_ruby(
        r#"
        class Board
          def initialize
            @table = [[1], [2]]
          end
          def initialize_copy(orig)
            super
            @table = @table.clone
          end
          def push_row
            @table.push([9])
          end
          def size
            @table.length
          end
        end
        b = Board.new
        c = b.clone
        c.push_row
        puts c.size
        puts b.size
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n2\n");
}

#[test]
fn reopening_object_dispatches_to_builtin_and_user_receivers() {
    // reopening Object (with an include + a def) makes those methods
    // dispatch on built-in AND user receivers with the real receiver as self.
    let result = run_ruby(
        r#"
        module ObjectGreeting
          def hi
            "hi from " + self.class.name
          end
        end
        class Object
          include ObjectGreeting
          def global_hi
            "global " + self.class.name
          end
        end
        class LocalThing
        end
        puts "x".hi
        puts "x".global_hi
        puts 1.global_hi
        puts [1, 2].global_hi
        puts LocalThing.new.global_hi
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "hi from String\nglobal String\nglobal Integer\nglobal Array\nglobal LocalThing\n"
    );
}

#[test]
fn data_custom_initialize_supers_with_keyword_args() {
    // an explicit keyword `super(x: .., y: ..)` binds the parent's
    // keyword params by name; omitting a required one raises CRuby's
    // `missing keyword` ArgumentError.
    let result = run_ruby(
        r#"
        Point = Data.define(:x, :y) do
          def initialize(x:, y:)
            super(x: x * 100, y: y + 1)
          end
        end
        def mk(a, b); Point.new(x: a, y: b); end
        p mk(5, 2)
        Pair = Data.define(:m, :n) do
          def initialize(m:, n:)
            super(m: m)
          end
        end
        begin
          Pair.new(m: 1, n: 2)
        rescue ArgumentError => e
          puts "err: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "#<data Point x=500, y=3>\nerr: missing keyword: :n\n");
}

#[test]
fn case_in_range_pattern_covers_float_poly_and_open_bounds() {
    // A range pattern is Range#=== (rb_case_eq/range_covers), so a Float or
    // Poly scrutinee, an exclusive bound, and a beginless/endless range all
    // work -- the old as_int_unchecked path panicked on non-Int scrutinees.
    let result = run_ruby(
        r#"
        def classify(x)
          case x
          in 0.0...0.5 then "low"
          in 0.5..1.0 then "high"
          in ..0.0 then "neg"
          else "other"
          end
        end
        puts classify(0.2)
        puts classify(0.9)
        puts classify(-1.0)
        puts classify(5.0)
        arr = [3, "x"]
        case arr[0]
        in 0..3 then puts "small"
        else puts "big"
        end
        case arr[1]
        in 0..3 then puts "small"
        else puts "not-int"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "low\nhigh\nneg\nother\nsmall\nnot-int\n");
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

/// Reopening a NATIVE exception class (D3): a reopen `def` ADDS a method that
/// reaches every subclass, and OVERRIDES an existing native method everywhere.
/// The native message lives in a hidden slot, so a reopened `message` reading
/// `@message` sees nil (CRuby parity) -- here it reads `to_s`.
#[test]
fn native_exception_reopen_adds_and_overrides() {
    let result = run_ruby(
        r#"
        class StandardError
          def code
            42
          end
        end
        class Exception
          def message
            "patched: " + to_s
          end
        end
        begin
          raise ArgumentError, "bad value"
        rescue => e
          puts e.code
          puts e.message
          puts e.is_a?(StandardError)
        end
        begin
          raise "plain"
        rescue => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "42\npatched: bad value\ntrue\npatched: plain\n"
    );
}

/// A raised native exception exposes NO `@message` ivar (CRuby stores it in a
/// hidden slot): `instance_variables` is empty and `@message` reads nil.
#[test]
fn native_exception_hides_its_message_ivar() {
    let result = run_ruby(
        r#"
        begin
          raise ArgumentError, "boom"
        rescue => e
          p e.instance_variables
          p e.instance_variable_get(:@message)
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[]\nnil\nboom\n");
}

/// A USER exception subclass is backed by the native `RubyException` (D3): a
/// custom ivar, `super` into the native `initialize` (which stores the message
/// in its hidden slot), and reflection all match CRuby -- `instance_variables`
/// is `[:@code]` only (NOT `@message`), and `@message` reads nil while
/// `#message` returns the super-provided text.
#[test]
fn user_exception_subclass_uses_native_representation() {
    let result = run_ruby(
        r##"
        class MyErr < StandardError
          def initialize(code)
            @code = code
            super("boom #{code}")
          end
          def code
            @code
          end
        end

        e = MyErr.new(42)
        puts e.message
        puts e.code
        puts e.is_a?(StandardError)
        p e.instance_variables
        p e.instance_variable_get(:@message)
        p e.instance_variable_get(:@code)
        begin
          raise MyErr, "direct"
        rescue StandardError => ex
          puts "#{ex.class}: #{ex.message}"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "boom 42\n42\ntrue\n[:@code]\nnil\n42\nMyErr: boom direct\n"
    );
}

/// A user exception subclass with NO `initialize` inherits the native default:
/// `Plain.new(msg)` stores the message in the hidden slot, so `#message`
/// returns it and `instance_variables` is empty. Multi-level `super` chains
/// (`B < A < StandardError`, bare `super` forwarding) also resolve natively.
#[test]
fn user_exception_subclass_super_chain_and_inherited_initialize() {
    let result = run_ruby(
        r#"
        class Plain < RuntimeError
        end
        p1 = Plain.new("hi")
        puts p1.message
        p p1.instance_variables

        class A < StandardError
          def initialize(msg = "a-default")
            super
          end
        end
        class B < A
          def initialize
            super()
            @tag = "b"
          end
          def tag; @tag; end
        end
        b = B.new
        puts b.message
        puts b.tag
        puts b.is_a?(A)

        class Done < StopIteration
        end
        puts Done.new("stop").message
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "hi\n[]\na-default\nb\ntrue\nstop\n"
    );
}

/// An exception subclass whose `initialize` takes KEYWORD arguments (D3): the
/// runtime construction path (`construct_by_class_id`) must carry keywords as
/// the trailing-Hash the trampoline expects, or `code:` would default. Two
/// sibling subclasses -- neither's `message` param pinned to a String -- also
/// exercise the poly `super(message)` coercion into the native message slot.
#[test]
fn user_exception_subclass_keyword_initialize() {
    let result = run_ruby(
        r#"
        class AError < StandardError
          attr_reader :code
          def initialize(message, code: 3)
            super(message)
            @code = code
          end
        end
        class BError < StandardError
          attr_reader :level
          def initialize(message, level = 7)
            super(message)
            @level = level
          end
        end
        a = AError.new("boom", code: 9)
        puts a.code
        puts a.message
        b = BError.new("bad", 5)
        puts b.level
        puts b.message
        puts AError.new("d").code
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "9\nboom\n5\nbad\n3\n");
}

/// A `def self.x` on an exception subclass emits into a `pub mod` (no struct to
/// attach an `impl` to), and a bare `new` inside it constructs via the runtime
/// (D3) -- `Self.new` there yields the native `RubyException`, not a `new_handle`.
#[test]
fn user_exception_subclass_class_method_constructs_via_runtime() {
    let result = run_ruby(
        r#"
        class D < StandardError
          def self.build(n)
            new("built-#{n}")
          end
        end
        e = D.build(3)
        puts e.message
        puts e.class
        puts e.is_a?(StandardError)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "built-3\nD\ntrue\n");
}

/// A user subclass of `Array` (D3) is the native `ValueSubclass` wrapping an
/// array payload: inherited methods (`push`/`<<`/`size`/`each`/`map`) run
/// against it, self-returning mutators re-wrap to the subclass while a NEW
/// collection (`map`) is a plain `Array`, and a custom method sees the elements.
#[test]
fn value_subclass_of_array() {
    let result = run_ruby(
        r#"
        class Stack < Array
          def peek; last; end
        end
        s = Stack.new
        s.push(1)
        s << 2
        p s
        puts s.size
        puts s.peek
        puts s.class
        puts s.push(3).class
        puts s.map { |x| x * 2 }.inspect
        puts s.map { |x| x }.class
        puts s.is_a?(Array)
        puts Stack.new([9, 8]).size
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2]\n2\n2\nStack\nStack\n[2, 4, 6]\nArray\ntrue\n2\n"
    );
}

/// A `String` subclass (D3): inherited `String` methods, a `super`-free custom
/// method, `dup` independence, and payload-based equality/`Hash`-key identity
/// (`Tag.new("k")` is the same key as `"k"`, symmetric `==`).
#[test]
fn value_subclass_of_string_equality_and_hashing() {
    let result = run_ruby(
        r#"
        class Tag < String
          def shout; upcase + "!"; end
        end
        t = Tag.new("hi")
        puts t.shout
        puts t.class
        puts(t == "hi")
        puts("hi" == t)
        puts(Tag.new("x").hash == "x".hash)
        h = { "key" => 1 }
        puts h[Tag.new("key")].inspect
        d = Tag.new("a")
        d2 = d.dup
        d2 << "b"
        puts d
        puts d2
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "HI!\nTag\ntrue\ntrue\ntrue\n1\na\nab\n"
    );
}

/// A `Hash` subclass with a `super` in `initialize` (D3): `super(0)` seeds the
/// default value on the payload, and inherited `[]`/`[]=`/`size` work.
#[test]
fn value_subclass_of_hash_with_super_initialize() {
    let result = run_ruby(
        r#"
        class Counter < Hash
          def initialize
            super(0)
          end
          def bump(k); self[k] += 1; end
        end
        c = Counter.new
        c.bump(:a); c.bump(:a); c.bump(:b)
        puts c[:a]
        puts c[:b]
        puts c[:missing]
        p c
        puts c.class
        puts c.size
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n1\n0\n{a: 2, b: 1}\nCounter\n2\n");
}

/// A user subclass of an IMMEDIATE builtin (D3): the DEFINITION is allowed --
/// `superclass`/`ancestors`/`is_a?` resolve -- but there are no instances, so
/// `MyInt.new` raises CRuby's exact `NoMethodError`.
#[test]
fn immediate_builtin_subclass_defines_but_cannot_instantiate() {
    let result = run_ruby(
        r##"
        class MyInt < Integer; end
        puts MyInt.superclass
        puts MyInt.ancestors.include?(Integer)
        puts MyInt.ancestors.include?(Numeric)
        begin
          MyInt.new
        rescue => e
          puts "#{e.class}: #{e.message}"
        end
        class MySym < Symbol; end
        begin; MySym.new; rescue => e; puts e.class; end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Integer\ntrue\ntrue\nNoMethodError: undefined method 'new' for class MyInt\nNoMethodError\n"
    );
}

/// A `Numeric` subclass (D3) is an ordinary ivar-carrying object: it defines its
/// own `<=>`/state, and `Comparable` (inherited through `Numeric`) drives
/// `<`/`>`/`min` off that `<=>`.
#[test]
fn numeric_subclass_is_a_plain_comparable_object() {
    let result = run_ruby(
        r#"
        class Money < Numeric
          def initialize(cents); @cents = cents; end
          def cents; @cents; end
          def <=>(o); cents <=> o.cents; end
        end
        m = Money.new(500)
        n = Money.new(300)
        puts m.cents
        puts(m > n)
        puts(m == Money.new(500))
        puts m.is_a?(Numeric)
        puts m.is_a?(Comparable)
        puts m.class
        puts [m, n].min.cents
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "500\ntrue\ntrue\ntrue\ntrue\nMoney\n300\n");
}

/// Reopening a builtin MODULE (D3): the added method reaches every includer --
/// Enumerable across Array/Hash, Comparable across Integer.
#[test]
fn builtin_module_reopen_reaches_all_includers() {
    let result = run_ruby(
        r#"
        module Enumerable
          def second
            first(2).last
          end
        end
        module Comparable
          def clamp_low(lo)
            self < lo ? lo : self
          end
        end
        puts [10, 20, 30].second
        puts({ a: 1, b: 2 }.map { |k, v| v }.second)
        puts 5.clamp_low(8)
        puts 12.clamp_low(8)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "20\n2\n8\n12\n");
}

/// A builtin reopen may RESTATE the class's real superclass (`class String <
/// Object`); the methods attach exactly as a clauseless reopen (D3).
#[test]
fn builtin_reopen_with_matching_superclass_clause() {
    let result = run_ruby(
        r#"
        class String < Object
          def shout
            upcase + "!"
          end
        end
        puts "hi".shout
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "HI!\n");
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
fn send_with_a_non_literal_target_binds_keyword_args() {
    // The G2 trailing-kwargs-hash convention: `send`'s truly-dynamic
    // fallback (non-literal target name) carries kwargs as one trailing
    // Hash; the callee's trampoline pops and binds it.
    let result = run_ruby(
        r#"
        class Foo
          def bar(x:)
            x
          end
        end
        name = :bar
        puts Foo.new.send(name, x: 41) + 1
        "#,
    );
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn poly_receiver_call_with_unknown_keyword_raises_at_runtime() {
    // The other old silent-drop site: kwargs on a Poly receiver now ride
    // the G2 convention; an undefined method stays a real NoMethodError.
    let result = run_ruby(
        r#"
        begin
          raise "boom"
        rescue => e
          begin
            e.foo(bar: 1)
          rescue NoMethodError
            puts "no method, kwargs carried"
          end
        end
        "#,
    );
    assert_eq!(result.stdout, "no method, kwargs carried\n");
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
fn self_inside_a_class_method_is_the_class_object() {
    // Was a documented scope-cut ("no first-class Class/Module runtime value
    // exists") and a codegen panic. `RubyValue::Class` has been the
    // representation for a while; a class method's `self` is now that value,
    // so `self` and the class constant are interchangeable -- including as a
    // receiver for the class's OWN other class methods.
    let result = run_ruby(
        r#"
        class Foo
          def self.bar
            self
          end
          def self.baz
            self.bar.name
          end
        end
        p Foo.bar
        p Foo.bar == Foo
        p Foo.baz
        p Foo.bar.new.class
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Foo\ntrue\n\"Foo\"\nFoo\n");
}

#[test]
fn class_level_ivars_are_per_class_and_not_inherited() {
    // The defining property of class-level `@x`, and the whole reason it
    // can't share `@@x`'s storage: a subclass gets its OWN slot, starting
    // empty, even though it inherits the method that reads it. Contrast the
    // `@@cv` line, which IS shared. Oracle-verified (ruby 4.0.5).
    let result = run_ruby(
        r#"
        class Base
          @reg = "base-ivar"
          @@cv = "base-cvar"
          def self.reg; @reg; end
          def self.reg=(v); @reg = v; end
          def self.cv; @@cv; end
          def self.unset; @never_written; end
        end
        class Sub < Base; end
        p Base.reg
        p Sub.reg
        p Sub.cv
        p Base.unset
        Sub.reg = "sub-only"
        p [Base.reg, Sub.reg]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"base-ivar\"\nnil\n\"base-cvar\"\nnil\n[\"base-ivar\", \"sub-only\"]\n"
    );
}

#[test]
fn a_class_ivar_and_an_instance_ivar_of_the_same_name_are_distinct_storage() {
    // `@x` in a class body/class method and `@x` in an instance method name
    // two completely different slots -- the class object's own, and the
    // instance's. Also exercises the same-name collision across Ruby's two
    // method namespaces (`def self.x` + `def x`), which share one generated
    // container and so need `ident::class_method_ident`'s mangling.
    let result = run_ruby(
        r#"
        class C
          @x = "class-level"
          def initialize; @x = "instance-level"; end
          def self.x; @x; end
          def x; @x; end
        end
        p [C.x, C.new.x]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[\"class-level\", \"instance-level\"]\n");
}

#[test]
fn a_class_body_ivar_write_initializes_class_level_state() {
    // A bare `@x = ...` directly in a class body -- the ordinary way
    // class-level state gets seeded. It used to fall through `register_class`'s
    // catch-all arm and be SILENTLY DROPPED, leaving the reader a bare nil
    // with no diagnostic at all.
    let result = run_ruby(
        r#"
        class Registry
          @items = []
          @count = 0
          def self.add(x); @items << x; @count += 1; self; end
          def self.items; @items; end
          def self.count; @count; end
        end
        Registry.add("a").add("b")
        p Registry.items
        p Registry.count
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[\"a\", \"b\"]\n2\n");
}

#[test]
fn module_level_state_accumulates_across_class_method_calls() {
    let result = run_ruby(
        r#"
        module Counter
          @n = 0
          def self.bump; @n += 1; end
          def self.n; @n; end
        end
        Counter.bump
        Counter.bump
        Counter.bump
        p Counter.n
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n");
}

#[test]
fn class_shovel_self_attr_accessor_backs_onto_class_level_ivars() {
    // `class << self; attr_accessor :x; end` is THE idiomatic way to declare
    // class-level state, and it works by generating `def self.x; @x; end` --
    // so it only works once class-level `@x` has real storage.
    let result = run_ruby(
        r#"
        module Reg
          class << self
            attr_accessor :handler
            def helper; "helped"; end
          end
        end
        Reg.handler = "H"
        p Reg.handler
        p Reg.helper

        class Cfg
          class << self
            attr_reader :mode
            attr_writer :mode
          end
          @mode = "default"
        end
        p Cfg.mode
        Cfg.mode = "custom"
        p Cfg.mode
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"H\"\n\"helped\"\n\"default\"\n\"custom\"\n");
}

#[test]
fn a_class_ivar_is_reachable_from_a_block_inside_a_class_method() {
    // The block captures `self` as a plain `RubyValue::Class`, so the ivar
    // resolves through `ivar_get_dyn`/`ivar_set_dyn`'s Class arm rather than
    // the static class-id path -- the two must agree on the same storage.
    let result = run_ruby(
        r#"
        class Blk
          @vals = []
          def self.collect
            [1, 2, 3].each { |i| @vals << i * 10 }
            @vals
          end
        end
        p Blk.collect
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[10, 20, 30]\n");
}

#[test]
fn a_class_method_dispatches_dynamically_through_a_class_valued_variable() {
    // The receiver isn't a literal constant, so codegen can't emit a direct
    // `H1::__cm_run(...)` -- it goes through the registry's class-method
    // table on a `RubyValue::Class` receiver.
    let result = run_ruby(
        r#"
        class H1
          def self.run(x); "h1:#{x}"; end
        end
        class H2
          def self.run(x); "h2:#{x}"; end
        end
        [H1, H2].each { |h| puts h.run(5) }
        handler = H1
        puts handler.run(9)
        handler = H2
        puts handler.run(9)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "h1:5\nh2:5\nh1:9\nh2:9\n");
}

#[test]
fn an_implicit_self_call_in_a_class_method_resolves_against_the_receiver() {
    // `self` in a class method is the class it was CALLED on, not the one
    // whose body the method was written in. Both of these were silently wrong
    // (no error, just the wrong object) while implicit-self resolution used
    // the lexical `defining_class`:
    //   - `Sub.create` built a Base;
    //   - `Ext.helped` answered "Helper".
    let result = run_ruby(
        r#"
        class Base
          def self.create; new; end
          def self.who; name; end
        end
        class Sub < Base; end
        p Base.create.class
        p Sub.create.class
        p Sub.who

        module Helper
          def helped; "helped-#{name}"; end
        end
        class Ext
          extend Helper
        end
        p Ext.helped
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Base\nSub\n\"Sub\"\n\"helped-Ext\"\n");
}

#[test]
fn class_methods_take_the_full_param_shapes() {
    let result = run_ruby(
        r#"
        class P
          def self.m(a, b = 2, *rest, k: 9, **kw, &blk)
            [a, b, rest, k, kw, blk ? blk.call : nil]
          end
        end
        p P.m(1)
        p P.m(1, 3, 4, 5, k: 0, z: 1) { "blk" }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2, [], 9, {}, nil]\n[1, 3, [4, 5], 0, {z: 1}, \"blk\"]\n"
    );
}

#[test]
fn class_objects_answer_the_instance_variable_reflection_family() {
    let result = run_ruby(
        r#"
        class K
          @a = 1
        end
        p K.instance_variable_get(:@a)
        p K.instance_variable_get("@a")
        p K.instance_variable_get(:@nope)
        p K.instance_variables
        p K.instance_variable_set(:@b, 2)
        p K.instance_variables
        p K.instance_variable_defined?(:@a)
        p K.instance_variable_defined?(:@zz)
        begin
          K.instance_variable_get(:a)
        rescue NameError => e
          puts "NameError: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "1\n1\nnil\n[:@a]\n2\n[:@a, :@b]\ntrue\nfalse\n\
         NameError: 'a' is not allowed as an instance variable name\n"
    );
}

#[test]
fn private_method_called_with_an_explicit_receiver_raises_at_runtime() {
    // A visibility violation is RUNTIME behavior in Ruby, not a syntax
    // error: it raises NoMethodError when the call actually runs, so it can
    // be rescued and an unreachable one stays silent. Message shape and
    // both behaviors oracle-verified.
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
        end

        b = Box.new
        begin
          b.helper
        rescue NoMethodError => e
          puts e.message
        end

        def never_runs(b)
          b.helper
        end
        puts "still here"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "private method 'helper' called for an instance of Box
still here
"
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
fn protected_method_called_from_outside_any_related_class_raises_at_runtime() {
    // Same rule as the private case above -- a runtime NoMethodError.
    let result = run_ruby(
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
        begin
          a.amount
        rescue NoMethodError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "protected method 'amount' called for an instance of Money
"
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
fn kwargs_double_splat_at_a_call_site_binds() {
    // `**h` merges into the G2 trailing-kwargs Hash (literal pairs first,
    // splat entries after, same-key replacement -- `Hash#merge`'s rule).
    let result = run_ruby(
        r##"
        class Greeter
          def f(x:, y: 0)
            "#{x}/#{y}"
          end
        end
        h = {x: 1}
        puts Greeter.new.f(**h)
        puts Greeter.new.f(y: 5, **{x: 2, y: 9})
        "##,
    );
    assert_eq!(result.stdout, "1/0\n2/9\n");
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
fn regex_backreferences_and_lookaround_via_fancy_engine() {
    // Constructs the linear-time `regex` crate can't do; a backtracking engine
    // transparently backs them. The fast path stays unaffected.
    let result = run_ruby(
        r#"
        puts("hello" =~ /(\w)\1/)             # 2
        p("abc" =~ /(\w)\1/)                  # nil
        puts "foobar".match?(/foo(?=bar)/)    # true
        puts "foobaz".match?(/foo(?=bar)/)    # false
        puts "$100".gsub(/(?<=\$)\d+/, "N")   # $N
        puts "catfish".match?(/cat(?!fish)/)  # false
        puts "abc".match?(/a(?#c)bc/)         # true
        puts "book".match?(/(?<c>o)\k<c>/)    # true
        # fast path unaffected
        m = "2024-01-15".match(/(\d+)-(\d+)-(\d+)/)
        puts m[2]                             # 01
        p "a,b,c".split(/,/)                  # ["a", "b", "c"]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "2\nnil\ntrue\nfalse\n$N\nfalse\ntrue\ntrue\n01\n[\"a\", \"b\", \"c\"]\n"
    );
}

#[test]
fn method_arity_parameters_and_unbound_bind() {
    let result = run_ruby(
        r#"
        class C
          def a(x, y); x + y; end
          def b(x, y=1); end
          def d(x, y:, z: 2); end
          def f(x, *r, z, k:, **o, &blk); end
          def g; end
        end
        o = C.new
        puts o.method(:a).arity                 # 2
        puts o.method(:a).parameters.inspect     # [[:req, :x], [:req, :y]]
        puts o.method(:b).arity                 # -2
        puts o.method(:d).arity                 # 2
        puts o.method(:d).parameters.inspect     # [[:req,:x],[:keyreq,:y],[:key,:z]]
        puts o.method(:f).arity                 # -4
        puts o.method(:g).arity                 # 0
        um = C.instance_method(:a)
        puts um.class                           # UnboundMethod
        puts um.name                            # a
        puts um.arity                           # 2
        puts um.bind(o).call(2, 3)              # 5
        puts um.bind_call(o, 4, 5)              # 9
        puts o.method(:a).unbind.class          # UnboundMethod
        begin
          um.bind(42)
        rescue TypeError
          puts "typeerror"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "2\n[[:req, :x], [:req, :y]]\n-2\n2\n[[:req, :x], [:keyreq, :y], [:key, :z]]\n-4\n0\nUnboundMethod\na\n2\n5\n9\nUnboundMethod\ntypeerror\n"
    );
}

#[test]
fn methods_and_instance_methods_reflect_names_with_visibility_and_inheritance() {
    let result = run_ruby(
        r#"
        module M
          def helper; end
        end
        class Base
          def pub; end
          private
          def priv; end
        end
        class Sub < Base
          include M
          def own; end
          def pub; end          # override
          def self.factory; end
        end
        puts Sub.instance_methods(false).sort.inspect   # [:own, :pub]
        puts Sub.instance_methods.include?(:helper)      # true (module)
        puts Sub.instance_methods.include?(:pub)         # true
        puts Sub.instance_methods.include?(:priv)        # false (private)
        puts Base.private_instance_methods(false).inspect # [:priv]
        puts M.instance_methods.inspect                  # [:helper]
        puts Sub.singleton_methods.inspect               # [:factory]
        o = Sub.new
        puts o.methods.include?(:own)                    # true
        puts o.methods.include?(:factory)                # false
        puts o.private_methods.include?(:priv)           # true
        puts "s".methods.include?(:upcase)               # true
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[:own, :pub]\ntrue\ntrue\nfalse\n[:priv]\n[:helper]\n[:factory]\ntrue\nfalse\ntrue\ntrue\n"
    );
}

#[test]
fn hash_compare_by_identity_keys_by_object_not_value() {
    let result = run_ruby(
        r#"
        h = {}
        puts h.compare_by_identity?          # false
        puts h.compare_by_identity.equal?(h) # true (returns self)
        puts h.compare_by_identity?          # true
        a = "x" + ""
        b = "x" + ""
        h[a] = 1
        h[b] = 2
        puts h.size                          # 2 (distinct identities)
        puts h[a]                            # 1
        p h["x" + ""]                        # nil (fresh object)
        # immediates still key by value
        hi = {}.compare_by_identity
        hi[1] = "one"; hi[1] = "ONE"
        hi[:s] = 9; hi[:s] = 10
        puts hi.size                         # 2 (1 and :s)
        puts hi[1]                           # ONE
        # frozen raises
        begin
          {}.freeze.compare_by_identity
        rescue => e
          puts e.class                       # FrozenError
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "false\ntrue\ntrue\n2\n1\nnil\n2\nONE\nFrozenError\n"
    );
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
fn subclassing_an_unsupported_built_in_type_is_a_clean_error() {
    // D3 opened subclassing for Array/String/Hash (payload), Numeric (plain
    // object), and the immediates (registry-only). `Range` stays rejected --
    // it has no runtime constructor -- so this remains a clean compile error.
    let err = spinelc::compile_to_rust(
        r#"
        class MyRange < Range
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
fn named_capture_auto_binding_assigns_a_local_per_group() {
    // `/(?<a>..)/ =~ str` assigns each named group to a local of that name.
    // Only with the literal on the LEFT -- `str =~ /(?<a>.)/` binds nothing,
    // which is Ruby's own asymmetry (the parser can only declare the locals
    // when it can see the names), not an approximation. Oracle-verified,
    // including that a failed match leaves each name nil.
    let result = run_ruby(
        r#"
        if /(?<first>\w+) (?<last>\w+)/ =~ "John Smith"
          puts first
          puts last
        end
        p(/(?<n>\d+)/ =~ "abc123")
        p n
        /(?<z>\d+)/ =~ "none"
        p z
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "John\nSmith\n3\n\"123\"\nnil\n");
}

#[test]
fn file_line_and_dir_name_the_file_the_code_was_written_in() {
    // The point of the SOURCE_FILE stack: `require` merges every file's
    // statements into one Program, so by codegen time nothing tells them
    // apart -- these have to be resolved at LOWERING time, per file. A
    // `__FILE__` inside a required file must name THAT file, not the main
    // one, and the main file's own `__FILE__` after the require must be
    // itself again.
    //
    // Only basenames are compared: the harness compiles from a per-test
    // temp dir, so the absolute paths differ per run.
    let result = run_ruby_project(
        &[
            (
                "helper.rb",
                "def helper_file; __FILE__; end\ndef helper_line; __LINE__; end\ndef helper_dir; __dir__; end\n",
            ),
            (
                "main.rb",
                r#"
                require_relative "helper"
                puts File.basename(helper_file)
                puts helper_line
                puts File.basename(__FILE__)
                puts __LINE__
                puts helper_dir == __dir__
                puts __dir__.start_with?("/")
                "#,
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    // helper_line is 2 (the `def helper_line` line); the main file's
    // `__LINE__` is on its own 6th line counting the leading newline.
    //
    // `__dir__` is only checked for absoluteness and for agreeing between
    // the two files, NOT compared against `File.expand_path(__FILE__)`:
    // `__dir__` is baked at COMPILE time while `expand_path` of a relative
    // `__FILE__` resolves against the RUNTIME cwd, and this harness runs
    // the binary from a different directory than it compiled in. The two
    // agree whenever the program is run from its own directory, which is
    // verified against the oracle separately.
    assert_eq!(result.stdout, "helper.rb\n2\nmain.rb\n6\ntrue\ntrue\n");
}

#[test]
fn encoding_surface_reports_transcodes_and_raises() {
    // The Encoding engine: a string is bytes + an encoding, queryable and
    // transcodable, with the Encoding class and its constants. `__ENCODING__`
    // answers the script encoding (UTF-8). Cross-checked against ruby 4.0.5.
    let result = run_ruby(
        r##"
        p "hello".encoding
        p __ENCODING__
        p "€".bytesize
        p "€".length
        p "hello".ascii_only?
        p "€".ascii_only?
        p Encoding::UTF_8.name
        p Encoding::ASCII_8BIT.names
        p Encoding.find("BINARY")
        p Encoding.default_external
        raw = "€".b
        p raw.encoding
        p raw.length
        mis = "€".dup.force_encoding("US-ASCII")
        p mis.valid_encoding?
        latin = "café".encode(Encoding::ISO_8859_1)
        p latin.encoding
        p latin.bytes
        p latin.encode(Encoding::UTF_8) == "café"
        p "café".encode(Encoding::US_ASCII, undef: :replace)
        p "a<b>&c".encode(Encoding::US_ASCII, xml: :text)
        begin
          "café".encode(Encoding::US_ASCII)
        rescue Encoding::UndefinedConversionError => e
          puts "raised #{e.class}"
        end
        p :hi.encoding
        p :café.encoding
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "#<Encoding:UTF-8>\n#<Encoding:UTF-8>\n3\n1\ntrue\nfalse\n\
         \"UTF-8\"\n[\"ASCII-8BIT\", \"BINARY\"]\n\
         #<Encoding:BINARY (ASCII-8BIT)>\n#<Encoding:UTF-8>\n\
         #<Encoding:BINARY (ASCII-8BIT)>\n3\nfalse\n\
         #<Encoding:ISO-8859-1>\n[99, 97, 102, 233]\ntrue\n\
         \"caf?\"\n\"a&lt;b&gt;&amp;c\"\n\
         raised Encoding::UndefinedConversionError\n\
         #<Encoding:US-ASCII>\n#<Encoding:UTF-8>\n"
    );
}

#[test]
fn broken_and_binary_strings_inspect_with_hex_escapes() {
    // A byte that isn't a character in the string's encoding renders as
    // \xNN, and the cross-encoding equality rule keeps ASCII-only strings
    // equal across encodings. Cross-checked against ruby 4.0.5.
    let result = run_ruby(
        r#"
        p "abc".b == "abc"
        p "abc".b.encoding
        s = "\xff\x80".b
        p s.valid_encoding?
        puts s.inspect
        puts "caf\xe9".force_encoding("ISO-8859-1").inspect
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\n#<Encoding:BINARY (ASCII-8BIT)>\ntrue\n\
         \"\\xFF\\x80\"\n\"caf\\xE9\"\n"
    );
}

#[test]
fn magic_comment_sets_the_script_encoding() {
    // A `# encoding:` comment on the first line tags every string literal and
    // __ENCODING__ with that encoding. Verified against ruby 4.0.5. (The
    // source must start with the comment, so no leading newline here.)
    let result = run_ruby(
        "# encoding: ISO-8859-1\n\
         p __ENCODING__\n\
         p \"hi\".encoding\n\
         p \"hi\".encoding == Encoding::ISO_8859_1\n\
         p \"hi\".bytes\n",
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "#<Encoding:ISO-8859-1>\n#<Encoding:ISO-8859-1>\ntrue\n[104, 105]\n"
    );
}

#[test]
fn file_read_applies_external_and_internal_encodings() {
    // File.read tags bytes with the external encoding (default UTF-8), or a
    // requested one; binread is always ASCII-8BIT; binwrite round-trips raw
    // bytes. Verified against ruby 4.0.5.
    let result = run_ruby(
        r#"
        Dir.mktmpdir do |dir|
          path = File.join(dir, "f.txt")
          File.write(path, "café")
          p File.read(path).encoding
          p File.read(path).bytesize
          p File.binread(path).encoding
          p File.binread(path).bytes
          p File.read(path, encoding: "ISO-8859-1").encoding
          p File.read(path, encoding: "ISO-8859-1").bytes
          File.binwrite(path, [0, 255, 128].pack("C*"))
          p File.binread(path).bytes
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "#<Encoding:UTF-8>\n5\n#<Encoding:BINARY (ASCII-8BIT)>\n\
         [99, 97, 102, 195, 169]\n#<Encoding:ISO-8859-1>\n\
         [99, 97, 102, 195, 169]\n[0, 255, 128]\n"
    );
}

#[test]
fn pack_and_unpack_roundtrip_core_directives() {
    // Array#pack / String#unpack across the integer, string, base64, hex,
    // BER and UTF-8 directives. Verified against ruby 4.0.5.
    let result = run_ruby(
        r#"
        p [65, 66, 67].pack("C*")
        p [258].pack("v").bytes
        p [258].pack("S>").bytes
        p [-1].pack("l").bytes
        p "\x00\x00\x00\x01".unpack("N")
        p "\xff\xff\xff\xff".unpack("l")
        p "\xff\xff\xff\xff".unpack("L")
        p ["hi"].pack("a5").bytes
        p ["hi"].pack("A5").bytes
        p "abc\0de".unpack("Z*")
        p ["hello world"].pack("m")
        p "aGVsbG8=\n".unpack("m")
        p ["ff01"].pack("H*").bytes
        p [300].pack("w").bytes
        p [12354].pack("U")
        p "あ".unpack("U*")
        p [65].pack("C").encoding
        p [12354].pack("U").encoding
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"ABC\"\n[2, 1]\n[1, 2]\n[255, 255, 255, 255]\n[1]\n[-1]\n[4294967295]\n\
         [104, 105, 0, 0, 0]\n[104, 105, 32, 32, 32]\n[\"abc\"]\n\
         \"aGVsbG8gd29ybGQ=\\n\"\n[\"hello\"]\n[255, 1]\n[130, 44]\n\
         \"\u{3042}\"\n[12354]\n\
         #<Encoding:BINARY (ASCII-8BIT)>\n#<Encoding:UTF-8>\n"
    );
}

#[test]
fn undef_removes_a_name_including_an_inherited_one() {
    // `undef` works on a name this class only INHERITS, which is why it
    // can't be "delete the local def" -- there is none. It stays live on
    // the ancestor that defined it, and `respond_to?` must agree.
    let result = run_ruby(
        r#"
        class B
          def inherited_m; "from B"; end
          def own; "own"; end
        end
        class C < B
          def local_m; "local"; end
          undef local_m
          undef inherited_m
        end
        begin; C.new.local_m; rescue NoMethodError; puts "local: NoMethodError"; end
        begin; C.new.inherited_m; rescue NoMethodError; puts "inherited: NoMethodError"; end
        p B.new.inherited_m
        p C.new.own
        p C.new.respond_to?(:inherited_m)
        class D
          def a; 1; end
          def b; 2; end
          undef a, b
        end
        begin; D.new.a; rescue NoMethodError; puts "D#a gone"; end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "local: NoMethodError\ninherited: NoMethodError\n\"from B\"\n\"own\"\nfalse\nD#a gone\n"
    );
}

#[test]
fn a_global_alias_shares_storage_in_both_directions() {
    // `alias $copy $orig` is a real alias, not a copy: one slot, two names,
    // and writing EITHER is visible through the other. Oracle-verified both
    // ways -- which is why it can't lower to `$copy = $orig`.
    let result = run_ruby(
        r#"
        $orig = 5
        alias $copy $orig
        $copy = 7
        p $orig
        p $copy
        $orig = 9
        p [$orig, $copy]
        alias $b $never_set
        p $b
        $never_set = 1
        p $b
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\n7\n[9, 9]\nnil\n1\n");
}

#[test]
fn begin_and_end_blocks_run_before_and_after_the_main_program() {
    // BEGIN runs first in SOURCE order; END runs at exit in REVERSE order,
    // which is `at_exit` exactly -- so END lowers to one.
    let result = run_ruby(
        r#"
        puts "main 1"
        END { puts "end A" }
        BEGIN { puts "begin A" }
        puts "main 2"
        END { puts "end B" }
        BEGIN { puts "begin B" }
        puts "main 3"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "begin A\nbegin B\nmain 1\nmain 2\nmain 3\nend B\nend A\n"
    );
}

#[test]
fn hash_shorthand_and_interpolated_symbols() {
    let result = run_ruby(
        r##"
        x = 1
        name = "n"
        p({x:, name:})
        def kw(a:, b:); [a, b]; end
        a = 10
        b = 20
        p kw(a:, b:)
        w = "world"
        p :"hello_#{w}"
        p :"a#{1 + 1}b"
        p :"plain"
        p :"a#{1}b".class
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "{x: 1, name: \"n\"}\n[10, 20]\n:hello_world\n:a2b\n:plain\nSymbol\n"
    );
}

#[test]
fn splat_in_a_when_clause_tests_every_candidate() {
    let result = run_ruby(
        r#"
        a = [1, 2]
        case 1
        when *a then puts "hit"
        else puts "miss"
        end
        case 9
        when *a then puts "hit"
        else puts "miss"
        end
        case 5
        when 3, *a, 5 then puts "mixed hit"
        else puts "mixed miss"
        end
        kinds = [Integer, String]
        case "s"
        when *kinds then puts "kind hit"
        end
        none = []
        case 1
        when *none then puts "never"
        else puts "empty ok"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "hit\nmiss\nmixed hit\nkind hit\nempty ok\n"
    );
}

#[test]
fn splatting_a_non_array_follows_rubys_to_a_rules() {
    // Splatting a non-Array is ordinary Ruby, not an error -- it used to
    // PANIC ("expected an Array to splat"), taking down `a, b = *1`.
    // `[*"str"]` is the case worth pinning: it looks like it should split
    // into characters and doesn't, because String has no `to_a`. Probing
    // respond_to? rather than special-casing types gets that right, and
    // gets a user class with its own to_a right too.
    let result = run_ruby(
        r#"
        p [*1]
        p [*nil]
        p [*[1, 2]]
        p [*{a: 1}]
        p [*"str"]
        p [*(1..3)]
        class HasToA; def to_a; [7, 8]; end; end
        p [*HasToA.new]
        class NoToA; end
        p([*NoToA.new].size)
        p [*:sym]
        a, b = *1
        p [a, b]
        c, d = *nil
        p [c, d]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1]\n[]\n[1, 2]\n[[:a, 1]]\n[\"str\"]\n[1, 2, 3]\n[7, 8]\n1\n[:sym]\n[1, nil]\n[nil, nil]\n"
    );
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
fn aliasing_a_genuinely_undefined_method_is_a_clean_error() {
    // An `alias`/`alias_method` whose source is defined neither in this body
    // nor anywhere in the ancestry is a clean compile error -- now raised by
    // `mro::resolve_aliases` (which resolves inherited sources), not a
    // lowering-time rejection.
    let err = spinelc::compile_to_rust(
        r#"
        class Foo
          alias bar undefined_method
        end
        "#,
    )
    .unwrap_err();
    assert!(err.contains("undefined method 'undefined_method'"), "{err}");
}

#[test]
fn class_shift_a_constant_object_defines_its_singleton() {
    // #97 F3: `class << CONST` on a constant-bound object installs per-object
    // singleton methods on it (previously a clean rejection).
    let result = run_ruby(
        r#"
        class Foo
          ANOTHER = Object.new
          class << ANOTHER
            def hi
              "hi"
            end
          end
        end
        puts Foo::ANOTHER.hi
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi\n");
}

#[test]
fn class_shift_self_containing_an_unsupported_statement_is_a_clean_lowering_error() {
    // `def`s, constants, `include`, and `attr_*`/`private`/`alias` are handled
    // in `class << self`; an ivar assignment on the singleton (`@x = 1`) is not
    // -- a clean rejection, not silently ignored.
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
    assert!(err.contains("unsupported statement in `class << self`"), "{err}");
}

#[test]
fn class_shift_self_constants_are_visible_to_the_singletons_class_methods() {
    // #96 harness-surfaced: the dominant `class << self` stdlib idiom (e.g.
    // URI's `class << self; RESERVED = ...; def escape; ...RESERVED...; end`)
    // defines constants alongside the class methods that reference them. The
    // constant is spliced onto the enclosing class, whose class methods resolve
    // it lexically (oracle-verified).
    let result = run_ruby(
        r##"
        class Config
          class << self
            PREFIX = "cfg:"
            LIMIT = 3
            def key(n); "#{PREFIX}#{n}"; end
            def capped(n); n > LIMIT ? LIMIT : n; end
          end
        end
        puts Config.key("host")
        puts Config.capped(9)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "cfg:host\n3\n");
}

#[test]
fn class_shift_self_include_extends_the_enclosing_class() {
    // `include M` inside `class << self` == `extend M` on the enclosing class:
    // the module's instance methods become the class's class methods.
    let result = run_ruby(
        r##"
        module Greeter
          def hi(n); "hi #{n}"; end
        end
        class Widget
          class << self
            include Greeter
          end
        end
        puts Widget.hi("bob")
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi bob\n");
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
    // The message carries the receiver's real `#<Pt:0xADDR @x=1>` inspect
    // (address normalized in-program). Semantics oracle-verified: a catchable
    // FrozenError, the ivar keeps its old value, readers still work.
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
          puts e.send(:message).gsub(/0x[0-9a-f]+/, "0xADDR")
        end
        puts p1.x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\ncan't modify frozen Pt: #<Pt:0xADDR @x=1>\n1\n"
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
fn condition_variable_wait_signal_and_broadcast_coordinate_threads() {
    // signal/broadcast return self; wait releases the mutex, parks
    // (coroutine-yielding) until broadcast, then re-acquires. The handoff is
    // deterministic via the shared `ready` flag under the mutex.
    let result = run_ruby(
        r#"
        cv = ConditionVariable.new
        puts cv.signal.equal?(cv)
        puts cv.broadcast.equal?(cv)
        # a lone wait with a timeout returns (does not hang)
        m0 = Mutex.new
        m0.synchronize { cv.wait(m0, 0.01) }
        puts "timeout-ok"

        mutex = Mutex.new
        ready = false
        worker = Thread.new do
          mutex.synchronize do
            cv.wait(mutex) until ready
          end
          puts "woke"
        end
        mutex.synchronize do
          ready = true
          cv.broadcast
        end
        worker.join
        puts "joined"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\ntimeout-ok\nwoke\njoined\n");
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
fn a_stdlib_style_feature_drops_in_through_an_i_search_root() {
    // #96: stdlib is delivered as ordinary `-I <lib>` load-path roots (no
    // bespoke flag) -- pointing `-I` at a Ruby checkout's `lib` makes each
    // `require "feature"` resolve a real stdlib `.rb`. This models that with a
    // pure-Ruby "stdlib" file living under an `-I` root, required by name and
    // compiled + run through the ordinary loader path (the same mechanism the
    // `stdlib-status` harness drives against the installed 4.0.5 lib).
    let result = support::run_ruby_project(
        &[
            (
                "rubylib/shellish.rb",
                r#"
                    module Shellish
                      def self.escape(s)
                        s.gsub(" ", "\\ ")
                      end
                    end
                "#,
            ),
            (
                "main.rb",
                r#"
                    require "shellish"
                    puts Shellish.escape("a b c")
                "#,
            ),
        ],
        "main.rb",
        &["rubylib"],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "a\\ b\\ c\n");
}

#[test]
fn an_earlier_i_root_shadows_a_later_same_named_stdlib_file() {
    // The documented precedence: `-I` roots are searched in order, first hit
    // wins (Ruby's own `$LOAD_PATH` rule) -- so a user root placed BEFORE the
    // stdlib checkout shadows the stdlib copy of a same-named feature, and the
    // later root's file is never loaded.
    let result = support::run_ruby_project(
        &[
            (
                "userlib/patched.rb",
                r#"
                    module Patched
                      def self.source = "user override"
                    end
                "#,
            ),
            (
                "stdliblib/patched.rb",
                r#"
                    module Patched
                      def self.source = "stdlib original"
                    end
                "#,
            ),
            (
                "main.rb",
                r#"
                    require "patched"
                    puts Patched.source
                "#,
            ),
        ],
        "main.rb",
        &["userlib", "stdliblib"],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "user override\n");
}

#[test]
fn autoload_nested_in_a_module_eagerly_splices_the_feature_file() {
    // #101: `autoload :Const, "feature"` is treated as a compile-time
    // require -- the loader's eager pre-pass splices the feature file (at any
    // structural nesting) so the constant is defined; the `autoload` call
    // itself is a no-op. Documented divergence from CRuby's laziness (loads at
    // the autoload site, not first access), but observationally identical for
    // a definitional autoloaded file.
    let result = support::run_ruby_project(
        &[
            (
                "lib/greeter.rb",
                r##"
                    module App
                      module Greeter
                        def self.hi(n); "hi #{n}"; end
                      end
                    end
                "##,
            ),
            (
                "main.rb",
                r#"
                    module App
                      autoload :Greeter, "greeter"
                    end
                    puts App::Greeter.hi("bob")
                "#,
            ),
        ],
        "main.rb",
        &["lib"],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi bob\n");
}

#[test]
fn autoload_with_file_expand_path_dir_resolves_a_sibling_file() {
    // The dominant stdlib/bundler idiom: `autoload :C, File.expand_path("c",
    // __dir__)` -- a computed sibling path. Resolved at compile time from the
    // requiring file's directory.
    let result = support::run_ruby_project(
        &[
            (
                "mirror.rb",
                r#"
                    class Config
                      class Mirror
                        def self.name; "mirror!"; end
                      end
                    end
                "#,
            ),
            (
                "main.rb",
                r#"
                    class Config
                      autoload :Mirror, File.expand_path("mirror", __dir__)
                    end
                    puts Config::Mirror.name
                "#,
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "mirror!\n");
}

#[test]
fn autoload_with_a_dynamic_feature_is_a_clean_compile_error() {
    // The splice target must be compile-time-known; a computed path that
    // isn't the `File.expand_path(..., __dir__)` idiom is a clean rejection
    // (like a non-top-level `require`), not a silently-undefined constant.
    let err = spinelc::compile_to_rust("autoload :X, some_method_call").unwrap_err();
    assert!(err.contains("must resolve at compile time"), "unexpected error: {err}");
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
fn load_with_a_wrap_argument_is_a_clean_rejection() {
    // (`autoload` is now supported via the loader's eager splice -- see the
    // `autoload_*` tests above.) `load "file", wrap` still needs load-time
    // anonymous-module scoping this compiler lacks.
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

// (Removed: `native_crate_is_linked_only_when_its_package_is_required` and
// `native_func_arity_is_checked_at_compile_time` -- both asserted the retired
// native-DSL linking behavior for `base64`, which is now an in-tree `ext/`
// module. `base64_package_matches_real_ruby` above now validates the in-tree
// path, and arity is enforced at runtime via `arity!` -> ArgumentError.)

#[test]
fn require_gated_extension_constant_is_a_name_error_without_its_require() {
    // The ext require-gate (#92): a require-gated builtin's constant
    // (`Base64`, gated by `"base64"`) is INVISIBLE until `require "base64"`
    // activates it -- referencing it un-required raises `NameError:
    // uninitialized constant Base64`, oracle-verified against ruby 4.0.5
    // (`uninitialized constant Base64 (NameError)`). The gate rides the
    // ordinary `resolve_class` -> unset-constant path, so it's a rescuable
    // RUNTIME NameError, not a compile error.
    let result = run_ruby(r#"puts Base64.strict_encode64("hi")"#);
    assert!(!result.status.success());
    assert!(
        result.stderr.contains("uninitialized constant Base64"),
        "stderr: {}",
        result.stderr
    );
}

#[test]
fn require_gated_extension_name_error_is_rescuable() {
    // Same gate, caught: since the miss surfaces through the runtime
    // constant path it's an ordinary rescuable `NameError` (oracle prints
    // `rescued: uninitialized constant Base64`). This program NEVER requires
    // base64, so activation stays off -- avoiding the documented
    // program-global-activation divergence (a `require` anywhere activates
    // the constant everywhere, unlike CRuby's file-ordered visibility).
    let result = run_ruby(
        r#"
        begin
          Base64.strict_encode64("hi")
        rescue NameError => e
          puts "rescued: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "rescued: uninitialized constant Base64\n");
}

#[test]
fn in_tree_base64_over_two_args_raises_argument_error_at_runtime() {
    // The in-tree Base64 ext checks arity at runtime (a rescuable
    // ArgumentError) via `arity!`.
    let result = run_ruby(
        r#"
        require "base64"
        begin
          Base64.encode64("a", "b")
        rescue ArgumentError
          puts "argerr"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "argerr\n");
}

// ---- Wave-1 in-tree extensions (stringio/strscan/cgi/digest) + json/yaml/zlib ----

#[test]
fn stringio_reads_writes_and_tracks_position() {
    let result = run_ruby(
        r#"
        require "stringio"
        io = StringIO.new
        io.puts "hello"
        io.print "world"
        puts io.string.inspect
        io.rewind
        puts io.gets.inspect
        puts io.read.inspect
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"hello\\nworld\"\n\"hello\\n\"\n\"world\"\n");
}

#[test]
fn strscan_scans_anchored_and_until() {
    let result = run_ruby(
        r#"
        require "strscan"
        sc = StringScanner.new("foo123bar")
        puts sc.scan(/[a-z]+/)
        puts sc.scan(/\d+/)
        puts sc.rest
        sc2 = StringScanner.new("a=1;b=2")
        puts sc2.scan_until(/;/)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "foo\n123\nbar\na=1;\n");
}

#[test]
fn cgi_escape_helpers_match_ruby() {
    let result = run_ruby(
        r#"
        require "cgi/escape"
        puts CGI.escape("a b&c=d")
        puts CGI.escapeHTML("<x>&'")
        puts CGI.unescape("a+b%26c")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "a+b%26c%3Dd\n&lt;x&gt;&amp;&#39;\na b&c\n");
}

#[test]
fn digest_class_and_streaming_apis_match_ruby() {
    let result = run_ruby(
        r#"
        require "digest"
        puts Digest::SHA256.hexdigest("abc")
        puts Digest::MD5.hexdigest("")
        d = Digest::SHA256.new
        d << "a"; d.update("bc")
        puts d.hexdigest
        puts Digest::SHA256.base64digest("abc")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n\
         d41d8cd98f00b204e9800998ecf8427e\n\
         ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n\
         ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=\n"
    );
}

#[test]
fn json_parse_and_generate_match_ruby() {
    let result = run_ruby(
        r#"
        require "json"
        puts JSON.generate({"a" => 1, "b" => [2, 3.5, nil, true]})
        puts JSON.parse('{"x":[1,2,3]}').inspect
        puts JSON.pretty_generate({"k" => [1, 2]})
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "{\"a\":1,\"b\":[2,3.5,null,true]}\n\
         {\"x\" => [1, 2, 3]}\n\
         {\n  \"k\": [\n    1,\n    2\n  ]\n}\n"
    );
}

#[test]
fn yaml_load_and_dump_match_ruby_via_the_psych_alias() {
    // `require "yaml"` exposes both `YAML` and `Psych` (Ruby's `yaml.rb` is
    // `YAML = Psych`); dump uses Psych's block style (sequences under a key
    // stay at the key's indent).
    let result = run_ruby(
        r#"
        require "yaml"
        print YAML.dump({"a" => 1, "b" => [2, "x"]})
        puts YAML.load("list:\n- a\n- b").inspect
        puts Psych.load("x: 1").inspect
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "---\na: 1\nb:\n- 2\n- x\n{\"list\" => [\"a\", \"b\"]}\n{\"x\" => 1}\n"
    );
}

#[test]
fn zlib_checksums_match_ruby() {
    let result = run_ruby(
        r#"
        require "zlib"
        puts Zlib.crc32("abc")
        puts Zlib.adler32("abc")
        puts Zlib.crc32("abc", 100)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "891568578\n38600999\n2063213118\n");
}

#[test]
fn scaffolded_extension_constant_resolves_but_is_a_name_error_without_require() {
    // A scaffolded ext (json) is still require-gated: its constant is a
    // NameError until `require "json"` fires, exactly like the fully-built
    // ones -- scaffolding changes only what the METHODS do, not the gate.
    let result = run_ruby(r#"puts JSON.generate([1])"#);
    assert!(!result.status.success());
    assert!(
        result.stderr.contains("uninitialized constant JSON"),
        "stderr: {}",
        result.stderr
    );
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
fn new_with_the_wrong_arity_raises_a_rescuable_runtime_error() {
    // Was a COMPILE-TIME panic, from `.new`'s own hand-rolled argument
    // binding. Now that `.new` binds through the same `emit_call_args_to`
    // every other call site uses, it inherits that machinery's posture,
    // which is real Ruby's: arity resolves at RUNTIME, the error is
    // rescuable, and a never-executed bad call compiles fine. Both lines
    // oracle-verified, message included.
    let result = run_ruby(
        r#"
        class Bag
          def initialize(a, b = 1); end
        end
        begin
          Bag.new(1, 2, 3)
        rescue ArgumentError => e
          puts "ArgumentError: #{e.message}"
        end
        def never_called
          Bag.new(1, 2, 3)
        end
        puts "compiled fine"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "ArgumentError: wrong number of arguments (given 3, expected 1..2)\ncompiled fine\n"
    );
}

#[test]
fn dynamic_dispatch_wrong_arity_raises_a_rescuable_argument_error() {
    // `send` routes through `emit_dynamic_trampoline`, whose arity guard used
    // to `panic!` -- a process abort where Ruby raises a RESCUABLE
    // ArgumentError. Now it emits `return Err(raise_error("ArgumentError",
    // ...))`, so the program survives every mismatch and the final line
    // prints. All three message shapes -- fixed `N`, range `N..M`, and the
    // rest form `N+` -- are oracle-verified against ruby 4.0.5.
    let result = run_ruby(
        r##"
        class A
          def fixed(a) = a
          def rng(a, b = 2) = a + b
          def restp(a, *b) = a
        end
        o = A.new
        [[:fixed, []], [:fixed, [1, 2]], [:rng, []], [:rng, [1, 2, 3]], [:restp, []]].each do |m, args|
          begin
            o.send(m, *args)
          rescue ArgumentError => e
            puts "#{m}: #{e.message}"
          end
        end
        puts "survived"
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "fixed: wrong number of arguments (given 0, expected 1)\n\
         fixed: wrong number of arguments (given 2, expected 1)\n\
         rng: wrong number of arguments (given 0, expected 1..2)\n\
         rng: wrong number of arguments (given 3, expected 1..2)\n\
         restp: wrong number of arguments (given 0, expected 1+)\n\
         survived\n"
    );
}

#[test]
fn value_trampoline_wrong_arity_raises_a_rescuable_argument_error() {
    // The `ValueMethodFn` trampoline (`emit_value_trampoline`) backs both a
    // user CLASS method reached dynamically and a reopened-builtin instance
    // method. Its arity guard shared the same `panic!` bug as the dynamic
    // trampoline; both now raise a rescuable ArgumentError. Oracle-verified.
    let result = run_ruby(
        r##"
        class A
          def self.cm(a, b) = a + b
        end
        class Integer
          def double(a); self * 2; end
        end
        k = A
        begin
          k.send(:cm, 1)
        rescue ArgumentError => e
          puts "cm: #{e.message}"
        end
        begin
          5.send(:double)
        rescue ArgumentError => e
          puts "double: #{e.message}"
        end
        puts "survived"
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "cm: wrong number of arguments (given 1, expected 2)\n\
         double: wrong number of arguments (given 0, expected 1)\n\
         survived\n"
    );
}

#[test]
fn default_object_inspect_and_to_s_carry_address_and_ivars() {
    // Default (no override) `to_s` is `#<Class:0xADDR>`; `inspect` adds the
    // ivars as `@name=<inspected>` in declaration order. Addresses are
    // normalized in-program (as the corpus tests do) so the expectation is
    // stable. A frozen-mutation FrozenError message carries the same inspect.
    let result = run_ruby(
        r#"
        def norm(s) = s.gsub(/0x[0-9a-f]+/, "0xADDR")
        class Widget
          def initialize(n); @name = n; @size = 3; end
        end
        w = Widget.new("gadget")
        puts norm(w.to_s)
        puts norm(w.inspect)
        class Empty; end
        puts norm(Empty.new.inspect)
        puts norm(Object.new.inspect)
        class Frozen
          attr_accessor :v
          def initialize; @v = 1; freeze; end
        end
        begin
          Frozen.new.v = 2
        rescue FrozenError => e
          puts norm(e.message)
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "#<Widget:0xADDR>\n\
         #<Widget:0xADDR @name=\"gadget\", @size=3>\n\
         #<Empty:0xADDR>\n\
         #<Object:0xADDR>\n\
         can't modify frozen Frozen: #<Frozen:0xADDR @v=1>\n"
    );
}

#[test]
fn kwargs_and_double_splats_preserve_source_order() {
    // The KwArg merge: `Call`/`HashLit`/`Yield` carry ONE ordered list of
    // literal pairs and `**h` double-splats. Ruby's Hash is insertion-ordered
    // and the merge is left-to-right last-wins, so the interleaving is
    // observable. The old two-field split got the order wrong for a splat
    // before a pair and couldn't represent two splats at all. All
    // oracle-verified against ruby 4.0.5.
    let result = run_ruby(
        r#"
        def capture(**h) = h
        p capture(**{a: 1, b: 2}, c: 3)   # splat before pair
        p capture(c: 3, **{a: 1, b: 2})   # pair before splat
        p capture(**{x: 1}, **{y: 2})     # two splats
        p capture(**{x: 1}, **{x: 2})     # last-wins on dup key
        p capture(**{a: 1}, b: 2, **{c: 3})
        h = {a: 1}
        p({**h, b: 2})
        p({b: 2, **h})
        def y
          yield 1, **{k: 2, j: 3}
        end
        y { |*a| p a }
        class HasToHash
          def to_hash = {z: 99}
        end
        p capture(**HasToHash.new, w: 1)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "{a: 1, b: 2, c: 3}\n\
         {c: 3, a: 1, b: 2}\n\
         {x: 1, y: 2}\n\
         {x: 2}\n\
         {a: 1, b: 2, c: 3}\n\
         {a: 1, b: 2}\n\
         {b: 2, a: 1}\n\
         [1, {k: 2, j: 3}]\n\
         {z: 99, w: 1}\n"
    );
}

#[test]
fn object_new_builds_a_distinct_boxed_sentinel() {
    // `Object.new` emitted `Arc::new(spinel_rt::Object)` -- using the struct
    // `Object` (a private-field struct, not a unit struct) as a value, an
    // E0423 that never compiled. It must be `Object::default()`. Each call is
    // a fresh Arc, so two sentinels are distinct by identity.
    let result = run_ruby(
        r#"
        A = Object.new
        B = Object.new
        p A == B
        p A == A
        p A.is_a?(Object)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "false\ntrue\ntrue\n");
}

#[test]
fn a_parenthesized_body_ending_in_self_returns_self() {
    // `def m = (@x = 2; self)` -- a `Seq` whose tail is `self`. The `Seq`
    // block emitted its tail unboxed (`Arc<Concrete>`) while every consumer
    // of a `Seq` (which always infers `Poly`) expects a boxed `RubyValue` --
    // an E0308 in `Ok(...)` position. The `Seq` now boxes its own tail.
    let result = run_ruby(
        r#"
        class C
          def initialize = (@x = 1)
          def tap_self = (@x = 2; self)
          attr_reader :x
        end
        c = C.new
        p c.tap_self.equal?(c)
        p c.x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\n2\n");
}

#[test]
fn user_object_freeze_frozen_and_custom_freeze_returning_self() {
    // The `freeze_user_object` scenario end to end: a bareword self-`freeze`
    // in `initialize` sets real state, `frozen?` reads it back, mutation
    // after freeze raises FrozenError, and a user `def freeze = (@log =
    // "custom"; self)` (the `Seq`-tail-`self` shape whose codegen boxing was
    // the E0308) returns self, runs its body, and -- since it overrides the
    // real freeze -- leaves the object UNfrozen. (The FrozenError message's
    // inspect tail `#<Sealed:0x.. @x=1>` is a separate default-inspect gap,
    // so this asserts the message PREFIX, not the address/ivar detail.)
    let result = run_ruby(
        r##"
        class Sealed
          attr_accessor :x
          def initialize
            @x = 1
            freeze
          end
        end
        o = Sealed.new
        p o.frozen?
        begin
          o.x = 2
        rescue FrozenError => e
          puts "FrozenError: #{e.message.start_with?('can\'t modify frozen Sealed')}"
        end
        p o.x

        class OwnFreeze
          attr_reader :log
          def initialize = (@log = "clean")
          def freeze = (@log = "custom"; self)
        end
        f = OwnFreeze.new
        p f.freeze.equal?(f)
        p f.log
        p f.frozen?
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nFrozenError: true\n1\ntrue\n\"custom\"\nfalse\n"
    );
}

#[test]
fn initialize_takes_the_full_param_shapes() {
    // `.new` was the last call site still binding arguments by hand, and it
    // only handled required + optional -- `def initialize(*values)` was a
    // flat compile-time rejection.
    let result = run_ruby(
        r#"
        class Splat
          def initialize(*v); @v = v; end
          attr_reader :v
        end
        p Splat.new.v
        p Splat.new(1).v
        p Splat.new(1, 2, 3).v

        class Mixed
          def initialize(a, b = 5, *rest, last)
            @all = [a, b, rest, last]
          end
          attr_reader :all
        end
        p Mixed.new(1, 9).all
        p Mixed.new(1, 2, 3, 4, 9).all

        class Kw
          def initialize(a, k:, j: 7, **rest)
            @all = [a, k, j, rest]
          end
          attr_reader :all
        end
        p Kw.new(1, k: 2).all
        p Kw.new(1, k: 2, j: 3, z: 4).all

        class Post
          def initialize(a, *m, y, z); @all = [a, m, y, z]; end
          attr_reader :all
        end
        p Post.new(1, 2, 3, 4, 5).all
        p Post.new(1, 2, 3).all
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[]\n[1]\n[1, 2, 3]\n[1, 5, [], 9]\n[1, 2, [3, 4], 9]\n\
         [1, 2, 7, {}]\n[1, 2, 3, {z: 4}]\n[1, [2, 3], 4, 5]\n[1, [], 2, 3]\n"
    );
}

#[test]
fn trailing_keywords_become_a_positional_hash_when_the_callee_has_no_keyword_params() {
    // Ruby's keywords-to-positional-hash conversion, still true in 3+: with
    // no keyword params and no `**kwrest` declared, trailing keywords are
    // not keywords at all -- they are one positional Hash. This is what
    // makes the classic options-hash idiom work, and it was raising
    // `unknown keyword: :a` for a method that has no keywords to be
    // unknown. A callee that DOES declare keywords keeps the real check.
    let result = run_ruby(
        r#"
        def m(opts = {}); opts; end
        p m(a: 1, b: 2)
        p m({a: 1})
        p m
        class C
          def initialize(opts = {}); @o = opts; end
          attr_reader :o
        end
        p C.new(a: 1).o
        def k(x:); x; end
        begin
          k(x: 1, zz: 2)
        rescue ArgumentError => e
          puts "ArgumentError: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "{a: 1, b: 2}\n{a: 1}\n{}\n{a: 1}\nArgumentError: unknown keyword: :zz\n"
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
fn reopening_enumerable_reaches_every_includer() {
    // Enumerable is a RUST-implemented builtin, yet reopenable (D3): an added
    // method registers as a value method on the module id and the MRO walk
    // finds it for every includer, its body free to drive the native
    // Enumerable protocol (`reduce`) on the receiver.
    let result = support::run_ruby(
        r#"
        module Enumerable
          def my_join
            reduce("") { |acc, x| acc + x.to_s }
          end
        end
        puts [1, 2, 3].my_join
        puts (1..3).my_join
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "123\n123\n");
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

#[test]
fn string_tr_duplicate_from_char_uses_the_last_mapping() {
    // A char repeated in `from` takes its LAST corresponding `to` char
    // (CRuby's rule) -- previously the FIRST mapping wrongly won.
    let result = support::run_ruby(
        r#"
        p "a___b".tr("___", ".+-")
        p "abcaa".tr("aa", "xy")
        p "abcd".tr("abc", "x")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"a---b\"\n\"ybcyy\"\n\"xxxd\"\n");
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
fn top_level_ivars_live_on_the_main_object() {
    // Was rejected as "the `main` object has no ivar storage". It has some
    // now (`dispatch::Object`'s name-keyed map), so a top-level `@x` -- read
    // or written, at the top level or from a top-level `def`, which is a
    // private method of Object whose self IS main -- is ordinary state.
    // A never-assigned one reads nil rather than raising.
    let result = run_ruby(
        r#"
        @x = 1
        p @x
        p @never
        def read_it; @x; end
        p read_it
        def bump; @count = (@count || 0) + 1; end
        bump
        bump
        p @count
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\nnil\n1\n2\n");
}

#[test]
fn a_top_level_def_touching_an_ivar_does_not_taint_builtins() {
    // `mro::materialize_methods` collects ivars from every ancestor, but a
    // BUILTIN never materializes Object's methods -- so the ivar loop has to
    // skip Object for builtins exactly as the method loop does. It didn't,
    // which made this program fail to compile with a rejection blaming
    // `Integer` for a `@count` that Integer has nothing to do with.
    let result = run_ruby(
        r#"
        def bump; @count = (@count || 0) + 1; end
        bump
        p @count
        p 1 + 2
        p "s".length
        p [1, 2].map { |i| i * 2 }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n3\n1\n[2, 4]\n");
}

#[test]
fn return_break_and_next_with_several_values_build_an_implicit_array() {
    // A single SPLAT is the case a plain "one argument -> pass it through"
    // rule gets wrong: `return *a` with `a == [1]` is `[1]`, not `1`.
    let result = run_ruby(
        r#"
        def two; return 1, 2; end
        p two
        def three; return 1, 2, 3; end
        p three
        def splat_ret(a); return *a; end
        p splat_ret([1, 2])
        p splat_ret([1])
        def mixed(a); return 1, *a; end
        p mixed([2, 3])
        p([1].each { break 1, 2 })
        p([[1, 2]].map { |a, b| next a, b })
        def none; return; end
        p none
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2]\n[1, 2, 3]\n[1, 2]\n[1]\n[1, 2, 3]\n[1, 2]\n[[1, 2]]\nnil\n"
    );
}

#[test]
fn a_parenthesized_multi_statement_expression_answers_its_last_statement() {
    // `(a; b)` introduces NO scope: `a` below is still readable afterwards,
    // which is why this lowers to a plain `Seq` rather than anything that
    // pushes a scope.
    let result = run_ruby(
        r#"
        x = (1; 2; 3)
        p x
        y = (a = 5; a * 2)
        p y
        p a
        p((puts "side"; 42))
        p [(1; 2), 3]
        z = (
          q = 7
          q + 1
        )
        p z
        p((1; (2; 3)))
        w = (if true then "yes" else "no" end; "after")
        p w
        p (5)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "3\n10\n5\nside\n42\n[2, 3]\n8\n3\n\"after\"\n5\n"
    );
}

#[test]
fn interpolation_takes_multi_statement_empty_and_braceless_forms() {
    let result = run_ruby(
        r##"
        p "v=#{1; 2}"
        p "v=#{a = 3; a * 2}"
        p a
        p "x#{}y"
        $g = "glob"
        class C
          def initialize; @iv = "ivar"; end
          def show; "iv=#@iv g=#$g"; end
        end
        p C.new.show
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"v=2\"\n\"v=6\"\n3\n\"xy\"\n\"iv=ivar g=glob\"\n");
}

#[test]
fn block_locals_shadow_an_enclosing_local_and_never_write_back() {
    let result = run_ruby(
        r#"
        sum = 99
        [1].each { |x; sum| sum = x }
        p sum

        a = 1
        b = 2
        [0].each { |z; a, b| a = 7; b = 8 }
        p [a, b]

        n = 100
        [10, 20].each { |v; n| n = v }
        p n

        r = 0
        [5].each do |i|
          [9].each { |j; r| r = j }
          r = i
        end
        p r
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "99\n[1, 2]\n100\n5\n");
}

#[test]
fn a_block_local_resets_to_nil_on_every_invocation() {
    // The part that makes a block-local more than a naming convention, and
    // the reason it can't be hoisted out of the per-call prologue: `total`
    // is nil again at the top of EVERY call, so this never accumulates.
    let result = run_ruby(
        r#"
        total = 42
        [1, 2, 3].each { |x; total| total = (total || 0) + x }
        p total

        outs = []
        [1, 2].each { |x; acc| acc ||= []; acc << x; outs << acc.dup }
        p outs
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n[[1], [2]]\n");
}

#[test]
fn block_locals_work_on_the_times_inline_path_and_in_lambdas() {
    // `n.times { }` on an integer literal is spliced inline as a native Rust
    // loop rather than becoming a real Proc, so it binds its params at its
    // own site and needs the block-local declaration applied there too.
    let result = run_ruby(
        r#"
        n = "outer"
        3.times { |i; n| n = i }
        p n

        f = ->(x; t) { t = x * 2; t }
        p f.call(5)

        q = "kept"
        [[1, 2, 3]].each { |(x, y), *r; q| q = x }
        p q
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"outer\"\n10\n\"kept\"\n");
}

#[test]
fn yield_takes_splat_arguments() {
    // `HirNode::Yield` carries the same `Vec<ArrayElem>` a Call's positional
    // args do, so a splat flattens at runtime and the block then binds from
    // the result through its ordinary parameter machinery -- auto-splat,
    // rest, and nil-padding of surplus params all included.
    let result = run_ruby(
        r#"
        def m(*a); yield(*a); end
        p(m(1, 2) { |x, y| [x, y] })
        p(m(1) { |x, y| [x, y] })
        p(m() { |x, y| [x, y] })

        def mix(*a); yield(0, *a, 9); end
        p(mix(1, 2) { |*z| z })

        def empty; yield(*[]); end
        p(empty { |*z| z })

        def kwmix(*a); yield(*a, k: 1); end
        p(kwmix(1) { |x, k:| [x, k] })

        class W
          def initialize(n); @n = n; end
          attr_reader :n
        end
        def objs; yield(*[W.new(1), W.new(2)]); end
        p(objs { |a, b| a.n + b.n })
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2]\n[1, nil]\n[nil, nil]\n[0, 1, 2, 9]\n[]\n[1, 1]\n3\n"
    );
}

#[test]
fn a_runtime_empty_double_splat_contributes_no_trailing_hash() {
    // `foo(1, **h)` with an empty `h` passes just `1` -- the trailing hash
    // is pushed only when non-empty. This is about a RUNTIME-empty hash, not
    // the literal `**{}`, so it can't be decided at lowering time. Pushing
    // unconditionally silently handed the callee an extra `{}`.
    let result = run_ruby(
        r#"
        def foo(*z); z; end
        def c(h); foo(1, **h); end
        p c({})
        p c({k: 2})

        def kw(*z); z; end
        def d(h); kw(**h); end
        p d({})

        def both(h); foo(1, k: 1, **h); end
        p both({})
        p both({j: 2})
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1]\n[1, {k: 2}]\n[]\n[1, {k: 1}]\n[1, {k: 1, j: 2}]\n"
    );
}

#[test]
fn enumerable_chunking_and_slicing() {
    let result = run_ruby(
        r#"
        a = [1, 2, 4, 9, 10, 11, 12, 15]
        p a.slice_when { |i, j| i + 1 != j }.to_a
        p a.chunk_while { |i, j| i + 1 == j }.to_a
        p [1, 1, 2, 3, 3].chunk_while { |i, j| i == j }.to_a
        p [1, 2, 3, 4, 5].slice_before { |x| x.even? }.to_a
        p [1, 2, 3, 4, 5].slice_after { |x| x.even? }.to_a
        p [1, 2, 3, 4, 5].slice_before(3).to_a
        p [1, 2, 3, 4, 5].slice_after(3).to_a
        p ["a", "b1", "c"].slice_before(/\d/).to_a
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[[1, 2], [4], [9, 10, 11, 12], [15]]\n\
         [[1, 2], [4], [9, 10, 11, 12], [15]]\n\
         [[1, 1], [2], [3, 3]]\n\
         [[1], [2, 3], [4, 5]]\n\
         [[1, 2], [3, 4], [5]]\n\
         [[1, 2], [3, 4, 5]]\n\
         [[1, 2, 3], [4, 5]]\n\
         [[\"a\"], [\"b1\", \"c\"]]\n"
    );
}

#[test]
fn enumerable_grep_zip_and_minmax_by() {
    let result = run_ruby(
        r#"
        p (1..10).grep(3..5)
        p [1, "a", 2, "b"].grep(Integer)
        p [1, "a", 2, "b"].grep(Integer) { |x| x * 10 }
        p [1, "a", 2, "b"].grep_v(Integer)
        p [1, "a", 2, "b"].grep_v(Integer) { |x| x + "!" }
        p [1, 2, 3].zip([4, 5, 6])
        p [1, 2, 3].zip([4, 5], [6])
        p([1, 2, 3].zip([4, 5, 6]) { |x| })
        p [1, 2, 3, 4].minmax_by { |x| -x }
        p [].minmax_by { |x| x }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[3, 4, 5]\n[1, 2]\n[10, 20]\n[\"a\", \"b\"]\n[\"a!\", \"b!\"]\n\
         [[1, 4], [2, 5], [3, 6]]\n[[1, 4, 6], [2, 5, nil], [3, nil, nil]]\n\
         nil\n[4, 1]\n[nil, nil]\n"
    );
}

#[test]
fn array_combinatorics_and_binary_search() {
    let result = run_ruby(
        r#"
        p [1, 2, 3].combination(2).to_a
        p [1, 2, 3].combination(0).to_a
        p [1, 2, 3].combination(4).to_a
        p [1, 2, 3].permutation(2).to_a
        p [1, 2].permutation.to_a
        r = []
        [1, 2, 3].combination(2) { |c| r << c }
        p r
        p [1, 2, 3].each_index.to_a
        p [1, 2, 3, 4].bsearch { |x| x >= 3 }
        p [1, 2, 3, 4].bsearch { |x| x >= 9 }
        p [1, 2, 3, 4].bsearch_index { |x| x >= 3 }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[[1, 2], [1, 3], [2, 3]]\n[[]]\n[]\n\
         [[1, 2], [1, 3], [2, 1], [2, 3], [3, 1], [3, 2]]\n[[1, 2], [2, 1]]\n\
         [[1, 2], [1, 3], [2, 3]]\n[0, 1, 2]\n3\nnil\n2\n"
    );
}

#[test]
fn array_cycle_bang_forms_and_values_at() {
    let result = run_ruby(
        r#"
        p [1, 2, 3].cycle(2).to_a
        c = []
        [1, 2].cycle(2) { |x| c << x }
        p c
        p [1, 2].cycle(0).to_a
        p [[1, [2, 3]], [4]].flatten!
        a = [1, 2]
        p a.flatten!
        b = [3, 1, 2]
        b.sort_by! { |x| -x }
        p b
        p [1, 2, 3].values_at(0, 2, 5)
        p [1, 2, 3].values_at(0..1)
        p [1, 2, 3, 4, 5].values_at(3..9)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2, 3, 1, 2, 3]\n[1, 2, 1, 2]\n[]\n[1, 2, 3, 4]\nnil\n[3, 2, 1]\n\
         [1, 3, nil]\n[1, 2]\n[4, 5, nil, nil, nil, nil, nil]\n"
    );
}

#[test]
fn array_new_takes_a_size_default_and_block() {
    // The block form has to reach the runtime allocator, so `Array.new`
    // joins the block-keeping set in lowering -- routing it through
    // `HirNode::New` (which has no block slot) silently answered nils.
    let result = run_ruby(
        r#"
        p Array.new(3) { |i| i * 2 }
        p Array.new(2, "x")
        p Array.new(3)
        p Array.new
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[0, 2, 4]\n[\"x\", \"x\"]\n[nil, nil, nil]\n[]\n"
    );
}

#[test]
fn pop_and_shift_take_a_count() {
    // `pop`/`shift` answer ONE element; `pop(n)`/`shift(n)` answer an ARRAY
    // -- a different return type, not just a different count, which is why
    // the no-arg form can't be `pop(1)`. Both were `arity!(args, 0)`, so the
    // count form raised a spurious ArgumentError on a call Ruby accepts.
    let result = run_ruby(
        r#"
        a = [1, 2, 3, 4]
        p a.pop(2)
        p a
        b = [1, 2, 3, 4]
        p b.shift(2)
        p b
        p [1, 2].pop(5)
        p [1, 2].pop(0)
        c = [1, 2]
        p c.shift(0)
        p c
        p [1, 2].pop
        p [].pop
        p [].shift
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[3, 4]\n[1, 2]\n[1, 2]\n[3, 4]\n[1, 2]\n[]\n[]\n[1, 2]\n2\nnil\nnil\n"
    );
}

#[test]
fn each_entry_packs_multi_value_yields_where_each_passes_them_through() {
    // The entire difference between `each_entry` and `each`, and it is only
    // observable when the receiver's own `each` yields MORE THAN ONE value.
    let result = run_ruby(
        r#"
        class Multi
          include Enumerable
          def each
            yield 1
            yield 2, 3
            yield
          end
        end
        r = []
        Multi.new.each_entry { |x| r << x }
        p r
        s = []
        Multi.new.each { |x| s << x }
        p s
        p Multi.new.each_entry.to_a
        p [1, 2].each_entry.to_a
        h = []
        ({a: 1}).each_entry { |e| h << e }
        p h
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, [2, 3], nil]\n[1, 2, nil]\n[1, [2, 3], nil]\n[1, 2]\n[[:a, 1]]\n"
    );
}

#[test]
fn new_on_a_class_with_no_initialize_rejects_arguments() {
    // `Object#initialize` takes none, so passing any is an ArgumentError --
    // it was silently DROPPING them and constructing happily. Rescuable and
    // raised at runtime (plan G1), covering both the static `.new` path and
    // the dynamic one through a class-valued variable.
    let result = run_ruby(
        r#"
        class Bare; end
        begin
          Bare.new(1, 2)
        rescue ArgumentError => e
          puts "ArgumentError: #{e.message}"
        end
        k = Bare
        begin
          k.new(9)
        rescue ArgumentError => e
          puts "dyn: #{e.message}"
        end
        p Bare.new.class
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "ArgumentError: wrong number of arguments (given 2, expected 0)\n\
         dyn: wrong number of arguments (given 1, expected 0)\nBare\n"
    );
}

#[test]
fn the_last_match_specials_read_off_the_most_recent_match() {
    let result = run_ruby(
        r#"
        if "hello world" =~ /(\w+)\s(\w+)/
          p $1
          p $2
          p $3
          p $&
          p $`
          p $'
          p $~[0]
          p $~.class
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"hello\"\n\"world\"\nnil\n\"hello world\"\n\"\"\n\"\"\n\"hello world\"\nMatchData\n"
    );
}

#[test]
fn a_failed_match_clears_the_last_match_specials() {
    // They are RESET, not left holding the previous match -- the property
    // that makes `if s =~ re then $1 end` safe to reuse in a loop.
    let result = run_ruby(
        r#"
        "ab" =~ /(a)/
        p $1
        "zzz" =~ /(\d+)/
        p $1
        p $&
        p $~
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"a\"\nnil\nnil\nnil\n");
}

#[test]
fn match_p_does_not_touch_the_last_match_but_match_does() {
    // `match?` is specifically the allocation-free predicate: it builds no
    // MatchData and so leaves the slot alone. `Regexp#match` sets it.
    let result = run_ruby(
        r#"
        "abc" =~ /b/
        p $&
        "xyz".match?(/y/)
        p $&
        /(\d)/.match("a1")
        p $1
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"b\"\n\"b\"\n\"1\"\n");
}

#[test]
fn matchdata_methods_work_on_a_dynamically_typed_receiver() {
    // `$~` can be nil (whenever the last match failed), so it never infers
    // as `TyKind::MatchData` and can't take codegen's static MatchData fast
    // path -- it dispatches dynamically, which needs a real runtime table.
    // `$~[0]` raised NoMethodError while `re.match(s)[0]` worked.
    let result = run_ruby(
        r#"
        "hello" =~ /e(l+)(o)/
        m = $~
        p m[0]
        p m[1]
        p m.captures
        p m.pre_match
        p m.post_match
        p m.to_a
        p m.string
        p m.to_s
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"ello\"\n\"ll\"\n[\"ll\", \"o\"]\n\"h\"\n\"\"\n[\"ello\", \"ll\", \"o\"]\n\"hello\"\n\"ello\"\n"
    );
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

// --- This session's correctness fixes. Each covers a behavior the
// `examples/*.rb` fixtures also exercise end to end against ruby 4.0.5;
// these pin the specific shape that was broken, so a regression names
// itself rather than showing up as an example-wide diff.

#[test]
fn a_block_auto_splats_a_lone_array_across_its_positional_params() {
    // CRuby's `has_lead && !ambiguous_param0` rule -- see
    // `codegen::params::auto_splats`, whose unit tests cover every shape.
    let result = run_ruby(
        r#"
        def one(v)
          yield v
        end
        p(one([1, 2]) { |a, b| [a, b] })
        p(one([1, 2]) { |a| a })
        p(one([1, 2]) { |*a| a })
        p(one([1, 2]) { |a, *b| [a, b] })
        p(one([1, 2]) { |a, **k| [a, k] })
        p [[1, 2], [3, 4]].map { |a, b| a + b }
        p({ x: 1 }.map { |k, v| [k, v] })
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2]\n[1, 2]\n[[1, 2]]\n[1, [2]]\n[[1, 2], {}]\n[3, 7]\n[[:x, 1]]\n"
    );
}

#[test]
fn a_lambda_never_auto_splats_and_checks_its_arity() {
    let result = run_ruby(
        r#"
        strict = ->(a, b) { [a, b] }
        p strict.call(1, 2)
        begin
          strict.call([1, 2])
        rescue ArgumentError => e
          puts "ArgumentError"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[1, 2]\nArgumentError\n");
}

#[test]
fn proc_arity_and_lambda_p_report_the_blocks_own_signature() {
    let result = run_ruby(
        r#"
        p ->(a, b) {}.arity
        p ->(a, b = 1) {}.arity
        p proc { |a, b = 1| }.arity
        p proc { |a, *b| }.arity
        p ->(a, b:) {}.arity
        p ->(a, **k) {}.arity
        p ->() {}.lambda?
        p proc {}.lambda?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n-2\n1\n-2\n2\n-2\ntrue\nfalse\n");
}

#[test]
fn proc_curry_collects_arguments_and_each_step_is_reusable() {
    let result = run_ruby(
        r#"
        add = ->(a, b, c) { a + b + c }
        p add.curry[1][2][3]
        p add.curry[1, 2][3]
        step = add.curry[10]
        p step[1][2]
        p step[3][4]
        p add.curry.arity
        p add.curry.lambda?
        p proc { |a, b| a * b }.curry[3][4]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n6\n13\n17\n-1\ntrue\n12\n");
}

#[test]
fn proc_new_with_a_block_is_that_block() {
    let result = run_ruby(
        r#"
        p Proc.new { |x| x * 2 }.call(4)
        p Proc.new { |a, b| [a, b] }.arity
        p Proc.new {}.lambda?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "8\n2\nfalse\n");
}

#[test]
fn array_index_assign_splices_start_length_and_range_spans() {
    let result = run_ruby(
        r#"
        a = [1, 2, 3, 4]
        a[1, 2] = [:x, :y, :z]
        p a
        b = [1, 2, 3]
        b[1, 0] = [:ins]
        p b
        c = [1, 2, 3, 4]
        c[1..2] = [:r]
        p c
        d = [1, 2, 3, 4]
        d[1...3] = [:e]
        p d
        e = [1, 2, 3]
        e[0, 2] = :scalar
        p e
        f = [1, 2, 3]
        p(f[0, 1] = [:v])
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, :x, :y, :z, 4]\n[1, :ins, 2, 3]\n[1, :r, 4]\n[1, :e, 4]\n[:scalar, 3]\n[:v]\n"
    );
}

#[test]
fn implicit_conversions_are_duck_typed_through_to_ary_to_hash_and_to_proc() {
    let result = run_ruby(
        r#"
        class Pair
          def initialize(a, b)
            @a = a
            @b = b
          end
          def to_ary = [@a, @b]
        end
        class Opts
          def to_hash = { a: 1, b: 2 }
        end
        class Dbl
          def to_proc = ->(x) { x * 2 }
        end

        # to_ary: block auto-splat, multi-assign, and a splice RHS.
        def one(v)
          yield v
        end
        p(one(Pair.new(1, 2)) { |a, b| [a, b] })
        x, y = Pair.new(3, 4)
        p [x, y]
        arr = [1, 2, 3]
        arr[1, 1] = Pair.new(7, 8)
        p arr

        # to_hash: a `**` splat.
        def take(a:, b:) = [a, b]
        p take(**Opts.new)

        # to_proc: an `&` block argument.
        p [1, 2].map(&Dbl.new)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2]\n[3, 4]\n[1, 7, 8, 3]\n[1, 2]\n[2, 4]\n"
    );
}

#[test]
fn a_multi_assign_from_a_non_array_binds_one_value_and_nil_fills() {
    let result = run_ruby(
        r#"
        a, b = 5
        p [a, b]
        c, d = [1, 2]
        p [c, d]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[5, nil]\n[1, 2]\n");
}

#[test]
fn bare_super_forwards_a_child_optional_into_a_parent_required_slot() {
    // The child's and parent's parameter SHAPES differ, so the forwarding
    // must flatten positionally rather than match bucket-for-bucket.
    let result = run_ruby(
        r##"
        class Parent
          def greet(name)
            "Parent(#{name})"
          end
        end
        class Child < Parent
          def greet(name = "default")
            super
          end
        end
        puts Child.new.greet
        puts Child.new.greet("given")
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Parent(default)\nParent(given)\n");
}

#[test]
fn super_with_a_literal_block_drives_the_parents_yield() {
    let result = run_ruby(
        r##"
        class Parent
          def run
            yield
          end
          def each_twice
            yield 1
            yield 2
          end
        end
        class Child < Parent
          def run
            super { "from-child" }
          end
          def each_twice
            super { |v| puts "got #{v}" }
          end
        end
        puts Child.new.run
        Child.new.each_twice
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "from-child\ngot 1\ngot 2\n");
}

#[test]
fn super_with_no_literal_block_forwards_the_callers_block() {
    let result = run_ruby(
        r#"
        class Parent
          def run
            yield
          end
        end
        class Child < Parent
          def run
            super
          end
        end
        puts(Child.new.run { "from-caller" })
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "from-caller\n");
}

#[test]
fn raise_with_a_bare_class_runs_the_classs_own_initialize() {
    // `raise E` must CONSTRUCT via `E.new` -- running defaults and `super`
    // -- rather than short-cutting the message to the class name. The
    // class-name default lives in the prelude's `Exception#to_s`.
    let result = run_ruby(
        r##"
        class E < StandardError
          def initialize(m = "def")
            super
          end
        end
        class F < StandardError
          def initialize(m = "dd")
            super("wrapped: #{m}")
          end
        end
        class G < StandardError; end

        begin; raise E; rescue => e; p e.message; end
        begin; raise E, "x"; rescue => e; p e.message; end
        begin; raise E.new; rescue => e; p e.message; end
        begin; raise F; rescue => e; p e.message; end
        begin; raise G; rescue => e; p e.message; end
        begin; raise StandardError; rescue => e; p e.message; end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"def\"\n\"x\"\n\"def\"\n\"wrapped: dd\"\n\"G\"\n\"StandardError\"\n"
    );
}

#[test]
fn an_exception_ivar_set_by_a_custom_initialize_survives_the_raise() {
    let result = run_ruby(
        r##"
        class Detailed < StandardError
          def initialize(field)
            @field = field
            super("invalid #{field}")
          end
          attr_reader :field
        end
        begin
          raise Detailed.new("email")
        rescue Detailed => e
          p [e.message, e.field]
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[\"invalid email\", \"email\"]\n");
}

#[test]
fn a_top_level_def_is_a_private_method_of_object() {
    // Callable by implicit self everywhere, invisible to `respond_to?`,
    // reachable via `send`, and a NoMethodError with an explicit receiver.
    let result = run_ruby(
        r##"
        def greet(name)
          "Hello, #{name}!"
        end
        class Speaker
          def speak
            greet("instance")
          end
        end
        puts greet("top")
        puts Speaker.new.speak
        p self.respond_to?(:greet)
        p self.respond_to?(:greet, true)
        p Speaker.new.respond_to?(:greet)
        p send(:greet, "send")
        p Speaker.new.send(:greet, "on-instance")
        begin
          Speaker.new.greet("explicit")
        rescue NoMethodError
          puts "NoMethodError"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Hello, top!\nHello, instance!\nfalse\ntrue\nfalse\n\"Hello, send!\"\n\"Hello, on-instance!\"\nNoMethodError\n"
    );
}

#[test]
fn a_top_level_def_is_callable_from_a_class_body_and_a_class_method() {
    // `self` in a class body/class method is the CLASS OBJECT -- there's no
    // concrete receiver to clone, so implicit-self dispatch must go through
    // `RubyValue::Class`, not an invalid `self.clone()`.
    let result = run_ruby(
        r##"
        def helper(tag)
          "helped-#{tag}"
        end
        class AtBody
          RESULT = helper("body")
        end
        class Factory
          def self.build
            helper("class-method")
          end
        end
        puts AtBody::RESULT
        puts Factory.build
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "helped-body\nhelped-class-method\n");
}

#[test]
fn a_class_named_after_a_rust_prelude_type_compiles_and_behaves() {
    // `class Vec` would otherwise define a crate-root `Vec` struct shadowing
    // Rust's own, breaking every `Vec<RubyValue>` in the generated program.
    let result = run_ruby(
        r##"
        class Vec
          def initialize(x, y)
            @x = x
            @y = y
          end
          attr_reader :x, :y
          def +(other) = Vec.new(@x + other.x, @y + other.y)
          def to_s = "Vec(#{@x}, #{@y})"
        end
        class Option
          def initialize(v) = @v = v
          def some? = !@v.nil?
        end
        class Box
          def initialize(v) = @v = v
          def get = @v
        end
        puts(Vec.new(1, 2) + Vec.new(10, 20))
        p Vec.new(1, 2).class.name
        p Option.new(nil).some?
        p Box.new(:b).get
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Vec(11, 22)\n\"Vec\"\nfalse\n:b\n");
}

#[test]
fn ruby_underscore_is_an_ordinary_readable_local() {
    // Rust's `_` isn't a named binding at all (`let mut _` doesn't parse,
    // and a macro `$x:ident` matcher rejects it), so it must be mangled.
    let result = run_ruby(
        r#"
        _ = 10
        p _
        _ = _ + 5
        p _
        [[1, :a]].each { |n, _| p n }
        [[1, :a]].each { |_, sym| p sym }
        _, second = [:first, :second]
        p [second, _]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n15\n1\n:a\n[:second, :first]\n");
}

#[test]
fn a_statically_wrong_arity_call_raises_at_runtime_and_dead_code_stays_silent() {
    // CRuby's behavior: the error belongs to the CALL, not the program.
    let result = run_ruby(
        r##"
        module M
          def self.run!(a, b, c, d)
            "#{a}#{b}#{c}#{d}"
          end
        end
        begin
          M.run!(1, 2, false)
        rescue ArgumentError => e
          puts "ArgumentError: #{e.message}"
        end
        puts M.run!(1, 2, 3, 4)
        def never_called
          M.run!(1)
        end
        puts "done"
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "ArgumentError: wrong number of arguments (given 3, expected 4)\n1234\ndone\n"
    );
}

#[test]
fn method_objects_bind_a_receiver_and_convert_to_procs() {
    let result = run_ruby(
        r##"
        def double(x) = x * 2
        m = method(:double)
        p m.call(21)
        p m.(21)
        p m[21]
        p m.name
        p [1, 2].map(&method(:double))

        class Greeter
          def initialize(g) = @g = g
          def greet(name) = "#{@g}, #{name}!"
        end
        g = Greeter.new("Hello")
        gm = g.method(:greet)
        p gm.call("Ada")
        p gm.receiver.equal?(g)
        p "hello".method(:upcase).call
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "42\n42\n42\n:double\n[2, 4]\n\"Hello, Ada!\"\ntrue\n\"HELLO\"\n"
    );
}

#[test]
fn method_of_an_unknown_name_raises_name_error_at_construction() {
    let result = run_ruby(
        r#"
        begin
          "str".method(:definitely_not_defined)
        rescue NameError => e
          puts e.message
        end
        class WithPrivate
          private
          def secret = "shh"
        end
        p WithPrivate.new.method(:secret).call
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "undefined method 'definitely_not_defined' for class 'String'\n\"shh\"\n"
    );
}

#[test]
fn a_fiber_reached_through_a_dynamic_receiver_still_resumes() {
    // The static fast path emits `fiber_resume` directly; a fiber held in a
    // collection/ivar dispatches through the runtime's Fiber table instead.
    let result = run_ruby(
        r#"
        holder = [Fiber.new { Fiber.yield 1; 2 }]
        p holder[0].resume
        p holder[0].alive?
        p holder[0].resume
        p holder[0].alive?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\ntrue\n2\nfalse\n");
}

#[test]
fn a_local_reassigned_to_a_different_class_still_compiles() {
    // `local_types` is read flow-insensitively, so a retyped local must
    // widen to Poly rather than let a later class's identifier describe an
    // earlier read.
    let result = run_ruby(
        r#"
        class A
          def who = "A"
        end
        class B
          def who = "B"
        end
        x = A.new
        puts x.who
        x = B.new
        puts x.who
        y = 1
        y = "str"
        puts y
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "A\nB\nstr\n");
}

#[test]
fn clamp_accepts_a_range_as_well_as_two_bounds() {
    let result = run_ruby(
        r#"
        p 0.clamp(1..5)
        p 9.clamp(1..5)
        p 3.clamp(1..5)
        p 0.clamp(1..)
        p 99.clamp(..5)
        p 9.clamp(1, 5)
        begin
          9.clamp(1...5)
        rescue ArgumentError => e
          puts "ArgumentError"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n5\n3\n1\n5\n5\nArgumentError\n");
}

#[test]
fn enumerable_min_and_max_take_a_count() {
    let result = run_ruby(
        r#"
        p (1..10).min(3)
        p (1..10).max(3)
        p [5, 1, 4, 2].min(2)
        p [5, 1, 4, 2].max(2)
        p [1, 2].min(9)
        p [1, 2].min(0)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2, 3]\n[10, 9, 8]\n[1, 2]\n[5, 4]\n[1, 2]\n[]\n"
    );
}

#[test]
fn to_h_maps_elements_through_its_block() {
    let result = run_ruby(
        r#"
        p [[1, 2], [3, 4]].to_h
        p [[1, 2], [3, 4]].to_h { |a, b| [b, a] }
        p({ a: 1, b: 2 }.to_h { |k, v| [v, k] })
        p (1..3).to_h { |i| [i, i * i] }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "{1 => 2, 3 => 4}\n{2 => 1, 4 => 3}\n{1 => :a, 2 => :b}\n{1 => 1, 2 => 4, 3 => 9}\n"
    );
}

/// A parenthesized parameter destructures the value bound to its slot --
/// `|(a, b)|`, mixed with plain params, and nested arbitrarily deep. Lowered
/// as the multi-assignment it is (see `hir::Params::destructures`).
#[test]
fn parenthesized_block_params_destructure_their_slot() {
    let result = run_ruby(
        r#"
        [[1, 2], [3, 4]].each { |(a, b)| p [a, b] }
        [[1, [2, 3], 4]].each { |a, (b, c), d| p [a, b, c, d] }
        [[[1, 2], 3]].each { |(a, b), c| p [a, b, c] }
        [[1, [2, [3, 4]]]].each { |a, (b, (c, d))| p [a, b, c, d] }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2]\n[3, 4]\n[1, 2, 3, 4]\n[1, 2, 3]\n[1, 2, 3, 4]\n"
    );
}

/// A destructuring param may carry a splat, named or anonymous -- the same
/// `before`/`splat`/`after` shape every multi-assignment target group has.
#[test]
fn destructuring_params_take_splats() {
    let result = run_ruby(
        r#"
        [[1, [2, 3, 4]]].each { |a, (b, *r)| p [a, b, r] }
        [[1, 2, 3]].each { |a, (*), b| p [a, b] }
        [[[1, 2, 3], 9]].each { |(*init, last), z| p [init, last, z] }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[1, 2, [3, 4]]\n[1, 3]\n[[1, 2], 3, 9]\n");
}

/// METHOD params destructure too, not just block params.
#[test]
fn method_params_destructure() {
    // `r##"..."##`: the body contains `"#{a}`, which would close an `r#"..."#`.
    let result = run_ruby(
        r##"
        def pair((a, b)) = "#{a}-#{b}"
        puts pair([1, 2])
        def nested((a, (b, c))) = [a, b, c]
        p nested([1, [2, 3]])
        def mixed(x, (y, z)) = [x, y, z]
        p mixed(1, [2, 3])
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1-2\n[1, 2, 3]\n[1, 2, 3]\n");
}

/// A destructured name is an ordinary local: an escaping block captures it by
/// reference, so a write inside the block is visible after it returns. This
/// regressed once already -- `captures::own_param_names` kept its own copy of
/// the param-name walk and didn't know about destructures, so the name was
/// classified as a block-own local and silently read `nil`.
#[test]
fn destructured_names_are_capturable_by_escaping_blocks() {
    let result = run_ruby(
        r#"
        def capture((a, b))
          bump = -> { a += 10 }
          bump.call
          [a, b]
        end
        p capture([1, 2])

        def collect((x, y))
          out = []
          [1, 2].each { |i| out << (x * i + y) }
          out
        end
        p collect([10, 1])
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[11, 2]\n[11, 21]\n");
}

/// A lone Array argument spreads across a block's positional params -- but a
/// block with exactly ONE plain param receives it whole (`ambiguous_param0`,
/// the `each { |pair| }` idiom), and so does a lone optional. Oracle-derived;
/// see `codegen::params::auto_splats` for the full truth table.
#[test]
fn block_auto_splat_follows_the_ambiguous_param0_rule() {
    let result = run_ruby(
        r#"
        def one(x) = yield x
        one([1, 2]) { |a| p a }
        one([1, 2]) { |a = 9| p a }
        one([1, 2]) { |*a| p a }
        one([1, 2]) { |a, **k| p a }
        one([1, 2]) { |a, b| p [a, b] }
        one([1, 2]) { |a, *b| p [a, b] }
        one([1, 2]) { |*a, b| p [a, b] }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2]\n[1, 2]\n[[1, 2]]\n[1, 2]\n[1, 2]\n[1, [2]]\n[[1], 2]\n"
    );
}

/// More than one OPTIONAL param auto-splats even with no required param at
/// all, while a single optional plus a rest does not. This pair is what rules
/// out the tempting "count the positional slots" formulation of the rule.
#[test]
fn block_auto_splat_triggers_on_more_than_one_optional() {
    let result = run_ruby(
        r#"
        def one(x) = yield x
        one([1, 2]) { |a = 5, b = 4| p [a, b] }
        one([1, 2]) { |a = 5, *b| p [a, b] }
        one([1, 2]) { |a = 5, b = 4, *c| p [a, b, c] }
        one([1, 2]) { |a = 5, *b, c| p [a, b, c] }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2]\n[[1, 2], []]\n[1, 2, []]\n[1, [], 2]\n"
    );
}

/// A lambda is strict: it never auto-splats, whatever its param shape.
#[test]
fn a_lambda_never_auto_splats() {
    let result = run_ruby(
        r#"
        p(->(a) { a }.call([1, 2]))
        p(->(a, b) { [a, b] }.call(1, 2))
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[1, 2]\n[1, 2]\n");
}

/// Post params fill left-to-right from whatever the lead/optional/rest slots
/// left behind, nil-padding the tail -- they are anchored to the END of the
/// argument list only when there are enough values to reach them.
#[test]
fn post_params_fill_from_the_front_when_underfull() {
    let result = run_ruby(
        r#"
        def one(x) = yield x
        one([1, 2]) { |a, *b, c, d| p [a, b, c, d] }
        one([1, 2, 3, 4, 5]) { |a, *b, c| p [a, b, c] }
        one([1, 2]) { |a, b = 5, c = 6, d, e| p [a, b, c, d, e] }
        one([1, 2, 3, 4, 5]) { |a, b = 5, c = 6, d, e| p [a, b, c, d, e] }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, [], 2, nil]\n[1, [2, 3, 4], 5]\n[1, 5, 6, 2, nil]\n[1, 2, 3, 4, 5]\n"
    );
}

/// The same underfull clamping in a multi-assignment: `w, *x, y, z = [1, 2]`
/// is `y=2, z=nil`, not `z=2`.
#[test]
fn multi_assign_post_targets_clamp_when_underfull() {
    let result = run_ruby(
        r#"
        w, *x, y, z = [1, 2]
        p [w, x, y, z]
        a, *b, c = [1]
        p [a, b, c]
        q, r, *s, t, u = [1, 2, 3]
        p [q, r, s, t, u]
        a2, *b2, c2 = [1, 2, 3, 4]
        p [a2, b2, c2]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, [], 2, nil]\n[1, [], nil]\n[1, 2, [], 3, nil]\n[1, [2, 3], 4]\n"
    );
}

/// A trailing comma (`|a, |`) means "this block takes more than one param",
/// which turns auto-splat on and then discards everything past the named
/// ones -- exactly an anonymous rest, which is how it lowers.
#[test]
fn a_trailing_comma_param_is_an_anonymous_rest() {
    let result = run_ruby(
        r#"
        def one(x) = yield x
        one([1, 2]) { |a,| p a }
        one([1, 2, 3]) { |a, b,| p [a, b] }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n[1, 2]\n");
}

// --- plan P-B: the core classes (File, Dir, Time, Process, ENV) ------------

/// `File`'s pure-path family: string work that never touches the disk. Every
/// expectation oracle-read from ruby 4.0.5 -- including the two that read
/// like off-by-ones (a TRAILING dot IS an extension, a LEADING one is not).
#[test]
fn file_pure_path_family() {
    let result = run_ruby(
        r#"
        puts File.basename("/home/user/notes.md")
        puts File.basename("/home/user/notes.md", ".md")
        puts File.basename("/home/user/notes.md", ".*")
        puts File.basename("/a/b/")
        puts File.basename("/")
        puts File.dirname("/home/user/notes.md")
        puts File.dirname("solo")
        puts File.dirname("/x")
        p File.extname("archive.tar.gz")
        p File.extname(".bashrc")
        p File.extname("trailing.")
        p File.extname("plain")
        p File.split("/a/b/c.rb")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "notes.md\nnotes\nnotes\nb\n/\n/home/user\n.\n/\n\".gz\"\n\"\"\n\".\"\n\"\"\n[\"/a/b\", \"c.rb\"]\n"
    );
}

/// `File.join` collapses a separator at the seam rather than doubling it, and
/// flattens a nested Array argument.
#[test]
fn file_join_collapses_separators() {
    let result = run_ruby(
        r#"
        puts File.join("a", "b", "c")
        puts File.join("a/", "b")
        puts File.join("a", "/b")
        puts File.join("a/", "/b")
        puts File.join("/a", "b")
        puts File.join("a", ["b", "c"])
        p File.absolute_path?("/abs")
        p File.absolute_path?("rel")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "a/b/c\na/b\na/b\na/b\n/a/b\na/b/c\ntrue\nfalse\n"
    );
}

/// A fixed instant read in UTC -- zone-independent, so these are safe to
/// assert on any machine. Oracle-read for epoch 1700000000.
#[test]
fn time_utc_civil_fields() {
    let result = run_ruby(
        r#"
        t = Time.at(1700000000).getutc
        puts t.to_s
        p [t.year, t.month, t.day, t.hour, t.min, t.sec]
        p [t.wday, t.yday]
        p t.to_i
        p t.utc?
        p t.zone
        p t.utc_offset
        p [t.monday?, t.tuesday?]
        puts Time.at(0).getutc.to_s
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "2023-11-14 22:13:20 UTC\n[2023, 11, 14, 22, 13, 20]\n[2, 318]\n1700000000\ntrue\n\"UTC\"\n0\n[false, true]\n1970-01-01 00:00:00 UTC\n"
    );
}

/// `strftime`'s directive table, including Ruby's `-`/`_` padding flags and
/// the verbatim-unknown-directive rule.
#[test]
fn time_strftime_directives() {
    let result = run_ruby(
        r#"
        t = Time.at(1700000000).getutc
        puts t.strftime("%Y-%m-%d %H:%M:%S")
        puts t.strftime("%F %T")
        puts t.strftime("%a %A %b %B")
        puts t.strftime("%j %u %w %p %I")
        puts t.strftime("%z %Z")
        puts t.strftime("%y %C %s")
        puts t.strftime("100%% literal")
        puts t.strftime("%Q")
        jan = Time.at(1704067200).getutc
        puts jan.strftime("%m|%-m|%_m")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "2023-11-14 22:13:20\n2023-11-14 22:13:20\nTue Tuesday Nov November\n318 2 2 PM 10\n+0000 UTC\n23 20 1700000000\n100% literal\n%Q\n01|1| 1\n"
    );
}

/// `t + n` answers a Time; `t - other_time` answers a Float of seconds; `t -
/// n` answers a Time. The argument's type picks.
#[test]
fn time_arithmetic_picks_by_argument_type() {
    let result = run_ruby(
        r#"
        t = Time.at(1700000000).getutc
        p (t + 60).to_i
        p (t - 60).to_i
        p (Time.at(100) - Time.at(40))
        p (Time.at(100) - Time.at(40)).class
        p (t + 60).class
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "1700000060\n1699999940\n60.0\nFloat\nTime\n"
    );
}

/// A Float epoch is stored EXACTLY (Ruby keeps the double's true rational,
/// denominator a power of two), and `nsec` is a TRUNCATED VIEW of it. This is
/// observable: `Time.at(10.8) - 0.9` is nsec 900000000, which a `(sec, nsec)`
/// representation gets wrong (899999999) by dropping 10.8's sub-nanosecond
/// tail before subtracting. All oracle-read.
#[test]
fn time_keeps_a_float_epoch_exactly() {
    let result = run_ruby(
        r#"
        p Time.at(0.5).subsec
        p Time.at(10.8).subsec
        p Time.at(10.8).nsec
        p (Time.at(10.8) - 0.9).nsec
        p (Time.at(10.8) - 0.9).subsec
        p Time.at(1.25).to_f
        p Time.at(1700000000).getutc.subsec
        p (Time.at(100) + -1.3).usec
        p (Time.at(100) - 1.3).usec
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "(1/2)\n(225179981368525/281474976710656)\n800000000\n900000000\n(8106479329266899/9007199254740992)\n1.25\n0\n699999\n699999\n"
    );
}

/// A negative epoch floors: second -1 plus a POSITIVE sub-second remainder,
/// never second 0 minus half.
#[test]
fn time_normalizes_a_negative_float_epoch() {
    let result = run_ruby(
        r#"
        p Time.at(-0.5).to_i
        p Time.at(-0.5).nsec
        p Time.at(-1).to_i
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "-1\n500000000\n-1\n");
}

/// Time includes Comparable, so the whole ordering surface falls out of
/// `<=>`. Equality is by INSTANT, so a Time equals its own UTC copy and they
/// hash alike (which is what makes a Time usable as a Hash key).
#[test]
fn time_drives_comparable_and_hashes_by_instant() {
    let result = run_ruby(
        r#"
        p Time.at(5) < Time.at(6)
        p Time.at(6) > Time.at(5)
        p Time.at(5).between?(Time.at(1), Time.at(9))
        p Time.at(1).clamp(Time.at(2), Time.at(5)).to_i
        p [Time.at(3), Time.at(1), Time.at(2)].sort.map(&:to_i)
        p Time.at(5) == Time.at(5)
        p Time.at(5).getutc == Time.at(5)
        p Time.at(1.5).hash == Time.at(1.5).hash
        p({ Time.at(99) => "found" }[Time.at(99)])
        p (Time.at(5) <=> Time.at(6))
        p (Time.at(5) <=> 5)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\ntrue\ntrue\n2\n[1, 2, 3]\ntrue\ntrue\ntrue\n\"found\"\n-1\nnil\n"
    );
}

/// The civil constructors disagree about their 7th argument on purpose:
/// `Time.utc`'s is MICROSECONDS, `Time.new`'s is a UTC OFFSET in seconds.
/// Both range-check it. Also: `inspect` renders an offset's seconds where
/// `to_s` doesn't.
#[test]
fn time_civil_constructors_and_their_seventh_argument() {
    let result = run_ruby(
        r#"
        u = Time.utc(2007, 11, 1, 15, 25, 0, 123456)
        p u.usec
        p u.nsec
        puts u.inspect
        p u.utc?

        o = Time.new(2000, 1, 1, 0, 0, 0, 3600)
        p o.utc_offset
        p o.utc?
        puts o.to_s

        # An offset that is not a whole minute: to_s truncates, inspect does not.
        s = Time.new(2000, 1, 1, 0, 0, 0, 123)
        puts s.to_s
        puts s.inspect

        begin
          Time.utc(2000, 1, 1, 0, 0, 0, 1000000)
        rescue ArgumentError => e
          puts "usec: #{e.message}"
        end
        begin
          Time.new(2000, 1, 1, 0, 0, 0, 86400)
        rescue ArgumentError => e
          puts "offset: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "123456\n123456000\n2007-11-01 15:25:00.123456 UTC\ntrue\n3600\nfalse\n2000-01-01 00:00:00 +0100\n2000-01-01 00:00:00 +0002\n2000-01-01 00:00:00 +000203\nusec: subsecx out of range\noffset: utc_offset out of range\n"
    );
}

/// `utc`/`gmtime`/`localtime` convert the receiver IN PLACE and answer self;
/// the `get*` forms answer a copy and leave the receiver alone.
#[test]
fn time_mutating_converters_versus_their_copies() {
    let result = run_ruby(
        r#"
        t = Time.at(1700000000)
        u = t.getutc
        p u.utc?
        p t.utc?          # getutc did NOT mutate t
        t.utc
        p t.utc?          # ...but utc did
        puts t.to_s
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nfalse\ntrue\n2023-11-14 22:13:20 UTC\n"
    );
}

/// `inspect` shows sub-second digits (trailing zeros trimmed); `to_s` never
/// does.
#[test]
fn time_inspect_shows_trimmed_subseconds() {
    let result = run_ruby(
        r#"
        t = Time.at(1700000000.5).getutc
        puts t.inspect
        puts t.to_s
        puts Time.at(1700000000).getutc.inspect
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "2023-11-14 22:13:20.5 UTC\n2023-11-14 22:13:20 UTC\n2023-11-14 22:13:20 UTC\n"
    );
}

/// A Time interpolates via its own `to_s` -- it is an Object with a runtime
/// table, not a registry class, so `call_user_method` has to find the row.
#[test]
fn time_interpolates_through_its_own_to_s() {
    let result = run_ruby(
        r##"
        t = Time.at(1700000000).getutc
        puts "at #{t}"
        puts "#{t.year}-#{t.month}"
        p t.to_s.class
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "at 2023-11-14 22:13:20 UTC\n2023-11\nString\n");
}

/// `ENV` is an Object with Hash-shaped methods, NOT a Hash (`ENV.class` is
/// `Object` -- oracle-verified), and it reads the LIVE environment.
#[test]
fn env_is_an_object_with_hash_shaped_methods() {
    let result = run_ruby(
        r#"
        p ENV.class
        ENV["SPINEL_E2E"] = "set"
        p ENV["SPINEL_E2E"]
        p ENV.key?("SPINEL_E2E")
        p ENV.fetch("SPINEL_E2E")
        p ENV.fetch("SPINEL_E2E_ABSENT", "default")
        p ENV.fetch("SPINEL_E2E_ABSENT") { |k| "computed:#{k}" }
        p ENV["SPINEL_E2E_ABSENT"]
        p ENV.delete("SPINEL_E2E")
        p ENV.key?("SPINEL_E2E")
        p ENV.delete("SPINEL_E2E")
        p ENV.to_h.class
        p ENV.keys.class
        ENV["SPINEL_E2E2"] = "x"
        ENV["SPINEL_E2E2"] = nil
        p ENV["SPINEL_E2E2"]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Object\n\"set\"\ntrue\n\"set\"\n\"default\"\n\"computed:SPINEL_E2E_ABSENT\"\nnil\n\"set\"\nfalse\nnil\nHash\nArray\nnil\n"
    );
}

/// ENV's error shapes: a non-String key is a TypeError, a bare `fetch` miss
/// is a KeyError.
#[test]
fn env_error_shapes() {
    let result = run_ruby(
        r#"
        begin
          ENV[:PATH]
        rescue TypeError => e
          puts "TypeError"
        end
        begin
          ENV.fetch("SPINEL_DEFINITELY_ABSENT")
        rescue KeyError => e
          puts "KeyError"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "TypeError\nKeyError\n");
}

/// `Process` identity and clocks -- asserted as FACTS about the values, never
/// the values themselves (a pid isn't reproducible).
#[test]
fn process_identity_and_clocks() {
    let result = run_ruby(
        r#"
        p Process.pid.is_a?(Integer)
        p Process.pid > 0
        p Process.clock_gettime(Process::CLOCK_MONOTONIC).is_a?(Float)
        p Process.clock_gettime(Process::CLOCK_MONOTONIC, :millisecond).is_a?(Integer)
        p Process.clock_gettime(Process::CLOCK_MONOTONIC, :float_second).is_a?(Float)
        a = Process.clock_gettime(Process::CLOCK_MONOTONIC)
        b = Process.clock_gettime(Process::CLOCK_MONOTONIC)
        p b >= a
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\ntrue\ntrue\ntrue\ntrue\n");
}

/// A missing path raises the right `Errno::*`, with CRuby's message shape --
/// and it is catchable by its PARENT (`SystemCallError`), which is what the
/// prelude hierarchy buys.
#[test]
fn file_errors_are_real_errno_classes() {
    let result = run_ruby(
        r##"
        begin
          File.read("/definitely/not/here")
        rescue Errno::ENOENT => e
          puts "#{e.class}: #{e.message}"
        end
        p Errno::ENOENT.superclass
        p Errno::ENOENT.ancestors.include?(StandardError)
        begin
          Dir.entries("/definitely/not/here")
        rescue SystemCallError => e
          puts e.class
        end
        begin
          File.read(5)
        rescue TypeError => e
          puts "TypeError"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Errno::ENOENT: No such file or directory @ rb_sysopen - /definitely/not/here\nSystemCallError\ntrue\nErrno::ENOENT\nTypeError\n"
    );
}

/// The `File.<predicate>?` family answers false for a missing path rather
/// than raising -- and `size?` is nil for a missing OR empty file, though
/// `size` is 0 for an empty one.
#[test]
fn file_predicates_answer_rather_than_raise() {
    let result = run_ruby(
        r##"
        dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "spinel_e2e_pred_#{Process.pid}")
        Dir.mkdir(dir)
        begin
          full = File.join(dir, "full.txt")
          empty = File.join(dir, "empty.txt")
          missing = File.join(dir, "missing.txt")
          File.write(full, "12345")
          File.write(empty, "")

          p [File.exist?(full), File.exist?(missing)]
          p [File.file?(full), File.file?(dir)]
          p [File.directory?(dir), File.directory?(full)]
          p [File.zero?(empty), File.zero?(full)]
          p File.size(full)
          p File.size(empty)
          p File.size?(full)
          p File.size?(empty)
          p File.size?(missing)
          p [File.exist?(missing), File.file?(missing), File.directory?(missing)]

          File.delete(full, empty)
        ensure
          Dir.rmdir(dir)
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[true, false]\n[true, false]\n[true, false]\n[true, false]\n5\n0\n5\nnil\nnil\n[false, false, false]\n"
    );
}

/// Whole-file read/write/readlines, including `chomp: true`.
#[test]
fn file_whole_file_io() {
    let result = run_ruby(
        r##"
        dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "spinel_e2e_rw_#{Process.pid}")
        Dir.mkdir(dir)
        begin
          path = File.join(dir, "lines.txt")
          n = File.write(path, "alpha\nbeta\ngamma\n")
          p n
          p File.read(path)
          p File.readlines(path)
          p File.readlines(path, chomp: true)
          File.write(path, "no trailing newline")
          p File.readlines(path)
          File.delete(path)
        ensure
          Dir.rmdir(dir)
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "17\n\"alpha\\nbeta\\ngamma\\n\"\n[\"alpha\\n\", \"beta\\n\", \"gamma\\n\"]\n[\"alpha\", \"beta\", \"gamma\"]\n[\"no trailing newline\"]\n"
    );
}

/// `File.open`: the block form closes on every exit path and answers the
/// block's value. A LENGTHED read at EOF is nil where a whole-rest read is
/// `""` -- the asymmetry a `while chunk = f.read(n)` loop relies on.
#[test]
fn file_open_block_form_and_positioned_reads() {
    let result = run_ruby(
        r##"
        dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "spinel_e2e_open_#{Process.pid}")
        Dir.mkdir(dir)
        begin
          path = File.join(dir, "counted.txt")
          File.open(path, "w") { |f| f.print "ABCDEFGHIJ" }
          p File.read(path)

          File.open(path, "r") do |f|
            p f.read(5)
            p f.read(5)
            p f.read(5)
            f.rewind
            p f.read(2)
            p f.tell
            f.seek(0)
            p f.read
            p f.eof?
          end

          p File.open(path, "r") { |f| f.read(3) }

          h = File.open(path, "r")
          p h.closed?
          h.close
          p h.closed?
          begin
            h.read
          rescue IOError => e
            puts "IOError: #{e.message}"
          end

          File.delete(path)
        ensure
          Dir.rmdir(dir)
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"ABCDEFGHIJ\"\n\"ABCDE\"\n\"FGHIJ\"\nnil\n\"AB\"\n2\n\"ABCDEFGHIJ\"\ntrue\n\"ABC\"\nfalse\ntrue\nIOError: closed stream\n"
    );
}

/// `File.open`'s block form closes the file even when the block RAISES --
/// that ensure is the whole reason the idiom exists.
#[test]
fn file_open_closes_even_when_the_block_raises() {
    let result = run_ruby(
        r##"
        dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "spinel_e2e_raise_#{Process.pid}")
        Dir.mkdir(dir)
        begin
          path = File.join(dir, "x.txt")
          File.write(path, "data")
          handle = nil
          begin
            File.open(path, "r") do |f|
              handle = f
              raise "boom"
            end
          rescue RuntimeError => e
            puts "rescued: #{e.message}"
          end
          p handle.closed?
          File.delete(path)
        ensure
          Dir.rmdir(dir)
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "rescued: boom\ntrue\n");
}

/// `Dir` listing: `entries` includes `.`/`..`, `children` does not.
#[test]
fn dir_listing_distinguishes_entries_from_children() {
    let result = run_ruby(
        r##"
        dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "spinel_e2e_list_#{Process.pid}")
        Dir.mkdir(dir)
        begin
          File.write(File.join(dir, "a.rb"), "")
          File.write(File.join(dir, "b.txt"), "")
          Dir.mkdir(File.join(dir, "sub"))

          p Dir.children(dir).sort
          p Dir.entries(dir).sort
          p Dir.exist?(dir)
          p Dir.exist?(File.join(dir, "a.rb"))
          p Dir.exist?("/definitely/not/here")
          p Dir.empty?(File.join(dir, "sub"))
          p Dir.pwd.start_with?("/")

          Dir.rmdir(File.join(dir, "sub"))
          File.delete(File.join(dir, "a.rb"), File.join(dir, "b.txt"))
        ensure
          Dir.rmdir(dir)
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[\"a.rb\", \"b.txt\", \"sub\"]\n[\".\", \"..\", \"a.rb\", \"b.txt\", \"sub\"]\ntrue\nfalse\nfalse\ntrue\ntrue\n"
    );
}

/// `Dir.glob`: `*` within a segment, `**` across them, `?`, `{a,b}`
/// alternation, and Ruby's hidden-file rule (a leading `.` is invisible to a
/// wildcard).
#[test]
fn dir_glob_semantics() {
    let result = run_ruby(
        r##"
        dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "spinel_e2e_glob_#{Process.pid}")
        Dir.mkdir(dir)
        begin
          Dir.chdir(dir) do
            Dir.mkdir("sub")
            File.write("x.rb", "")
            File.write("y.txt", "")
            File.write(".hidden", "")
            File.write("sub/z.rb", "")

            p Dir.glob("*.rb").sort
            p Dir.glob("**/*.rb").sort
            p Dir.glob("sub/*.rb")
            p Dir.glob("*.{rb,txt}").sort
            p Dir.glob("?.rb")
            p Dir["*.rb"]
            p Dir.glob("*").sort
            p Dir.glob("*").include?(".hidden")
            p Dir.glob(".*").include?(".hidden")

            File.delete("x.rb", "y.txt", ".hidden", "sub/z.rb")
            Dir.rmdir("sub")
          end
        ensure
          Dir.rmdir(dir)
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[\"x.rb\"]\n[\"sub/z.rb\", \"x.rb\"]\n[\"sub/z.rb\"]\n[\"x.rb\", \"y.txt\"]\n[\"x.rb\"]\n[\"x.rb\"]\n[\"sub\", \"x.rb\", \"y.txt\"]\nfalse\ntrue\n"
    );
}

/// `Dir.chdir`'s block form restores the previous directory afterwards.
#[test]
fn dir_chdir_block_form_restores_the_previous_directory() {
    let result = run_ruby(
        r##"
        dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "spinel_e2e_chdir_#{Process.pid}")
        Dir.mkdir(dir)
        begin
          before = Dir.pwd
          Dir.chdir(dir) do
            p Dir.pwd != before
          end
          p Dir.pwd == before
        ensure
          Dir.rmdir(dir)
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\n");
}

/// `Dir.mkdir`/`rmdir` round-trip, and `File.rename`/`delete`.
#[test]
fn dir_and_file_mutation_round_trips() {
    let result = run_ruby(
        r##"
        dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "spinel_e2e_mut_#{Process.pid}")
        Dir.mkdir(dir)
        begin
          fresh = File.join(dir, "fresh")
          Dir.mkdir(fresh)
          p Dir.exist?(fresh)
          Dir.rmdir(fresh)
          p Dir.exist?(fresh)

          a = File.join(dir, "a.txt")
          b = File.join(dir, "b.txt")
          File.write(a, "x")
          File.rename(a, b)
          p [File.exist?(a), File.exist?(b)]
          p File.delete(b)
          p File.exist?(b)
        ensure
          Dir.rmdir(dir)
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nfalse\n[false, true]\n1\nfalse\n"
    );
}

#[test]
fn array_optional_and_variadic_arities() {
    // The builtin tables used to hardcode a single arity and reject the
    // optional-`n` and variadic forms Ruby accepts: `last(n)`, `sample(n)`,
    // the block form of `rindex`, the fill span forms, and the variadic
    // set-op siblings `union`/`intersection`/`difference` (distinct from the
    // binary `|`/`&`/`-`). All oracle-verified against ruby 4.0.5.
    let result = run_ruby(
        r#"
        p [1, 2, 3].last(2)
        p [1, 2, 3].last(0)
        p [1, 2, 3].last(5)
        p [1, 2, 3, 2].rindex { |x| x < 3 }
        p [1, 2, 3, 2].rindex(2)
        p [1, 2, 3].union
        p [1, 2, 3].union([2, 3], [4])
        p [1, 2, 3, 4].intersection([2, 3, 4], [3, 4, 5])
        p [1, 2, 3].difference([2], [4])
        p [1, 1, 2].difference([2])
        a = [0, 0, 0]; a.fill(9); p a
        a = [0, 0, 0]; a.fill(9, 1, 1); p a
        a = [1, 2, 3]; a.fill(9, 1, 5); p a
        a = [1, 2, 3]; a.fill { |i| i }; p a
        a = [1, 2, 3, 4, 5]; a.fill(-2) { |i| i * 10 }; p a
        p [1, 2, 3].sample(2).length
        p [1, 2, 3].sample(5).sort
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[2, 3]\n[]\n[1, 2, 3]\n3\n3\n\
         [1, 2, 3]\n[1, 2, 3, 4]\n[3, 4]\n[1, 3]\n[1, 1]\n\
         [9, 9, 9]\n[0, 9, 0]\n[1, 9, 9, 9, 9, 9]\n[0, 1, 2]\n[1, 2, 3, 30, 40]\n\
         2\n[1, 2, 3]\n"
    );
}

#[test]
fn string_optional_arg_arities() {
    // `count`/`delete` take one OR MORE char-set specs (intersected, `^`
    // negation honored); `match`/`match?`/`rindex` take an optional start
    // position; `rindex` also accepts a Regexp; `each_line` an optional
    // separator; `split` an optional limit (positive caps fields, negative
    // keeps trailing empties). All oracle-verified against ruby 4.0.5.
    let result = run_ruby(
        r#"
        p "hello world".count("lo")
        p "hello world".count("lo", "o")
        p "hello world".count("^l", "lo")
        p "hello".rindex("l", 2)
        p "hello".rindex("l", 3)
        p "abcdabcd".rindex(/c/)
        p "hello".rindex(/l/, 2)
        p "hello".match?(/e/, 1)
        p "hello".match?(/o/, -1)
        r = "hello".match(/l/, 3); p(r && r[0])
        p "1-2-3".each_line("-").to_a
        p "a,b,c".split(",", 2)
        p "a,b,,".split(",")
        p "a,b,,".split(",", -1)
        p "a1b2c3".split(/\d/, 2)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "5\n2\n2\n2\n3\n6\n2\ntrue\ntrue\n\"l\"\n\
         [\"1-\", \"2-\", \"3\"]\n\
         [\"a\", \"b,c\"]\n[\"a\", \"b\"]\n[\"a\", \"b\", \"\", \"\"]\n\
         [\"a\", \"b2c3\"]\n"
    );
}

#[test]
fn class_variables_at_module_and_top_level_scope() {
    // `@@x` written in a module body, in a `def self.` body, and bare at
    // the top level (whose storage lives on Object) all read back.
    let result = run_ruby(
        r#"
        module Conf
          @@secret = ""
          def self.secret; @@secret; end
          def self.secret=(v); @@secret = v; end
        end
        puts Conf.secret.length
        Conf.secret = "hi"
        puts Conf.secret
        @@plain = 42
        puts @@plain
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "0\nhi\n42\n");
}

#[test]
fn top_level_class_variable_is_stored_on_object() {
    // A bare `@@x` written outside any class/module body resolves its storage
    // to Object, so a later top-level read sees the same value. NOTE a
    // divergence: Ruby 4.0.5 itself now RAISES `RuntimeError: class variable
    // access from toplevel` for both the write and the read here (it was a
    // warning in older rubies). Spinel keeps the older permissive behavior to
    // match the committed `test/module_cvars.rb` snapshot the conformance
    // suite scores against; this test pins that intentional choice.
    let result = run_ruby(
        r#"
        @@plain = 42
        puts @@plain
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn regexp_ruby_escape_e_lowers_to_x1b() {
    // Ruby's `\e` (ESC) isn't a Rust `regex`-crate escape; it must be
    // translated to `\x1b`, and `#source` still shows the original `\e`.
    let result = run_ruby(
        r#"
        re = /\e\[[0-9;]*m/
        puts "\e[31mRED\e[0m".gsub(re, "")
        puts re.source
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "RED\n\\e\\[[0-9;]*m\n");
}

#[test]
fn module_function_promotes_bareword_and_symbol_forms() {
    // A bare `module_function` promotes every following `def` to a module
    // method; `module_function :name` promotes an already-defined one.
    let result = run_ruby(
        r#"
        module M
          module_function
          def shout(s) = s.upcase
        end
        module N
          def whisper(s) = s.downcase
          module_function :whisper
        end
        puts M.shout("hi")
        puts N.whisper("HI")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "HI\nhi\n");
}

#[test]
fn splat_and_double_splat_at_a_new_call_site() {
    // A `*args` positional splat and a `**h` double-splat at `.new`
    // dispatch through the runtime constructor rather than being rejected.
    let result = run_ruby(
        r#"
        class Point
          def initialize(x, y); @x = x; @y = y; end
          def to_s; "(#{@x}, #{@y})"; end
        end
        Pair = Data.define(:a, :b)
        args = [1, 2]
        h = { a: 3, b: 4 }
        puts Point.new(*args)
        p Pair.new(**h)
        p Pair.new(**{ a: 5, b: 6 })
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "(1, 2)\n#<data Pair a=3, b=4>\n#<data Pair a=5, b=6>\n"
    );
}

#[test]
fn alias_under_a_static_modifier_and_if_elsif() {
    // A statically-literal `if`/`unless` guard on an `alias` is folded at
    // definition time: the selected branch's alias is registered.
    let result = run_ruby(
        r#"
        class C
          def one = 1
          def two = 2
          alias uno one if true
          alias dos two unless false
          alias never one if false
          if false
            alias chosen one
          elsif true
            alias chosen two
          end
        end
        puts C.new.uno
        puts C.new.dos
        puts C.new.chosen
        puts C.new.respond_to?(:never)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n2\n2\nfalse\n");
}

#[test]
fn exception_backtrace_full_message_and_inspect() {
    // backtrace is an empty array (spinel does not track per-exception
    // backtraces); inspect renders "#<Class: msg>" (or the bare class name
    // when the message is empty); full_message is "Class: msg".
    let result = run_ruby(
        r#"
        begin
          raise ArgumentError, "bad"
        rescue => e
          p e.backtrace
          p e.inspect
          puts e.full_message
        end
        p StandardError.new("").inspect
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[]\n\"#<ArgumentError: bad>\"\nArgumentError: bad\n\"StandardError\"\n"
    );
}

#[test]
fn dollar_bang_reads_the_exception_being_handled() {
    // `$!` is the current exception inside a rescue (block or modifier form),
    // nil outside one.
    let result = run_ruby(
        r#"
        p $!
        r = (Integer("x") rescue $!.class)
        p r
        begin
          raise "boom"
        rescue
          puts $!.message
        end
        p $!
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "nil\nArgumentError\nboom\nnil\n");
}

#[test]
fn module_const_get_and_const_defined_reflect_the_registry() {
    let result = run_ruby(
        r#"
        module M
          X = 7
          module N
          end
        end
        puts M.const_get(:X)
        puts M.const_get("X")
        p M.const_defined?(:X)
        p M.const_defined?(:Nope)
        p M.const_defined?(:N)
        p M.const_get(:N).is_a?(Module)
        begin; M.const_get(:Missing); rescue NameError => e; puts e.message; end
        begin; M.const_defined?("bad"); rescue NameError => e; puts e.message; end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "7\n7\ntrue\nfalse\ntrue\ntrue\n\
         uninitialized constant M::Missing\nwrong constant name bad\n"
    );
}

#[test]
fn class_method_defined_and_class_variable_reflection() {
    let result = run_ruby(
        r#"
        class Base
          @@shared = 1
          def inherited_m; end
        end
        class Sub < Base
          def own_m; end
        end
        p Sub.method_defined?(:own_m)
        p Sub.method_defined?(:inherited_m)
        p Sub.method_defined?(:frozen?)
        p Sub.method_defined?(:nope)
        p Base.class_variable_defined?(:@@shared)
        p Base.class_variable_get(:@@shared)
        Base.class_variable_set(:@@shared, 42)
        p Base.class_variable_get(:@@shared)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\ntrue\nfalse\ntrue\n1\n42\n");
}

#[test]
fn instance_variable_reflection_on_objects() {
    // get/set/list over a concrete receiver, a poly receiver, missing names,
    // an empty object, and a malformed-name NameError.
    let result = run_ruby(
        r#"
        class Box
          def initialize(v); @v = v; @tag = "b"; end
        end
        b = Box.new(7)
        p b.instance_variables
        p b.instance_variable_get(:@v)
        b.instance_variable_set(:@v, 70)
        p b.instance_variable_get("@v")
        p b.instance_variable_get(:@missing)
        class Bare; end
        p Bare.new.instance_variables
        def peek(o) = o.instance_variable_get(:@v)
        p peek(Box.new(99))
        begin
          b.instance_variable_get(:v)
        rescue NameError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[:@v, :@tag]\n7\n70\nnil\n[]\n99\n\
         'v' is not allowed as an instance variable name\n"
    );
}

#[test]
fn define_singleton_method_forms() {
    // In-body (bare and self), external constant receiver (with a param),
    // and a namespaced receiver all define a class method.
    let result = run_ruby(
        r#"
        class C
          define_singleton_method(:a) { "a" }
          self.define_singleton_method(:b) { "b" }
        end
        C.define_singleton_method(:c) { |n| n + 1 }
        module M
          class D; end
        end
        M::D.define_singleton_method(:d) { "nested" }
        puts C.a
        puts C.b
        puts C.c(41)
        puts M::D.d
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "a\nb\n42\nnested\n");
}

#[test]
fn alias_and_alias_method_including_inherited_sources() {
    // Same-class and inherited sources, both spellings, chained across levels.
    let result = run_ruby(
        r#"
        class Base
          def greet(who); "hi " + who; end
          alias hail greet
          alias_method :salute, :greet
        end
        class Mid < Base
          alias_method :welcome, :greet
          alias hey greet
        end
        class Leaf < Mid
          alias again welcome
        end
        puts Base.new.hail("a")
        puts Base.new.salute("b")
        puts Mid.new.welcome("c")
        puts Mid.new.hey("d")
        puts Leaf.new.again("e")
        p Leaf.method_defined?(:again)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "hi a\nhi b\nhi c\nhi d\nhi e\ntrue\n"
    );
}

#[test]
fn string_new_builds_empty_and_copied_buffers_with_encoding() {
    let result = run_ruby(
        r#"
        p String.new
        p String.new("hi")
        p String.new.encoding.name
        p String.new("x", encoding: "ASCII-8BIT").encoding.name
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"\"\n\"hi\"\n\"ASCII-8BIT\"\n\"ASCII-8BIT\"\n");
}

#[test]
fn hash_new_default_value_and_default_block() {
    let result = run_ruby(
        r#"
        h = Hash.new(0)
        h[:a] += 1
        h[:a] += 1
        p h[:a]
        p h[:z]
        p h.size
        d = Hash.new { |hh, k| hh[k] = k.to_s }
        p d[:q]
        p d
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n0\n1\n\"q\"\n{q: \"q\"}\n");
}

#[test]
fn regexp_new_from_string_flags_and_copy() {
    let result = run_ruby(
        r#"
        p(Regexp.new("a.c") =~ "xabc")
        p Regexp.new("hi", Regexp::IGNORECASE).match?("HI")
        p Regexp.new(/z/i).match?("Z")
        p Regexp.new("a.c").source
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\ntrue\ntrue\n\"a.c\"\n");
}

#[test]
fn set_construction_membership_operators_and_enumerable() {
    let result = run_ruby(
        r#"
        s = Set[3, 1, 2, 1]
        p s.size
        p s.include?(2)
        p (Set[1, 2] | Set[2, 3]).to_a.sort
        p Set[1, 2].subset?(Set[1, 2, 3])
        p s.map { |x| x * 2 }.sort
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\ntrue\n[1, 2, 3]\ntrue\n[2, 4, 6]\n");
}

#[test]
fn enumerable_chunk_groups_consecutive_runs() {
    let result = run_ruby(
        r#"
        p [1, 1, 2, 2, 2, 3].chunk { |x| x }.map { |k, v| [k, v.size] }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[[1, 2], [2, 3], [3, 1]]\n");
}

#[test]
fn lazy_enumerator_over_infinite_and_finite_sources() {
    let result = run_ruby(
        r#"
        p (1..Float::INFINITY).lazy.select(&:even?).map { |x| x * x }.first(3)
        p [1, 2, 3, 4].lazy.map { |x| x + 1 }.to_a
        p [1, 2, 3].lazy.class.name
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[4, 16, 36]\n[2, 3, 4, 5]\n\"Enumerator::Lazy\"\n");
}

#[test]
fn numeric_coerce_protocol_for_user_types() {
    let result = run_ruby(
        r#"
        class Money
          attr_reader :cents
          def initialize(c); @cents = c; end
          def coerce(o); [Money.new(o * 100), self]; end
          def +(o); Money.new(@cents + o.cents); end
          def cents_s; @cents.to_s; end
        end
        puts((5 + Money.new(250)).cents_s)
        puts((2 + 3))
        begin; 1 + "x"; rescue TypeError => e; puts e.message; end
        class Bad; def coerce(o); 42; end; end
        begin; 1 + Bad.new; rescue TypeError; puts "bad"; end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "750\n5\nString can't be coerced into Integer\nbad\n"
    );
}

#[test]
fn format_directives_named_positional_star_and_alternate_form() {
    let result = run_ruby(
        r#"
        puts format("%<name>s is %<age>d", name: "Ada", age: 36)
        puts format("%#b / %#x", 10, 255)
        puts format("%*d|", 5, 42)
        puts format("%2$s %1$s", "world", "hello")
        puts format("%a", 0.5)
        puts("%<x>05.2f" % { x: 3.14159 })
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Ada is 36\n0b1010 / 0xff\n   42|\nhello world\n0x1p-1\n03.14\n"
    );
}

#[test]
fn array_and_hash_to_s_use_inspect_form_only_puts_flattens() {
    let result = run_ruby(
        r##"
        p [1, 2].to_s
        print [1, 2]; puts
        puts "i #{[1, 2]}"
        puts [1, 2]
        p({ a: 1 }.to_s)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"[1, 2]\"\n[1, 2]\ni [1, 2]\n1\n2\n\"{a: 1}\"\n"
    );
}

#[test]
fn frozen_string_dedup_intern_and_mutation_guard() {
    let result = run_ruby(
        r#"
        p((-"hello").equal?(-"hello"))
        p("a".dedup.equal?("a".dedup))
        p(("he" + "llo").dedup.equal?("hello".dedup))
        a = "abc".freeze
        p((-a).equal?(a))
        p((-"x").equal?(-"y"))
        p((-"frozen").frozen?)
        nul = "a b"
        p nul.bytesize
        p("a b".dedup.equal?("a b".dedup))
        begin
          (-"immutable") << "!"
        rescue FrozenError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\ntrue\ntrue\ntrue\nfalse\ntrue\n3\ntrue\n\
         can't modify frozen String: \"immutable\"\n"
    );
}

#[test]
fn frozen_string_literal_pragma_freezes_literals() {
    let result = run_ruby(
        "# frozen_string_literal: true\n\
         p \"abc\".frozen?\n\
         p \"interp #{1 + 1}\".frozen?\n\
         p \"abc\".dup.frozen?\n\
         buf = \"abc\"\n\
         begin; buf << \"y\"; rescue FrozenError => e; puts e.message; end\n",
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nfalse\nfalse\ncan't modify frozen String: \"abc\"\n"
    );
}

#[test]
fn for_loops_over_hashes_and_parenthesized_collections() {
    let result = run_ruby(
        r##"
        total = 0
        for k, v in { "a" => 1, "b" => 2, "c" => 3 }
          total += v
          puts "#{k}:#{v}"
        end
        p total
        for pair in { x: 10, y: 20 }
          p pair
        end
        for k, v in {}
          puts "unreachable"
        end
        sum = 0
        for i in (1..3)
          sum += i
        end
        p sum
        for e in ([9, 8])
          p e
        end
        for last in [7, 8, 9]
        end
        p last
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "a:1\nb:2\nc:3\n6\n[:x, 10]\n[:y, 20]\n6\n9\n8\n9\n"
    );
}

#[test]
fn block_and_proc_argument_semantics() {
    let result = run_ruby(
        r##"
        h = { a: 1, b: 2 }
        h.each { |pair| p pair }
        h.each { |k, v| puts "#{k}=#{v}" }
        def show(pair) = p pair
        h.each(&method(:show))

        add = proc { |a:, b:| a + b }
        p add.call(a: 1, b: 2)
        opt = proc { |x:, y: 100| [x, y] }
        p opt.call(x: 5)
        begin
          add.call(a: 1)
        rescue ArgumentError => e
          puts e.message
        end

        lam = ->(x:, y: 9) { [x, y] }
        p lam.call(x: 5)

        pr = proc { |a, b| [a, b] }
        p pr.call(1, 2, 3)
        p pr.call(1)
        p [[1, 2], [3, 4]].map { |a, b| a + b }
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[:a, 1]\n[:b, 2]\na=1\nb=2\n[:a, 1]\n[:b, 2]\n3\n[5, 100]\n\
         missing keyword: :b\n[5, 9]\n[1, 2]\n[1, nil]\n[3, 7]\n"
    );
}

#[test]
fn comparison_protocol_validates_spaceship_clamp_and_sort() {
    let result = run_ruby(
        r#"
        class Temp
          include Comparable
          attr_reader :deg
          def initialize(d); @deg = d; end
          def <=>(o); (deg - o.deg).to_f; end
        end
        p [Temp.new(3), Temp.new(1), Temp.new(2)].sort.map(&:deg)
        p Temp.new(5) < Temp.new(9)
        p Temp.new(5).clamp(Temp.new(1), Temp.new(9)).deg
        p Temp.new(5) == Temp.new(5)

        class Ver
          include Comparable
          def initialize(n); @n = n; end
          attr_reader :n
          def <=>(o); o.is_a?(Ver) ? (n <=> o.n) : nil; end
        end
        a = Ver.new(1)
        p a == Ver.new(2)
        p a == a
        p(begin; a < "x"; rescue => e; e.class; end)

        p 5.clamp(1, nil)
        p 5.clamp(nil, 3)
        p(begin; 5.clamp(10, 1); rescue => e; e.message; end)
        p(begin; 5.clamp(1...10); rescue => e; e.message; end)

        x = "a"
        p(begin; [1, x, 2].min; rescue => e; e.message; end)
        p(begin; [1, x, 2].max; rescue => e; e.class; end)
        p({ b: 2, a: 1, c: 3 }.sort)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2, 3]\ntrue\n5\ntrue\nfalse\ntrue\nArgumentError\n5\n3\n\
         \"min argument must be less than or equal to max argument\"\n\
         \"cannot clamp with an exclusive range\"\n\
         \"comparison of String with 1 failed\"\nArgumentError\n\
         [[:a, 1], [:b, 2], [:c, 3]]\n"
    );
}

#[test]
fn string_and_hash_leaf_methods() {
    let result = run_ruby(
        r#"
        puts "hello world".gsub(/[aeiou]/, "a" => "1", "e" => "2", "o" => "3")
        p "hello".split("")
        p "hello world"[/(\w+) (\w+)/, 2]
        p "Hello".casecmp?("HELLO")
        p "0x1f".oct
        s = "hello"; s.slice!(1, 2); p s
        p({ b: 2, a: 1 }.sort)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "h2ll3 w3rld\n[\"h\", \"e\", \"l\", \"l\", \"o\"]\n\"world\"\ntrue\n31\n\"hlo\"\n[[:a, 1], [:b, 2]]\n"
    );
}

#[test]
fn integer_bit_reference_operator() {
    let result = run_ruby(
        r#"
        p 0b1011[0]
        p 0b1011[2]
        p 5[0, 2]
        p 0b1101[1, 3]
        p 255[0..3]
        p 255[4..]
        p (-2)[0]
        p (-2)[1]
        p 10[100]
        p((1 << 100)[100])
        begin
          255[..3]
        rescue ArgumentError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "1\n0\n1\n6\n15\n15\n0\n1\n0\n1\nThe beginless range for Integer#[] results in infinity\n"
    );
}

#[test]
fn integer_bit_predicates_and_string_bytesplice() {
    let result = run_ruby(
        r#"
        p 0b1010.allbits?(0b0010)
        p 0b1010.allbits?(0b0110)
        p 0b1010.anybits?(0b0110)
        p 0b1010.nobits?(0b0101)
        s = "hello"; s.bytesplice(0, 2, "XY"); p s
        t = "hello"; t.bytesplice(1..2, "__"); p t
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nfalse\ntrue\ntrue\n\"XYllo\"\n\"h__lo\"\n"
    );
}

#[test]
fn float_adjacent_representable_values() {
    let result = run_ruby(
        r#"
        p 1.0.next_float > 1.0
        p 1.0.prev_float < 1.0
        p 1.0.next_float.prev_float == 1.0
        p 3.14.next_float.class
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\ntrue\nFloat\n");
}

#[test]
fn string_bang_mutators_and_sum_chr() {
    let result = run_ruby(
        r##"
        s = "abc"; p s.upcase!; p s; p s.upcase!
        t = "  hi  "; t.strip!; p t
        u = "hello"; u.reverse!; p u
        v = "a b c"; v.gsub!(" ", "-"); p v
        w = "az"; w.succ!; p w
        p "hello".sum
        p "hello".chr
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"ABC\"\n\"ABC\"\nnil\n\"hi\"\n\"olleh\"\n\"a-b-c\"\n\"ba\"\n532\n\"h\"\n"
    );
}

#[test]
fn numeric_coerce_div_and_predicates() {
    let result = run_ruby(
        r#"
        p 7.coerce(2)
        p 7.coerce(2.0)
        p 7.ceildiv(2)
        p((-7).ceildiv(2))
        p 5.i
        p 10.finite?
        p 10.infinite?
        p 2.0.coerce(3)
        p 7.0.div(2)
        p 5.0.i
        p 1.0.next_float > 1.0
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[2, 7]\n[2.0, 7.0]\n4\n-3\n(0+5i)\ntrue\nnil\n[3.0, 2.0]\n3\n(0+5.0i)\ntrue\n"
    );
}

#[test]
fn hash_projection_and_inplace_methods() {
    let result = run_ruby(
        r#"
        h = { a: 1, b: 2, c: 3 }
        p h.values_at(:a, :c)
        p h.assoc(:b)
        p h.rassoc(2)
        p({ a: 1, b: nil, c: 3 }.compact)
        y = { a: 1, b: 2, c: 3 }; p y.shift; p y
        p({ a: 1, b: 2 }.select! { |_k, v| v > 1 })
        z = { a: 1, b: 2 }; z.transform_values! { |v| v * 10 }; p z
        p({ a: 1, b: 2 } <= { a: 1, b: 2, c: 3 })
        p({ a: 1, b: 2, c: 3 } > { a: 1 })
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 3]\n[:b, 2]\n[:b, 2]\n{a: 1, c: 3}\n[:a, 1]\n{b: 2, c: 3}\n{b: 2}\n{a: 10, b: 20}\ntrue\ntrue\n"
    );
}

#[test]
fn array_inplace_and_universal_object_methods() {
    let result = run_ruby(
        r#"
        p [1, nil, 2, nil].compact!
        p [1, 2].compact!
        a = [1, 2, 3, 4]; a.rotate!(2); p a
        case [1, 2]; in [x, y]; p [x, y]; end
        p :hello.start_with?("he")
        p :hello.end_with?("lo")
        p("abc" !~ /z/)
        p "hi".display
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2]\nnil\n[3, 4, 1, 2]\n[1, 2]\ntrue\ntrue\ntrue\nhinil\n"
    );
}

#[test]
fn matchdata_values_at_and_string_to_r() {
    let result = run_ruby(
        r#"
        m = "2024-01-15".match(/(\d+)-(\d+)-(\d+)/)
        p m.values_at(1, 3)
        p m.values_at(0, 2)
        p "123".to_r
        p "3/4".to_r
        p "1.5".to_r
        p "abc".to_r
        p "  -12".to_r
        p "1_000".to_r
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[\"2024\", \"15\"]\n[\"2024-01-15\", \"01\"]\n(123/1)\n(3/4)\n(3/2)\n(0/1)\n(-12/1)\n(1000/1)\n"
    );
}

#[test]
fn string_array_hash_symbol_proc_leaf_methods() {
    let result = run_ruby(
        r##"
        p "hello".codepoints
        s = "hi\n"; p s.chomp!; p s.chomp!
        p "hello".partition("l")
        p "hello".rpartition("l")
        p "a.b.c".partition(/\./)
        p "TestName".delete_prefix("Test")
        p "file.rb".delete_suffix(".rb")
        c = +"x"; c.clear; p c
        p((+"y").frozen?)
        p [1, 2].repeated_permutation(2).to_a
        p [1, 2].repeated_combination(2).to_a
        p [1, 2, 3].intersect?([3, 4])
        p [1, 2].chain([3], [4]).to_a
        p({ a: 1, b: 2, c: 3 }.slice(:a, :c))
        p({ a: 1, b: 2 }.except(:a))
        p({ a: 1, b: 2 }.fetch_values(:a, :b))
        p({ a: 1, b: [2, 3] }.flatten)
        p :hello[1, 3]
        p :Hello.casecmp?(:hELLO)
        f = ->(x) { x + 1 }
        g = ->(x) { x * 2 }
        p((f >> g).call(3))
        p((f << g).call(3))
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[104, 101, 108, 108, 111]\n\"hi\"\nnil\n[\"he\", \"l\", \"lo\"]\n[\"hel\", \"l\", \"o\"]\n[\"a\", \".\", \"b.c\"]\n\"Name\"\n\"file\"\n\"\"\nfalse\n[[1, 1], [1, 2], [2, 1], [2, 2]]\n[[1, 1], [1, 2], [2, 2]]\ntrue\n[1, 2, 3, 4]\n{a: 1, c: 3}\n{b: 2}\n[1, 2]\n[:a, 1, :b, [2, 3]]\n\"ell\"\ntrue\n8\n7\n"
    );
}

#[test]
fn regexp_complex_range_leaf_methods() {
    let result = run_ruby(
        r#"
        p Regexp.escape("a.b*c")
        p Regexp.quote("1+1")
        p(/abc/i.options)
        p(/abc/m.options)
        p(/abc/.options)
        p Complex.rect(3, 4)
        p Complex.rectangular(3)
        p((1..10).bsearch { |x| x >= 4 })
        p((1..100).bsearch { |x| x >= 40 })
        p((1..10).bsearch { |x| x >= 40 })
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"a\\\\.b\\\\*c\"\n\"1\\\\+1\"\n1\n4\n0\n(3+4i)\n(3+0i)\n4\n40\nnil\n"
    );
}

#[test]
fn rational_complex_numeric_leaf_methods() {
    let result = run_ruby(
        r#"
        r = Rational(3, 2)
        p r.finite?
        p r.infinite?
        p r.coerce(2)
        p r.coerce(2.0)
        p r.div(1)
        p Rational(7, 2).div(2)
        p r.i
        c = Complex(6, 0)
        p c.finite?
        p c.infinite?
        p c.to_f
        p c.to_i
        p c.to_r
        p c.coerce(3)
        p Complex(3, 4).finite?
        p Complex(3, 4).numerator
        p Complex(3, 4).denominator
        p Complex(Rational(2, 3), Rational(3, 4)).numerator
        p Complex(Rational(2, 3), Rational(3, 4)).denominator
        begin
          Complex(3, 4).to_f
        rescue RangeError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nnil\n[(2/1), (3/2)]\n[2.0, 1.5]\n1\n1\n(0+(3/2)*i)\ntrue\nnil\n6.0\n6\n(6/1)\n[(3+0i), (6+0i)]\ntrue\n(3+4i)\n1\n(8+9i)\n12\ncan't convert 3+4i into Float\n"
    );
}

#[test]
fn numeric_rect_polar_and_collection_leaf_methods() {
    let result = run_ruby(
        r#"
        p 5.rect
        p 5.polar
        p((-5).polar)
        p 2.5.polar
        p 5.rationalize
        p Rational(3, 2).polar
        p(nil =~ /x/)
        p [1, 2, 3, 4].rfind { |x| x.even? }
        p [10, 20, 30].fetch_values(0, 2)
        p([10, 20, 30].fetch_values(0, 5) { |i| i * 100 })
        p({ a: 1, b: 2 }.to_proc.call(:b))
        p({ a: 1, b: 2 }.transform_keys!(&:to_s))
        p((1..5).overlap?(5..8))
        p((1...5).overlap?(5..8))
        p((1..5).overlap?(6..8))
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[5, 0]\n[5, 0]\n[5, 3.141592653589793]\n[2.5, 0]\n(5/1)\n[(3/2), 0]\nnil\n4\n[10, 30]\n[10, 500]\n2\n{\"a\" => 1, \"b\" => 2}\ntrue\nfalse\nfalse\n"
    );
}

#[test]
fn string_upto_and_byte_indexing_methods() {
    let result = run_ruby(
        r#"
        p "a8".upto("b1").to_a
        r = []
        "a".upto("e") { |x| r << x }
        p r
        p "hello".byteindex("l")
        p "hello".byteindex("l", 3)
        p "hello".byterindex("l")
        p "hello".byterindex("l", 2)
        p "hello".byteslice(1, 3)
        p "café".byteslice(0, 3)
        p "hello".byteslice(-2, 2)
        p "hello".byteslice(10)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[\"a8\", \"a9\", \"b0\", \"b1\"]\n[\"a\", \"b\", \"c\", \"d\", \"e\"]\n2\n3\n3\n2\n\"ell\"\n\"caf\"\n\"lo\"\nnil\n"
    );
}

#[test]
fn enumerable_zip_compact_cycle_chain_over_range_and_hash() {
    let result = run_ruby(
        r#"
        p (1..3).zip([4, 5, 6], [7, 8, 9])
        p (1..5).compact
        p (1..3).chain([4, 5]).to_a
        seen = []
        (1..3).cycle(2) { |x| seen << x }
        p seen
        p({ a: 1, b: 2 }.zip([10, 20]))
        p({ a: 1 }.rehash)
        p "hello".tr_s("l", "r")
        p "aabbcc".tr_s("a-c", "x")
        s = "hello"
        p s.tr_s!("l", "r")
        p s
        p "clean".scrub!
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[[1, 4, 7], [2, 5, 8], [3, 6, 9]]\n[1, 2, 3, 4, 5]\n[1, 2, 3, 4, 5]\n[1, 2, 3, 1, 2, 3]\n[[[:a, 1], 10], [[:b, 2], 20]]\n{a: 1}\n\"hero\"\n\"x\"\n\"hero\"\n\"hero\"\n\"clean\"\n"
    );
}

#[test]
fn numeric_tower_exactness() {
    let result = run_ruby(
        r#"
        p 42.size
        p (2**64).size
        p (2**64 - 1).size
        p (2**128).size
        p 0.3.rationalize
        p 2.5.rationalize
        p 3.14159.rationalize
        p 1.333.rationalize(0.01)
        p Rational(2.5)
        p Rational(1.5, 0.5)
        begin
          Complex(nil)
        rescue TypeError => e
          puts e.message
        end
        p 2 ** Complex(0, 1)
        p Complex(6, 0).to_r
        p Complex(3, 4).numerator
        p 7.div(Rational(2))
        p 10.div(Rational(3, 2))
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "8\n9\n8\n17\n(3/10)\n(5/2)\n(314159/100000)\n(4/3)\n(5/2)\n(3/1)\ncan't convert nil into Complex\n(0.7692389013639721+0.6389612763136348i)\n(6/1)\n(3+4i)\n3\n6\n"
    );
}

#[test]
fn pack_float_native_and_encoding_directives() {
    let result = run_ruby(
        r#"
        p [1.5].pack("D").bytes
        p [1.5].pack("G").bytes
        p [3.14].pack("d").unpack("d")
        p [1].pack("l!").bytesize
        p [1].pack("i").bytesize
        p [1].pack("j").bytesize
        p ["hello world"].pack("M")
        p ["hi there folks"].pack("u").unpack("u")
        p(["hi there folks"].pack("u").unpack("u") == ["hi there folks"])
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[0, 0, 0, 0, 0, 0, 248, 63]\n[63, 248, 0, 0, 0, 0, 0, 0]\n[3.14]\n8\n4\n8\n\"hello world=\\n\"\n[\"hi there folks\"]\ntrue\n"
    );
}

#[test]
fn universal_reflection_and_to_set() {
    let result = run_ruby(
        r#"
        class Point
          def initialize(x, y)
            @x = x
            @y = y
          end
        end
        pt = Point.new(3, 4)
        p pt.instance_variables
        p pt.instance_variable_set(:@x, 99)
        p pt.instance_variable_get(:@x)
        p pt.instance_variable_defined?(:@y)
        p pt.instance_variable_defined?(:@z)
        p 42.instance_variables
        p 42.singleton_methods
        p 42.respond_to?(:instance_variable_get)
        p 42.send(:instance_variables)
        p [1, 2, 2, 3].to_set
        p (1..3).to_set
        p({ a: 1, b: 2 }.to_set.size)
        p(/x/.timeout)
        p Regexp.timeout
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[:@x, :@y]\n99\n99\ntrue\nfalse\n[]\n[]\ntrue\n[]\nSet[1, 2, 3]\nSet[1, 2, 3]\n2\nnil\nnil\n"
    );
}

#[test]
fn module_ordering_operators_and_subclasses() {
    let result = run_ruby(
        r#"
        class Animal; end
        class Dog < Animal; end
        class Cat < Animal; end
        p(Dog < Animal)
        p(Animal < Dog)
        p(Dog < Cat)
        p(Dog <= Dog)
        p(Animal > Dog)
        p(Dog <=> Animal)
        p(Dog <=> Cat)
        p(Dog <=> 5)
        p(Integer < Numeric)
        begin
          Dog < 5
        rescue TypeError => e
          puts e.message
        end
        class Base; end
        class Kid1 < Base; end
        class Kid2 < Base; end
        class GKid < Kid1; end
        p Base.subclasses.length
        p Base.subclasses.map { |c| c.to_s }.sort
        p Kid1.subclasses.map(&:to_s)
        p Base.singleton_class?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nfalse\nnil\ntrue\ntrue\n-1\nnil\nnil\ntrue\ncompared with non class/module\n2\n[\"Kid1\", \"Kid2\"]\n[\"GKid\"]\nfalse\n"
    );
}

// ---------------------------------------------------------------------------
// Runtime string `eval` / `instance_eval` (#97 stage 2 -- the eval VM).
//
// Every source below is held in a VARIABLE (or built with `.dup`/`+`), so it is
// NOT a string literal and therefore runs through the runtime eval VM (a
// tree-walking interpreter over prism), not the compile-time inline path a
// string literal takes. That is the surface these tests are here to cover.
// ---------------------------------------------------------------------------

#[test]
fn eval_dynamic_arithmetic_honours_precedence() {
    let result = run_ruby(r#"code = "1 + 2 * 3"; puts eval(code)"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\n");
}

#[test]
fn eval_dynamic_method_call_on_evaluated_receiver() {
    let result = run_ruby(r#"src = "[3, 1, 2].sort.inspect"; puts eval(src)"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[1, 2, 3]\n");
}

#[test]
fn eval_locals_defined_and_read_within_one_scope() {
    let result = run_ruby(r#"src = "a = 4; b = 5; a * b"; puts eval(src)"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "20\n");
}

#[test]
fn eval_string_interpolation_inside_source() {
    let result = run_ruby(r#"code = 'n = 6; "n=#{n * n}"'; puts eval(code)"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "n=36\n");
}

#[test]
fn eval_source_assembled_at_runtime() {
    let result = run_ruby(r#"op = "-"; puts eval("10 #{op} 3")"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\n");
}

#[test]
fn eval_control_flow_yields_last_expression() {
    let result = run_ruby(r#"src = "if 1 < 2 then :yes else :no end"; puts eval(src)"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "yes\n");
}

#[test]
fn eval_while_loop_in_the_vm() {
    let result = run_ruby(
        r#"src = "s = 0; i = 1; while i <= 4; s = s + i; i = i + 1; end; s"; puts eval(src)"#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n");
}

#[test]
fn eval_short_circuit_returns_the_operand() {
    let result = run_ruby(r#"src = "nil || 'fallback'"; puts eval(src)"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "fallback\n");
}

#[test]
fn eval_resolves_builtin_class_and_module_constants() {
    let result = run_ruby(
        r#"
        puts eval("Integer".dup)
        puts eval("Math::PI".dup).round(2)
        puts eval("Math.sqrt(81)".dup)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Integer\n3.14\n9.0\n");
}

#[test]
fn eval_resolves_user_constants_and_classes() {
    let result = run_ruby(
        r#"
        FOO = 42
        class Widget; end
        puts eval("FOO".dup)
        puts eval("Widget".dup)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\nWidget\n");
}

#[test]
fn eval_collections_arrays_hashes_ranges() {
    let result = run_ruby(
        r#"
        puts eval("[1, 2, 3, 4].length".dup)
        puts eval("{ a: 1, b: 2 }.length".dup)
        puts eval("(1..5).to_a.inspect".dup)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "4\n2\n[1, 2, 3, 4, 5]\n");
}

#[test]
fn eval_sees_globals_and_main_object_ivars() {
    let result = run_ruby(
        r#"
        $g = "global"
        @iv = 41
        puts eval("$g".dup)
        puts eval("@iv + 1".dup)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "global\n42\n");
}

#[test]
fn eval_non_string_argument_is_a_type_error() {
    let result = run_ruby(
        r#"
        begin
          eval(123)
        rescue TypeError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "no implicit conversion of Integer into String\n"
    );
}

#[test]
fn eval_syntax_error_is_catchable_at_runtime() {
    let result = run_ruby(
        r#"
        bad = "1 +"
        begin
          eval(bad)
        rescue SyntaxError
          puts "caught"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught\n");
}

#[test]
fn eval_value_is_usable_in_the_surrounding_expression() {
    let result = run_ruby(r#"a = "6"; b = "7"; puts(eval(a) * eval(b))"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn instance_eval_string_reads_the_receivers_ivars() {
    let result = run_ruby(
        r#"
        class Account
          def initialize(n)
            @balance = n
          end
        end
        acct = Account.new(100)
        src = "@balance + 5"
        puts acct.instance_eval(src)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "105\n");
}

#[test]
fn instance_eval_string_rebinds_self_to_a_literal_receiver() {
    let result = run_ruby(r#"src = "upcase.reverse"; puts "hello".instance_eval(src)"#);
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "OLLEH\n");
}

#[test]
fn instance_eval_string_mutates_an_ivar() {
    let result = run_ruby(
        r#"
        class Counter
          def initialize
            @n = 0
          end
        end
        c = Counter.new
        c.instance_eval("@n = @n + 3")
        puts c.instance_eval("@n")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n");
}

// ---------------------------------------------------------------------------
// Builtin method additions (conformance drawdown): Hash[], try_convert,
// Complex.polar, Module#include?, Regexp.last_match, Thread#join/#value, and
// the Random PRNG. Ruby-visible surface; the PRNG cases assert only
// deterministic guarantees (a seeded xorshift64* diverges from CRuby's MT).
// ---------------------------------------------------------------------------

#[test]
fn hash_class_bracket_constructor() {
    let result = run_ruby(
        r#"
        p Hash[]
        p Hash[1, 2, 3, 4]
        p Hash[[[:a, 1], [:b, 2]]]
        p Hash[{ x: 1 }]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "{}\n{1 => 2, 3 => 4}\n{a: 1, b: 2}\n{x: 1}\n"
    );
}

#[test]
fn array_and_hash_try_convert() {
    let result = run_ruby(
        r#"
        p Array.try_convert([1, 2])
        p Array.try_convert("no")
        p Hash.try_convert({ a: 1 })
        p Hash.try_convert(5)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[1, 2]\nnil\n{a: 1}\nnil\n");
}

#[test]
fn complex_polar_constructor() {
    let result = run_ruby("p Complex.polar(2, 0)");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "(2+0.0i)\n");
}

#[test]
fn module_include_predicate() {
    let result = run_ruby(
        r#"
        module Greetable; end
        class Person; include Greetable; end
        p Person.include?(Greetable)
        p Person.include?(Comparable)
        begin
          Person.include?(Object)
        rescue TypeError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nfalse\nwrong argument type Class (expected Module)\n"
    );
}

#[test]
fn regexp_last_match_and_groups() {
    let result = run_ruby(
        r#"
        "abc123" =~ /([a-z]+)(\d+)/
        p Regexp.last_match(1)
        p Regexp.last_match(2)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"abc\"\n\"123\"\n");
}

#[test]
fn thread_join_and_value_via_dynamic_dispatch() {
    // The threads live in an Array, so `join`/`value` dispatch dynamically
    // (the runtime Thread table), not the static codegen fast path.
    let result = run_ruby(
        r#"
        threads = 3.times.map { |i| Thread.new { i * 10 } }
        threads.each(&:join)
        p threads.map(&:value)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[0, 10, 20]\n");
}

#[test]
fn random_is_seeded_reproducible_and_typed() {
    let result = run_ruby(
        r#"
        p(Random.new(42).rand(1000) == Random.new(42).rand(1000))
        p(Random.new(1).rand(1000) != Random.new(2).rand(1000))
        p Random.new(5).rand(10).class
        p Random.new(5).rand(2.5).class
        p Random.new(5).rand.class
        p((r = Random.new(9).rand(6)) >= 0 && r < 6)
        p Random.new(1).bytes(8).bytesize
        p Random.new(123).seed
        p Random.new(3.9).seed
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\ntrue\nInteger\nFloat\nFloat\ntrue\n8\n123\n3\n"
    );
}

#[test]
fn random_rejects_non_positive_bounds() {
    let result = run_ruby(
        r#"
        begin; Random.new(1).rand(0); rescue ArgumentError => e; puts e.message; end
        begin; Random.new(1).rand(-3); rescue ArgumentError => e; puts e.message; end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "invalid argument - 0\ninvalid argument - -3\n");
}

#[test]
fn integer_sqrt_is_exact_including_bignums() {
    let result = run_ruby(
        r#"
        p Integer.sqrt(0)
        p Integer.sqrt(8)
        p Integer.sqrt(9)
        p Integer.sqrt(10**20)
        p Integer.sqrt(2**100)
        begin
          Integer.sqrt(-4)
        rescue Math::DomainError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "0\n2\n3\n10000000000\n1125899906842624\nNumerical argument is out of domain - \"isqrt\"\n"
    );
}

#[test]
fn kernel_catch_throw_sleep_via_dynamic_dispatch() {
    // `send`/`&:` force dynamic dispatch through the Kernel table rather than
    // the static codegen fast path.
    let result = run_ruby(
        r#"
        p send(:catch, :done) { throw :done, 42 }
        p [1, 2, 3].map { |x| catch(:skip) { throw :skip, -1 if x == 2; x } }
        send(:sleep, 0)
        puts "slept"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n[1, -1, 3]\nslept\n");
}

#[test]
fn clone_honors_the_freeze_keyword() {
    let result = run_ruby(
        r#"
        a = "hi".freeze
        p a.clone.frozen?
        p a.clone(freeze: false).frozen?
        p a.clone(freeze: true).frozen?
        b = "yo"
        p b.clone.frozen?
        p b.clone(freeze: true).frozen?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\ntrue\nfalse\ntrue\n");
}

#[test]
fn string_index_accepts_a_start_offset() {
    let result = run_ruby(
        r#"
        p "hello world".index("o")
        p "hello world".index("o", 5)
        p "hello world".index("o", -3)
        p "hello".index("z", 2)
        p "abcabc".index(/b/, 2)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "4\n7\nnil\nnil\n4\n");
}

#[test]
fn module_method_defined_accepts_the_inherit_flag() {
    let result = run_ruby(
        r#"
        class Foo; def bar; end; end
        p Foo.method_defined?(:bar)
        p Foo.method_defined?(:bar, true)
        p Foo.method_defined?(:nope, false)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\nfalse\n");
}

#[test]
fn string_concat_is_variadic() {
    let result = run_ruby(
        r#"
        s = "a"
        s.concat("b", "c", "d")
        puts s
        t = "x"
        t << "y" << "z"
        puts t
        u = "n"
        u.concat(65, 66)
        puts u
        puts "keep".concat
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "abcd\nxyz\nnAB\nkeep\n");
}

#[test]
fn exception_cause_chains_from_active_rescue() {
    let result = run_ruby(
        r#"
        begin
          begin
            raise "inner"
          rescue
            raise "outer"
          end
        rescue => e
          puts e.message
          puts e.cause.message
          puts e.cause.class
        end
        begin
          raise "solo"
        rescue => e
          p e.cause
        end
        begin
          begin
            Integer("x")
          rescue
            raise ArgumentError, "wrapped"
          end
        rescue => e
          puts e.cause.class
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "outer\ninner\nRuntimeError\nnil\nArgumentError\n"
    );
}

#[test]
fn string_lines_chomp_and_prepend_variadic() {
    let result = run_ruby(
        r#"
        p "a\nb\nc".lines
        p "a\nb\nc".lines(chomp: true)
        p "a\r\nb\r\nc\r\n".lines(chomp: true)
        p "a-b-c".lines("-")
        collected = []
        "x\ny\n".each_line(chomp: true) { |l| collected << l }
        p collected
        t = "world"
        t.prepend("hello ", "big ")
        p t
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[\"a\\n\", \"b\\n\", \"c\"]\n[\"a\", \"b\", \"c\"]\n[\"a\", \"b\", \"c\"]\n[\"a-\", \"b-\", \"c\"]\n[\"x\", \"y\"]\n\"hello big world\"\n"
    );
}

#[test]
fn time_at_units_matchdata_slice_and_float_exponent() {
    let result = run_ruby(
        r#"
        p Time.at(0, 500, :millisecond).to_f
        p Time.at(0, 500, :nanosecond).to_f
        md = "2024-01-31".match(/(\d+)-(\d+)-(\d+)/)
        p md[1, 2]
        p md[1..]
        p(md == "2024-01-31".match(/(\d+)-(\d+)-(\d+)/))
        p 5.0e-7
        p 1.0e20
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "0.5\n5.0e-07\n[\"01\", \"31\"]\n[\"01\", \"31\"]\ntrue\n5.0e-07\n1.0e+20\n"
    );
}

#[test]
fn range_size_covers_endless_and_float_ends() {
    let result = run_ruby(
        r#"
        p (1..5).size
        p (1...5).size
        p (1..).size
        p (1..5.5).size
        p (1...5.5).size
        p (10..1).size
        p Proc.new { |x| x * 3 }.call(4)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\n4\nInfinity\n5\n5\n0\n12\n");
}

#[test]
fn enumerable_predicates_accept_a_pattern() {
    let result = run_ruby(
        r#"
        p [1, 2, 3].any?(Integer)
        p [1, "a", 3].all?(Integer)
        p [1, 2, 3].none?(String)
        p [1, 2, 3].one?(2)
        p [1, 2, 3].any?(4..10)
        p %w[foo bar].all?(/o|a/)
        p({ a: 1 }.any?([:a, 1]))
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\ntrue\ntrue\nfalse\ntrue\ntrue\n");
}

#[test]
fn data_constructs_positionally_or_by_keyword() {
    let result = run_ruby(
        r#"
        Point = Data.define(:x, :y)
        a = Point.new(1, 2)
        b = Point.new(x: 1, y: 2)
        p [a.x, a.y]
        p(a == b)
        p a.frozen?
        p(Point.new(1) rescue $!.class)
        p(Point.new(x: 1, y: 2, z: 3) rescue $!.class)
        p(a.with(z: 9) rescue $!.class)
        p a.with(y: 5).to_h
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2]\ntrue\ntrue\nArgumentError\nArgumentError\nArgumentError\n{x: 1, y: 5}\n"
    );
}

#[test]
fn rational_round_family_takes_precision() {
    let result = run_ruby(
        r#"
        r = Rational(157, 50)
        p r.round(2)
        p r.round(-1)
        p r.round
        p r.floor(1)
        p r.ceil(1)
        p r.truncate(1)
        p Rational(-7, 2).round
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "(157/50)\n0\n3\n(157/50)\n(157/50)\n(157/50)\n-4\n"
    );
}

#[test]
fn proc_parameters_reflect_the_signature() {
    let result = run_ruby(
        r#"
        p proc { |x, y| }.parameters
        p lambda { |x, y| }.parameters
        p proc { |a, b = 1, *c, d, k:, m: 2, **n, &blk| }.parameters
        p ->(a, b) { }.parameters
        p proc { |*| }.parameters
        p proc { |**| }.parameters
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[[:opt, :x], [:opt, :y]]\n\
         [[:req, :x], [:req, :y]]\n\
         [[:opt, :a], [:opt, :b], [:rest, :c], [:opt, :d], [:keyreq, :k], [:key, :m], [:keyrest, :n], [:block, :blk]]\n\
         [[:req, :a], [:req, :b]]\n\
         [[:rest, :*]]\n\
         [[:keyrest, :**]]\n",
    );
}

#[test]
fn range_arguments_across_string_and_array_slicing() {
    let result = run_ruby(
        r#"
        p "hello".byteslice(1..3)
        p "hello".byteslice(2..)
        p "hello".byteslice(10..12)
        a = [1, 2, 3, 4]
        p a.slice!(1..2)
        p a
        b = [0, 0, 0, 0, 0]
        b.fill(1..2) { |i| i + 100 }
        p b
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"ell\"\n\"llo\"\nnil\n[2, 3]\n[1, 4]\n[0, 101, 102, 0, 0]\n"
    );
}

#[test]
fn hash_transform_keys_accepts_a_mapping() {
    let result = run_ruby(
        r#"
        p({ a: 1, b: 2 }.transform_keys(a: :x))
        p({ a: 1, b: 2 }.transform_keys(a: :x) { |k| k.to_s })
        h = { a: 1, b: 2 }
        h.transform_keys!(b: :y)
        p h
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "{x: 1, b: 2}\n{x: 1, \"b\" => 2}\n{a: 1, y: 2}\n");
}

#[test]
fn float_round_half_modes_and_string_hash_sub() {
    let result = run_ruby(
        r#"
        p 2.5.round(half: :even)
        p 3.5.round(half: :even)
        p 2.5.round(half: :down)
        puts "hello".sub("l", "l" => "X")
        puts "hello".gsub("l", "l" => "X")
        p 5 << 2.0
        p 100 >> 1.9
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n4\n2\nheXlo\nheXXo\n20\n50\n");
}

#[test]
fn matchdata_inspect_shows_groups_and_regexp_string_methods() {
    let result = run_ruby(
        r#"
        p "hello".match(/l(l)o/)
        p "2024-01".match(/(?<y>\d+)-(?<m>\d+)/)
        p "hello".start_with?(/he/)
        p "hello".start_with?(/ell/)
        p "hello".byteindex(/l+/)
        p "hello".byterindex(/l+/)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "#<MatchData \"llo\" 1:\"l\">\n#<MatchData \"2024-01\" y:\"2024\" m:\"01\">\ntrue\nfalse\n2\n3\n"
    );
}

#[test]
fn numeric_step_accepts_by_and_to_keywords() {
    let result = run_ruby(
        r#"
        1.step(by: 2, to: 10) { |i| print i, " " }
        puts
        1.step(10, 2) { |i| print i, " " }
        puts
        1.step(to: 5) { |i| print i, " " }
        puts
        10.step(by: -3, to: 1) { |i| print i, " " }
        puts
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "1 3 5 7 9 \n1 3 5 7 9 \n1 2 3 4 5 \n10 7 4 1 \n"
    );
}

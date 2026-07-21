use crate::support::{run_ruby};

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
fn break_bubbles_through_nested_begins_to_the_outer_loop() {
    // An inner `begin` (loop labels cleared) `?`-propagates the `break` up to
    // the outer `begin`, which -- inline in the loop -- performs the literal
    // translation. The crossing scan descends THROUGH nested begins (H2).
    let result = run_ruby(
        r#"
        i = 0
        while i < 4
          begin
            begin
              break if i == 2
              puts "inner #{i}"
            rescue
            end
          rescue
          end
          i += 1
        end
        puts "out"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "inner 0\ninner 1\nout\n");
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

#[test]
fn case_when_with_class_candidates_checks_instance_ancestry() {
    // `Module#===` -- previously a compile-time rejection (`when Integer`
    // never worked); now real Ruby's instance-of check.
    let result = run_ruby(
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

/// `Range#===` IS `#cover?` -- what makes `case x when 1..50` actually
/// match (previously it fell to structural equality and silently never
/// did). Endpoint semantics oracle-verified incl. exclusive ends, floats,
/// and incomparable-subject false.
#[test]
fn range_case_equality_is_real() {
    let result = run_ruby(
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
fn eval_while_loop_in_the_vm() {
    let result = run_ruby(
        r#"src = "s = 0; i = 1; while i <= 4; s = s + i; i = i + 1; end; s"; puts eval(src)"#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n");
}

#[test]
fn rand_ranges_edge_cases_and_random_equality() {
    let result = run_ruby(
        r#"
        p rand(5...5)
        p rand(5..3)
        p rand(-3) >= 0
        srand(3); v = rand(1..1000); p (1..1000).cover?(v)
        p Random.new(1) == Random.new(1)
        p Random.new(1) == Random.new(2)
        r = Random.new(7); p r == r
        p Random.new_seed.class
        def t; yield; rescue => e; e.class; end
        p t { rand(1..) }
        p t { rand(..5) }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "nil\nnil\ntrue\ntrue\ntrue\nfalse\ntrue\nInteger\nErrno::EDOM\nErrno::EDOM\n",
    );
}

#[test]
fn break_from_a_block_passed_to_a_singleton_method_returns_from_it() {
    // `break` in the block yields the method's return value -- the method-body
    // lambda's terminal arm folds `Signal::Break` into an `Ok` return, exactly
    // as an ordinary method does.
    let result = run_ruby(
        r##"
        obj = Object.new
        def obj.count
          yield 1
          yield 2
          yield 3
          "finished"
        end
        p(obj.count { |n| break "stop#{n}" if n == 2 })
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"stop2\"\n");
}

#[test]
fn next_from_a_block_passed_to_a_singleton_method_is_its_yield_value() {
    let result = run_ruby(
        r#"
        obj = Object.new
        def obj.run
          a = yield 1
          b = yield 2
          [a, b]
        end
        p(obj.run { |n| next n * 10 })
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[10, 20]\n");
}

/// `case`/`when`, an `in` value/pin pattern, and `grep` all dispatch the
/// pattern's own `===`. The native ladder they used to share bottomed out in
/// `==`, so a user class whose `===` differs from its `==` silently took the
/// WRONG branch, a singleton `===` on a class was ignored, and `lazy.grep`
/// disagreed with eager `grep` on the same pattern. An Object-typed pin was
/// worse than wrong: it emitted a bare `Arc<Even>`, and the GENERATED program
/// failed to compile. The builtin pattern shapes must keep their meaning.
#[test]
fn case_equality_dispatches_a_user_defined_triple_equals() {
    let result = run_ruby(
        r#"
        class Even
          def ===(n); n.even?; end
        end
        e = Even.new
        puts(case 4 when e then "when-obj" else "when-missed" end)
        puts(case 3 when e then "when-obj" else "when-missed" end)
        r = case 4
            in ^e then "pin"
            else "pin-missed"
            end
        puts r
        puts [1, 2, 3, 4].grep(e).inspect
        puts (1..4).lazy.grep(e).to_a.inspect
        class Trip; end
        def Trip.===(n); n % 3 == 0; end
        puts(case 9 when Trip then "singleton" else "singleton-missed" end)
        puts(case 5 when Integer then "builtin-class" else "no" end)
        puts(case "hi" when /h/ then "regexp" else "no" end)
        puts(case 3 when 1..5 then "range" else "no" end)
        puts(case 2 when 1, 2, 3 then "listed" else "no" end)
        cands = [7, 8]
        puts(case 8 when *cands then "splat" else "no" end)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "when-obj\nwhen-missed\npin\n[2, 4]\n[2, 4]\nsingleton\n\
         builtin-class\nregexp\nrange\nlisted\nsplat\n"
    );
}

#[test]
fn defined_yield_reflects_block_presence_at_runtime() {
    // defined?(yield) is nil without a block, "yield" with one.
    let result = run_ruby(
        r#"
        def f
          defined?(yield)
        end
        p f
        p f { 1 }
        def g(&blk)
          defined?(yield)
        end
        p g
        p g { 42 }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "nil\n\"yield\"\nnil\n\"yield\"\n");
}

#[test]
fn defined_classifies_calls_assignments_and_keyword_literals() {
    // A method call answers "method" only if the receiver responds; a bare
    // undefined name is nil. Assignments answer "assignment". nil/true/false
    // answer their own name.
    let result = run_ruby(
        r#"
        def foo; end
        p defined?(foo)
        p defined?(undefined_zzz)
        p defined?(1 + 2)
        p defined?(1.nope_zzz)
        p defined?(x = 2)
        p defined?(@iv = 3)
        p defined?(nil)
        p defined?(true)
        p defined?(false)
        p defined?(puts)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"method\"\nnil\n\"method\"\nnil\n\"assignment\"\n\"assignment\"\n\
         \"nil\"\n\"true\"\n\"false\"\n\"method\"\n"
    );
}

#[test]
fn defined_globals_and_match_vars_check_definedness_at_runtime() {
    // A user global is "global-variable" only once assigned; a predefined
    // special ($~) always is; a match capture only when it participated. An
    // array literal is defined only if every element is.
    let result = run_ruby(
        r#"
        $set_g = 1
        p defined?($set_g)
        p defined?($never_set_g)
        p defined?($~)
        p defined?($1)
        "hi" =~ /(h)/
        p defined?($1)
        p defined?($2)
        p defined?([1, Array])
        p defined?([Nonexist_zzz, Array])
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"global-variable\"\nnil\n\"global-variable\"\nnil\n\
         \"global-variable\"\nnil\n\"expression\"\nnil\n"
    );
}

use crate::support::run_ruby;

// --- Real escaping Proc/closures, yield, block_given?, self-capture ---

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
fn a_block_escaping_from_inside_another_escaping_block_works() {
    // The original blanket rejection of Proc-within-Proc was LIFTED
    // (`Thread.new { m.synchronize { } }` is the canonical threading
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
fn a_nested_block_captures_the_outer_blocks_own_local() {
    // The INNER block reads the OUTER block's own param `n`. The outer
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

#[test]
fn inlined_times_block_local_is_captured_by_a_nested_block() {
    // A `.times` block-LOCAL (`|i; n|`) captured by a nested escaping block is
    // cell-wrapped for the same reason as its param (H1).
    let result = run_ruby(
        r##"
        store = []
        [1].each do |a|
          3.times do |i; n|
            n = i * 10
            store << ->() { "#{a}:#{i}:#{n}" }
          end
        end
        store.each { |p| puts p.call }
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1:0:0\n1:1:10\n1:2:20\n");
}

#[test]
fn an_escaping_block_capturing_self_via_explicit_self_dot_method() {
    // Before `HirNode::SelfRef` existed, an escaping block could only ever
    // capture `self` implicitly via a bare `@ivar` reference --
    // `self.method_name` is another way a block needs the same capture (see
    // `analyze::captures`'s new `SelfRef` arm). Also exercises implicit-self
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
    assert_eq!(
        result.stdout,
        "hello world\nhello\nworld\n---\nhello\nworld\n"
    );
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
fn scan_with_a_string_pattern_matches_literally() {
    // A String pattern is a literal (no metacharacters), matches
    // non-overlapping, and an empty pattern matches at every character
    // boundary. The block form yields each match and returns the receiver.
    let result = run_ruby(
        r#"
        p "MixedCase".scan("e")
        p "aaaa".scan("aa")
        p "abc".scan("z")
        p "café".scan("")
        matches = []
        returned = "banana".scan("an") { |m| matches << m.upcase }
        p matches
        p returned == "banana"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[\"e\", \"e\"]\n[\"aa\", \"aa\"]\n[]\n[\"\", \"\", \"\", \"\", \"\"]\n[\"AN\", \"AN\"]\ntrue\n"
    );
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
fn process_identity_and_scheduling_surface() {
    // Process ids/scheduling against libc, plus the PRIO_* selector constants.
    let result = run_ruby(
        r#"
        p [Process.uid.class, Process.gid.class, Process.euid.class, Process.egid.class]
        p [Process.getpgrp.class, Process.getsid.class]
        p Process.clock_getres(Process::CLOCK_MONOTONIC).class
        p Process.clock_getres(Process::CLOCK_REALTIME, :nanosecond).class
        p [Process::PRIO_PROCESS, Process::PRIO_PGRP, Process::PRIO_USER]
        p Process.groups.all? { |g| g.is_a?(Integer) }
        p Process.getpriority(Process::PRIO_PROCESS, 0).class
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[Integer, Integer, Integer, Integer]\n[Integer, Integer]\nFloat\nInteger\n[0, 1, 2]\ntrue\nInteger\n"
    );
}

#[test]
fn blockless_forms_return_real_enumerators() {
    // A blockless map returns a real Enumerator whose `each` re-invokes the
    // captured method. Oracle-verified.
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

/// Symbol Tier A + the `&:sym` block-argument conversion
/// (`Symbol#to_proc`), previously unsupported.
#[test]
fn symbol_breadth_and_to_proc() {
    let result = run_ruby(
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

/// A native anonymous struct is Enumerable and honours a class-body method
/// block, exactly like the synthesized constant form.
#[test]
fn anonymous_struct_is_enumerable_and_keeps_its_method_block() {
    let result = run_ruby(
        r#"
        k = Struct.new(:a, :b, :c) do
          def sum
            to_a.sum
          end
        end
        o = k.new(1, 2, 3)
        p o.map { |v| v * 10 }
        p o.sum
        p o.select { |v| v.odd? }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[10, 20, 30]\n6\n[1, 3]\n");
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
fn lambda_escaping_inside_an_escaping_block() {
    // A lambda literal nested inside a stored (escaping) block used to be
    // rejected at compile time; it now composes through the capture analysis
    // like any other escaping closure.
    let result = run_ruby(
        r#"
        makers = [1, 2, 3].map do |n|
          -> { n * 10 }
        end
        p makers.map(&:call)

        def build
          total = 0
          adder = ->(x) { total += x }
          [adder, -> { total }]
        end
        add, get = build
        add.call(5)
        add.call(7)
        p get.call

        squares = (1..3).map { |i| -> { i * i } }
        p squares.map(&:call)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[10, 20, 30]\n12\n[1, 4, 9]\n");
}

#[test]
fn nested_closure_captures_enclosing_blocks_own_local() {
    // A name that is both an enclosing block's own local AND captured by a
    // nested escaping closure must be declared once (as a shared cell), not
    // also fresh-declared by the own-locals prelude.
    let result = run_ruby(
        r#"
        adders = []
        [1, 2, 3].each do |n|
          base = n * 100
          adders << -> { base + n }
        end
        p adders.map(&:call)

        procs = []
        [10, 20].each do |k|
          acc = 0
          procs << -> { acc }
          acc = k + 1
          procs << -> { acc }
        end
        p procs.map(&:call)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[101, 202, 303]\n[11, 11, 21, 21]\n");
}

#[test]
fn nonlocal_return_from_proc_unwinds_the_home_method() {
    // A non-lambda Proc's `return` returns from its creating method (live
    // home), including through builtin iterators and `yield`; a lambda's is
    // local.
    let result = run_ruby(
        r#"
        def simple; proc { return 30 }.call; 40; end
        p simple
        def cond(x); proc { return "yes" if x > 0 }.call; "no"; end
        p cond(5); p cond(-1)
        def multi; proc { return 1, 2, 3 }.call; [9]; end
        p multi
        def thru_each; proc { [1,2,3].each { |x| return x*10 if x==2 }; :none }.call; end
        p thru_each
        def thru_ewi; proc { [10,20,30].each_with_index { |v,i| return i if v==20 }; -1 }.call; end
        p thru_ewi
        def gives; yield; end
        def via_yield; gives { return 55 }; 66; end
        p via_yield
        def inner; proc { return "IR" }.call; "IN"; end
        def outer; x = inner; proc { return "O:#{x}" }.call; "ON"; end
        p outer
        def cd(n, acc); proc { return acc if n==0 }.call; cd(n-1, acc+n); end
        p cd(5, 0)
        def with_lambda; -> { return 30 }.call; 40; end
        p with_lambda
        def dbl(x); proc { return x*2 }.call; -1; end
        s = 0; 300.times { |i| s += dbl(i) }; p s
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "30\n\"yes\"\n\"no\"\n[1, 2, 3]\n20\n1\n55\n\"O:IR\"\n15\n40\n89700\n"
    );
}

#[test]
fn splat_into_a_proc_or_lambda_receiver() {
    let result = run_ruby(
        r#"
        add3 = ->(a, b, c) { a + b + c }
        args = [1, 2, 3]
        p add3.call(*args)
        p add3[*args]
        p add3.(*args)
        p add3.yield(*args)
        prc = proc { |a, b| a * b }
        p prc.call(*[4, 5])
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n6\n6\n6\n20\n");
}

#[test]
fn array_product_block_sample_and_each_slice_return() {
    // product's block form yields each tuple and returns self; sample(-n) has
    // its own message; each_slice/each_cons block forms return the receiver.
    let result = run_ruby(
        r#"
        r = []
        ret = [1, 2].product([3, 4]) { |t| r << t }
        p r
        p ret
        p([1, 2, 3].each_slice(2) { |s| })
        p([1, 2, 3].each_cons(2) { |s| })
        begin
          [1, 2].sample(-1)
        rescue ArgumentError => e
          p e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[[1, 3], [1, 4], [2, 3], [2, 4]]\n[1, 2]\n[1, 2, 3]\n[1, 2, 3]\n\"negative sample number\"\n",
    );
}

#[test]
fn array_join_recursive_and_delete_block() {
    // join flattens nested arrays under the same separator; delete's not-found
    // block supplies the answer.
    let result = run_ruby(
        r#"
        p [1, [2, [3, 4]], 5].join("-")
        p [[1], "a", [2, [3]]].join("|")
        p ["a", "b"].delete("z") { "missing" }
        p ["a", "b"].delete("a") { "missing" }
        p [1, 2].delete(9)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"1-2-3-4-5\"\n\"1|a|2|3\"\n\"missing\"\n\"a\"\nnil\n"
    );
}

#[test]
fn enumerator_feed_sets_yield_return() {
    // #feed sets the value the paused y.yield returns on the next #next;
    // feeding twice before a #next raises TypeError; #feed answers nil.
    let result = run_ruby(
        r#"
        g = Enumerator.new { |y| got = y.yield(10); y << (got * 2) }
        p g.next
        g.feed(5)
        p g.next
        k = Enumerator.new { |y| y.yield(1) }
        k.next
        p k.feed(:x)
        m = Enumerator.new { |y| y.yield(1); y.yield(2) }
        m.next
        m.feed(:a)
        begin
          m.feed(:b)
        rescue => e
          p e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n10\nnil\n\"feed value already set\"\n");
}

#[test]
fn proc_source_location_is_a_string_integer_pair() {
    let result = run_ruby(
        r#"
        loc = ->(x) { x }.source_location
        p loc.class
        p loc.length
        p loc[0].class
        p loc[1].class
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Array\n2\nString\nInteger\n");
}

#[test]
fn break_in_a_non_lambda_proc_call_raises_local_jump_error() {
    // `break` inside an explicitly created proc invoked via `#call` has no
    // iterator to unwind to, so CRuby raises LocalJumpError (`#reason`
    // `:break`) -- even while the creating iterator is still live. A lambda's
    // `break`, by contrast, just returns from the lambda.
    let result = run_ruby(
        r##"
        p(-> { break 9 }.call)
        def orphan; proc { break 1 }; end
        begin
          orphan.call
        rescue LocalJumpError => e
          puts "#{e.message} / #{e.reason.inspect}"
        end
        [1, 2].each do |x|
          pr = proc { break :pb }
          begin
            pr.call
          rescue LocalJumpError => e
            puts "live: #{e.message}"
          end
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "9\nbreak from proc-closure / :break\nlive: break from proc-closure\nlive: break from proc-closure\n"
    );
}

#[test]
fn a_procs_own_block_param_receives_the_call_site_block() {
    // A proc/lambda that declares its own `&block` parameter receives the
    // block passed to its `#call` -- the block rides the closure's third
    // parameter into the body, where `b.call`/`b.nil?` reflect it. Covers a
    // Proc-typed receiver (the lambda literal, a fast-path `#call`), a Poly
    // local (`f`/`g`, the dynamic `Proc#call` builtin), a blockless call
    // (binds nil), and a passed block that captures an enclosing local.
    let result = run_ruby(
        r#"
        p(->(&b) { b.call(9) }.call { |x| x + 1 })
        f = ->(&b) { b.nil? ? "none" : b.call(1) }
        p f.call
        p(f.call { |x| x * 100 })
        g = ->(a, &b) { a + b.call(a) }
        p(g.call(5) { |x| x * 2 })
        base = 50
        p(->(&b) { b.call(3) }.call { |x| base + x })
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n\"none\"\n100\n15\n53\n");
}

#[test]
fn deeply_nested_dynamic_blocks_share_locals_through_capture_cells() {
    // bm_ao_render's shape: `.times` on an UNTYPED receiver makes every
    // level a real escaping Proc, and the innermost block both assigns its
    // own local (`vf`) and reads outer block params. The mid-chain block
    // used to panic ("capturing its enclosing BLOCK's own local") because
    // the guard ran before the nested-capture cell machinery classified
    // the name; it now recognizes a deeper block's own local. The
    // accumulator (`rad`) round-trips through three closure levels via its
    // cell. Output oracle-verified.
    let result = run_ruby(
        r#"
        def render(n)
          n.times do |x|
            rad = 0.0
            n.times do |v|
              n.times do |u|
                vf = v.to_f
                rad = rad + vf + u.to_f
              end
            end
            puts rad
          end
        end
        render(2)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "4.0\n4.0\n");
}

#[test]
fn a_nested_block_reading_outer_cells_and_writing_its_own_locals_composes() {
    // Four levels deep, mixing: a method-level captured local (`total`),
    // an outer block's cell-promoted local (`row`), block params read from
    // two levels down, and inner-only locals. Oracle-verified.
    let result = run_ruby(
        r#"
        def grid(n)
          total = 0
          n.times do |a|
            row = 0
            n.times do |b|
              n.times do |c|
                cell = a * 100 + b * 10 + c
                row = row + cell
                n.times do |d|
                  bump = d + cell
                  total = total + bump
                end
              end
            end
            total = total + row
          end
          total
        end
        p grid(2)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1340\n");
}

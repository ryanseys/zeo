use crate::support::{run_ruby, run_ruby_packages, run_ruby_project};

#[test]
fn operators_work_on_locals_not_just_literals() {
    // Originally, only a literal-on-literal `+` compiled at all. This
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
fn safe_navigation_on_a_non_nil_receiver() {
    // Only proves `&.` doesn't regress a normal, statically-typed dispatch --
    // a receiver that's *actually* nil at runtime needs a nilable/union type
    // the compiler's `TyKind` doesn't have yet (every `New` is unconditionally
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
fn ruby_engine_identifies_as_ruby() {
    // The north star is byte-for-byte MRI parity, so the engine-identity
    // constants report CRuby's own: RUBY_ENGINE == "ruby", RUBY_ENGINE_VERSION
    // == RUBY_VERSION, and RUBY_DESCRIPTION takes version.c's exact banner
    // (`ruby <ver> (<date> revision <short-rev>) +PRISM [<platform>]`).
    let result = run_ruby(
        r#"
        puts RUBY_ENGINE
        puts RUBY_ENGINE_VERSION == RUBY_VERSION
        puts RUBY_DESCRIPTION.start_with?("ruby #{RUBY_VERSION} ")
        puts RUBY_DESCRIPTION.include?("+PRISM")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "ruby\ntrue\ntrue\ntrue\n");
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
fn kernel_callee_putc_and_public_method() {
    let result = run_ruby(
        r##"
        def who = [__method__, __callee__]
        p who
        putc 72
        putc "i"
        putc "\n"
        p putc(321).class
        puts
        p putc("BC")
        puts
        class Widget
          def render = "drawn"
          private
          def secret = 42
        end
        w = Widget.new
        p w.public_method(:render).call
        begin
          w.public_method(:secret)
        rescue NameError => e
          puts e.message
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[:who, :who]\nHi\nAInteger\n\nB\"BC\"\n\n\"drawn\"\n\
         method 'secret' for class 'Widget' is private\n"
    );
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
fn nil_predicate_works_universally() {
    // `.nil?` on Poly, builtin, and Object receivers, with a user override
    // winning.
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
fn a_stdlib_style_feature_drops_in_through_an_i_search_root() {
    // stdlib is delivered as ordinary `-I <lib>` load-path roots (no
    // bespoke flag) -- pointing `-I` at a Ruby checkout's `lib` makes each
    // `require "feature"` resolve a real stdlib `.rb`. This models that with a
    // pure-Ruby "stdlib" file living under an `-I` root, required by name and
    // compiled + run through the ordinary loader path.
    let result = run_ruby_project(
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
fn builtin_receivers_dispatch_dynamically() {
    // send_value's builtin table (oracle-verified): Array#each on a
    // literal, Hash#each, send(:length) on an Array, hash/array ops on a
    // Poly ivar, and a rescuable NoMethodError from a builtin receiver.
    let result = run_ruby_packages(
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
fn self_referential_collections_print_recursion_markers() {
    // Pre-15.2 both of these self-deadlocked on the collection's own
    // non-reentrant payload Mutex.
    let result = run_ruby(
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

#[test]
fn top_level_self_is_the_main_object() {
    let result = run_ruby("puts self.is_a?(Object)\n");
    assert_eq!(result.stdout, "true\n");
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
fn argv_is_seeded_from_the_command_line() {
    // No args in this harness: ARGV exists and is empty (CRuby startup
    // parity; argv-carrying coverage lives in the conformance corpus).
    let result = run_ruby("p ARGV\nputs ARGV.length\n");
    assert_eq!(result.stdout, "[]\n0\n");
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
fn warn_and_stderr_writes_land_on_stderr_not_stdout() {
    // `warn`, `$stderr.puts`, and `STDERR.write` all go to the error stream;
    // `$stdout.puts` stays on stdout. The two streams are asserted separately,
    // so a leak in either direction fails the test.
    let result = run_ruby(
        r#"
        $stdout.puts "out1"
        warn "w1"
        $stderr.puts "err1"
        STDERR.write "err2\n"
        $stdout.puts "out2"
        warn "w2", "w3"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "out1\nout2\n");
    assert_eq!(result.stderr, "w1\nerr1\nerr2\nw2\nw3\n");
}

#[test]
fn warn_category_suppresses_only_deprecated() {
    // :deprecated is off by default (prints nothing); :experimental and the
    // uncategorized form print to stderr. The kwarg Hash is never itself
    // printed.
    let result = run_ruby(
        r#"
        warn("plain")
        warn("dep", category: :deprecated)
        warn("exp", category: :experimental)
        warn("a", "b", category: :deprecated)
        warn("c", "d", category: :experimental)
        puts "done"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "done\n");
    assert_eq!(result.stderr, "plain\nexp\nc\nd\n");
}

#[test]
fn bsearch_find_any_mode_follows_the_comparator_protocol() {
    // A Numeric block result selects find-any mode: 0 is a hit, negative
    // searches the lower half, positive the upper; a non-hit answers nil. The
    // boolean find-minimum mode still works alongside it.
    let result = run_ruby(
        r#"
        a = [0, 4, 7, 10, 12]
        p a.bsearch { |x| 7 <=> x }
        p a.bsearch { |x| 10 <=> x }
        p a.bsearch_index { |x| 12 <=> x }
        p(a.bsearch { |x| 3 <=> x })
        p a.bsearch { |x| x >= 10 }
        p [1, 2, 3].bsearch { |x| 1 - x }
        p((1..100).bsearch { |x| x >= 8 })
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\n10\n4\nnil\n10\n1\n8\n");
}

#[test]
fn empty_parens_are_nil_and_concat_snapshots_its_sources() {
    // `()` is nil (falsy as a condition, a nil value in expression position);
    // `Array#concat` copies all sources before appending, so a self-aliasing
    // `a.concat(a, a)` terminates at 6 elements rather than feeding itself.
    let result = run_ruby(
        r#"
        p(())
        p((() && true))
        n = 0
        while () ; n += 1 ; end
        p n
        a = [1, 2]
        a.concat(a, a)
        p a
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "nil\nnil\n0\n[1, 2, 1, 2, 1, 2]\n");
}

#[test]
fn operations_with_the_receiver_as_their_own_argument() {
    // `s + s` hands the SAME Arc<Mutex<..>> in as both receiver and argument.
    // parking_lot's Mutex is not reentrant, so any implementation taking both
    // guards in one expression deadlocks -- the process hangs with no output
    // and no error, which is strictly worse than a wrong answer. Every
    // self-argument shape is covered here because the bug is silent: it
    // surfaces as a timeout, never as a failed assertion.
    let result = run_ruby(
        r#"
        s = "ab"
        p s + s
        p s * 2
        p s.concat(s)
        p Encoding.compatible?(s, s)
        a = [1, 2]
        p(a <=> a)
        p a == a
        p a + a
        p a - a
        p a & a
        p a | a
        h = {x: 1}
        p h == h
        p h.merge(h)
        r = Random.new(5)
        p r == r
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"abab\"\n\
         \"abab\"\n\
         \"abab\"\n\
         #<Encoding:UTF-8>\n\
         0\n\
         true\n\
         [1, 2, 1, 2]\n\
         []\n\
         [1, 2]\n\
         [1, 2]\n\
         true\n\
         {x: 1}\n\
         true\n",
    );
}

/// `Monitor` is reentrant where `Mutex` deadlocks: the owner may enter again,
/// and the lock releases only at the outermost exit.
#[test]
fn monitor_is_a_reentrant_lock() {
    let result = run_ruby(
        r##"
        require "monitor"
        m = Monitor.new
        m.synchronize do
          m.synchronize { p m.mon_owned? }
          p m.mon_locked?
        end
        p m.mon_locked?
        p(m.synchronize { 21 * 2 })
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\nfalse\n42\n");
}

/// The bundled `optparse` package parses the switch shapes it advertises,
/// with CRuby's own error messages. The full-fidelity check against real
/// `OptionParser` lives in `examples/optparse_subset.rb`.
#[test]
fn bundled_optparse_parses_switches_and_leaves_positionals() {
    let result = run_ruby_packages(
        &[(
            "main.rb",
            r##"
        require "optparse"
        opts = {}
        parser = OptionParser.new do |o|
          o.on("-v", "--verbose", "loud") { |x| opts[:verbose] = x }
          o.on("-n", "--name NAME", "who") { |v| opts[:name] = v }
        end
        argv = ["-v", "--name=matz", "file.txt", "--", "-notaflag"]
        parser.parse!(argv)
        p argv
        p opts[:verbose]
        p opts[:name]
        begin
          parser.parse!(["--nope"])
        rescue OptionParser::InvalidOption => e
          p e.message
        end
        begin
          parser.parse!(["--name"])
        rescue OptionParser::MissingArgument => e
          p e.message
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
        "[\"file.txt\", \"-notaflag\"]\ntrue\n\"matz\"\n\"invalid option: --nope\"\n\"missing argument: --name\"\n"
    );
}

/// Loading the Ruby half must not cost the native half: `require "strscan"`
/// resolves `crates/zeo-rt/ext/strscan/lib/strscan.rb`, which pulls the
/// native half in with `require "strscan.so"` -- CRuby's loader idiom.
#[test]
fn the_native_half_still_works_through_its_ruby_half() {
    let result = run_ruby(
        r##"
        require "strscan"
        require "json"
        s = StringScanner.new("hello world")
        p s.scan(/\w+/)
        p s.rest
        p JSON.dump({ "a" => 1 })
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"hello\"\n\" world\"\n\"{\\\"a\\\":1}\"\n");
}

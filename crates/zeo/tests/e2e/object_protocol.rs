use crate::support::run_ruby;

#[test]
fn float_to_s_scientific_notation_threshold() {
    // Fixed for decpt in -3..=15, else scientific -- matches CRuby's Float#to_s.
    let result = run_ruby(
        r#"
        puts 999999999999999.0
        puts 1000000000000000.0
        puts 6402373705728000.0
        puts 0.0001
        puts 0.00009
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "999999999999999.0\n1.0e+15\n6.402373705728e+15\n0.0001\n9.0e-05\n"
    );
}

#[test]
fn method_owner_composition_equality_and_curry() {
    let result = run_ruby(
        r##"
        class Animal
          def speak(sound) = "#{sound}!"
        end
        class Dog < Animal
        end
        d = Dog.new
        m = d.method(:speak)
        p m.owner
        p m.original_name
        shout = ->(s) { s.upcase }
        p (m >> shout).call("woof")
        p (m << shout).call("woof")
        p (1.method(:+) >> ->(n) { n * 2 }).call(10)
        p d.method(:speak) == d.method(:speak)
        p Dog.new.method(:speak) == Dog.new.method(:speak)
        p m.hash == d.method(:speak).hash
        class Adder
          def add3(a, b, c) = a + b + c
        end
        p Adder.new.method(:add3).curry[1][2][3]
        um = Dog.instance_method(:speak)
        p um.owner
        p um.bind(Dog.new).call("bark")
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Animal\n:speak\n\"WOOF!\"\n\"WOOF!\"\n22\ntrue\nfalse\ntrue\n6\nAnimal\n\"bark!\"\n"
    );
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
    assert_eq!(result.stdout, "(?x-mi:abc)\n(?mi-x:a.c)\n/abc/x\n/abc/mi\n");
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
fn dup_and_clone_on_collections_follow_the_frozen_rule() {
    // dup: fresh unfrozen payload (mutable even when the source is
    // frozen, and mutating it leaves the source untouched); clone:
    // carries the frozen flag.
    let result = run_ruby(
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
fn user_to_s_and_inspect_drive_puts_interpolation_and_p() {
    let result = run_ruby(
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
    let result = run_ruby(
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
fn enumerable_min_max_dispatch_a_user_spaceship() {
    let result = run_ruby(
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
    let result = run_ruby(
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

#[test]
fn string_tr_duplicate_from_char_uses_the_last_mapping() {
    // A char repeated in `from` takes its LAST corresponding `to` char
    // (CRuby's rule) -- previously the FIRST mapping wrongly won.
    let result = run_ruby(
        r#"
        p "a___b".tr("___", ".+-")
        p "abcaa".tr("aa", "xy")
        p "abcd".tr("abc", "x")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"a---b\"\n\"ybcyy\"\n\"xxxd\"\n");
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
    assert_eq!(
        result.stdout,
        "at 2023-11-14 22:13:20 UTC\n2023-11\nString\n"
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
fn array_insert_eql_uniq_spaceship_edges() {
    // insert past the end pads with nil; eql?/uniq are class-strict (1 != 1.0);
    // <=> of an array with itself (incl. a cycle) is 0 without deadlock.
    let result = run_ruby(
        r#"
        b = [1, 2, 3]
        b.insert(5, 8)
        p b
        p [1, 2].eql?([1, 2.0])
        p [1, 2].eql?([1, 2])
        p [1.0, 1].uniq
        a = [1, 2, 3]
        p(a <=> a)
        r = [1, 2]
        r.push(r)
        p(r <=> r)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2, 3, nil, nil, 8]\nfalse\ntrue\n[1.0, 1]\n0\n0\n",
    );
}

#[test]
fn marshal_dump_load_roundtrip_and_wire_format() {
    // Marshal round-trips the value tower (primitives, bignum, array, hash,
    // Rational, shared refs and cycles) and writes CRuby's exact wire bytes for
    // symbols (with ;-symlinks) and floats.
    let result = run_ruby(
        r#"
        def rt(x) = Marshal.load(Marshal.dump(x))
        p rt(42)
        p rt(-987654321)
        p rt(3.14)
        p rt("hi")
        p rt(:sym)
        p rt([1, "x", :y, nil, true])
        p rt(2 ** 200)
        p rt(Rational(3, 4))
        p [rt(Rational(1, 3)), rt(Rational(2, 5))]
        shared = [1, 2]
        sg = rt([shared, shared])
        sg[0] << 99
        p sg[1]
        cyc = [10]
        cyc << cyc
        dc = rt(cyc)
        p dc[1][1][0]
        puts Marshal.dump([:ab, :cd, :ab]).bytes.join(",")
        puts Marshal.dump(100.0).bytes.join(",")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "42\n-987654321\n3.14\n\"hi\"\n:sym\n[1, \"x\", :y, nil, true]\n\
         1606938044258990275541962092341162602522202993782792835301376\n\
         (3/4)\n[(1/3), (2/5)]\n[1, 2, 99]\n10\n\
         4,8,91,8,58,7,97,98,58,7,99,100,59,0\n4,8,102,8,49,101,50\n",
    );
}

#[test]
fn respond_to_sees_class_and_module_singleton_methods() {
    // respond_to? on a class/module value must see user `def self.x`,
    // module_function, and class<<self accessors, plus inherited builtin
    // Class methods -- but a module never responds to :new.
    let result = run_ruby(
        r#"
        class Foo
          def self.custom; end
        end
        module Bar
          def self.helper; end
          module_function
          def mf; end
        end
        module Acc
          class << self
            attr_accessor :x
          end
        end
        puts Foo.respond_to?(:new)
        puts Foo.respond_to?(:custom)
        puts Foo.respond_to?(:name)
        puts Foo.respond_to?(:nope_xyz)
        puts Bar.respond_to?(:new)
        puts Bar.respond_to?(:helper)
        puts Bar.respond_to?(:mf)
        puts Acc.respond_to?(:x)
        puts Acc.respond_to?(:x=)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\ntrue\ntrue\nfalse\nfalse\ntrue\ntrue\ntrue\ntrue\n"
    );
}

#[test]
fn respond_to_on_implicit_self_inside_a_class_method() {
    // Implicit-self respond_to? inside a `def self.x` resolves against the
    // class value (like explicit self.respond_to?), so a sibling class method
    // answers true, not false.
    let result = run_ruby(
        r#"
        class Screen
          def self.build
            puts respond_to?(:build)
            puts respond_to?(:name)
            puts respond_to?(:nope_xyz)
          end
          def self.other; end
        end
        Screen.build
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\nfalse\n");
}

#[test]
fn generic_object_freeze_clone_and_immutable_unfreeze() {
    // Object.new instances track frozen state (freeze/frozen?/clone-preserves);
    // clone(freeze: false) on an always-frozen immediate raises ArgumentError.
    let result = run_ruby(
        r#"
        o = Object.new
        p o.frozen?
        o.freeze
        p o.frozen?
        p o.clone.frozen?
        p o.dup.frozen?
        u = Object.new
        p u.clone(freeze: true).frozen?
        p u.clone(freeze: false).frozen?
        p((nil.clone(freeze: false) rescue $!.class))
        p((1.clone(freeze: false) rescue $!.class))
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "false\ntrue\ntrue\nfalse\ntrue\nfalse\nArgumentError\nArgumentError\n",
    );
}

#[test]
fn env_dup_clone_freeze_raise_type_error() {
    // ENV overrides Kernel#dup/#clone/#freeze to raise (copy it via ENV.to_h).
    // These must route through dispatch, not the universal value fast paths,
    // which would silently shallow-copy/freeze the singleton instead.
    let result = run_ruby(
        r#"
        ENV['ZZ_E2E'] = '1'
        p (ENV.dup rescue $!.message)
        p (ENV.clone rescue $!.message)
        p (ENV.freeze rescue $!.message)
        p ENV['ZZ_E2E']
        p ENV.store('ZZ_E2E2', '3')
        p ENV.delete('ZZ_E2E2')
        p ENV.to_s
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"Cannot dup ENV, use ENV.to_h to get a copy of ENV as a hash\"\n\
         \"Cannot clone ENV, use ENV.to_h to get a copy of ENV as a hash\"\n\
         \"cannot freeze ENV\"\n\
         \"1\"\n\"3\"\n\"3\"\n\"ENV\"\n"
    );
}

#[test]
fn class_frozen_false_hash_delete_block_and_default_record_separator() {
    // A user class (and a core class) reports frozen? == false. Hash#delete
    // calls its block when the key is absent. $/ defaults to "\n".
    let result = run_ruby(
        r#"
        class C001; end
        p C001.frozen?
        p Integer.frozen?
        d = { a: 1, b: 2 }
        p d.delete(:b)
        p d.delete(:z) { |k| "gone #{k}" }
        p $/
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "false\nfalse\n2\n\"gone z\"\n\"\\n\"\n");
}

#[test]
fn nil_object_id_range_cover_and_struct_not_equal() {
    // nil.object_id is 4 (CRuby 4.0.5). Range#cover? accepts a Range argument
    // (containment). Struct#!= negates the struct's value == (not identity).
    let result = run_ruby(
        r#"
        p nil.object_id
        p((1..5).cover?(2..4))
        p((1..5).cover?(0..4))
        p((1..5).cover?(2..6))
        S = Struct.new(:a, :b)
        x = S.new(5, 6)
        p(x == S.new(5, 6))
        p(x != S.new(5, 6))
        p(x != S.new(5, 9))
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "4\ntrue\nfalse\nfalse\ntrue\nfalse\ntrue\n");
}

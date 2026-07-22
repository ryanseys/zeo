use crate::support::{compile_project, run_ruby, run_ruby_packages};

#[test]
fn load_with_a_wrap_argument_is_a_clean_rejection() {
    // (`autoload` is now supported via the loader's eager splice -- see the
    // `autoload_*` tests above.) `load "file", wrap` still needs load-time
    // anonymous-module scoping this compiler lacks.
    let err = compile_project(
        &[("w.rb", "puts 1\n"), ("main.rb", "load \"./w.rb\", true\n")],
        "main.rb",
        &[],
    )
    .unwrap_err();
    assert!(err.contains("wrap"), "unexpected error: {err}");
}

// ---- Enumerable implemented in Rust (zeo_rt::enumerable) ----

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
    let result = run_ruby_packages(
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
        &[crate::REPO_PACKAGES],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "4\n3\n6\n16\n6\n106\ntrue\n6\n3.5\n1\n9\nc\n2\n1\n2\n2\n2\ntrue\nfalse\ntrue\nfalse\ntrue\n0:10\n1:20\n3\n2\ntrue\n4\n3\n3\n15\ntrue\n5\n15\n4\n3\n4\n6\ntrue\ntrue\ntrue\ntrue\nfalse\n"
    );
}

/// The hierarchy is live in `is_a?`/`kind_of?`/`instance_of?` -- statically
/// folded sites AND the runtime path through a Poly receiver, plus a user
/// class inheriting the full Object tail. Every line oracle-verified.
#[test]
fn is_a_walks_the_cruby_chains() {
    let result = run_ruby(
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
// Enumerable Tier A breadth (the enum.c architecture: every
// method drives the receiver's own #each). Oracle: ruby 4.0.5.
// ---------------------------------------------------------------------------

#[test]
fn enumerable_breadth_matches_the_oracle() {
    let result = run_ruby(
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
    assert_eq!(
        result.stdout,
        "[4, 16, 36]\n[2, 3, 4, 5]\n\"Enumerator::Lazy\"\n"
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
fn random_rejects_non_positive_bounds() {
    let result = run_ruby(
        r#"
        begin; Random.new(1).rand(0); rescue ArgumentError => e; puts e.message; end
        begin; Random.new(1).rand(-3); rescue ArgumentError => e; puts e.message; end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "invalid argument - 0\ninvalid argument - -3\n"
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
    assert_eq!(
        result.stdout,
        "{x: 1, b: 2}\n{x: 1, \"b\" => 2}\n{a: 1, y: 2}\n"
    );
}

#[test]
fn enumerator_produce_endless_generator() {
    // Enumerator.produce(initial) { |prev| ... } yields initial, then each
    // block result, forever -- bounded by the consumer. Without initial, the
    // first value is block.call(nil).
    let result = run_ruby(
        r#"
        p(Enumerator.produce(1) { |n| n * 2 }.take(3))
        p(Enumerator.produce(1) { |n| n + 1 }.first(4))
        g = Enumerator.produce(0) { |n| n + 2 }
        p g.next
        p g.next
        e = Enumerator.produce { |n| (n || 0) + 1 }
        p e.take(3)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[1, 2, 4]\n[1, 2, 3, 4]\n0\n2\n[1, 2, 3]\n",);
}

#[test]
fn for_iterates_any_object_answering_each() {
    // Ruby's `for` performs NO type dispatch: `for x in obj` compiles to
    // `obj.each { |x| ... }` (compile_iter, compile.c:8548). So a user class
    // iterates, a pair-yielding each destructures, the loop variable outlives
    // the loop (`for` introduces no scope), and an object with no `each`
    // fails at RUNTIME rather than failing the compile.
    let result = run_ruby(
        r##"
        class Nums
          include Enumerable
          def initialize(*xs) = @xs = xs
          def each; @xs.each { |x| yield x }; end
        end
        class Pairs
          include Enumerable
          def each; yield [1, :a]; yield [2, :b]; end
        end
        total = 0
        for x in Nums.new(1, 2, 3, 4)
          total += x
        end
        p total
        pairs = []
        for k, v in Pairs.new
          pairs << "#{k}:#{v}"
        end
        p pairs
        for survivor in [10, 20, 30]
        end
        p survivor
        begin
          for z in 5; end
        rescue NoMethodError => e
          puts e.message
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "10\n[\"1:a\", \"2:b\"]\n30\nundefined method 'each' for an instance of Integer\n",
    );
}

#[test]
fn enumerator_chain_product_and_lazy_size() {
    // Enumerator::Chain over held (not flattened) sources, Enumerator.product's
    // rightmost-fastest ordering, and Lazy#size folding ops without iterating.
    let result = run_ruby(
        r#"
        p(([1, 2].each + [3, 4].each).class)
        p(([1, 2].each + [3, 4].each).to_a)
        p([1, 2].chain([3], [4, 5]).to_a)
        p([1, 2].chain([3]).size)

        class Letters
          include Enumerable
          def initialize(*xs); @xs = xs; end
          def each(&blk); @xs.each(&blk); end
        end
        p([9].chain(Letters.new(7, 8)).to_a)
        p([1, 2].chain([3]).select { |x| x > 1 })

        p(Enumerator.product([1, 2], [3, 4]).class)
        p(Enumerator.product([1, 2], [3], [4, 5]).to_a)
        p(Enumerator.product([1, 2], [3, 4]).size)

        p([1, 2, 3].lazy.map { |x| x * 2 }.size)
        p([1, 2, 3].lazy.select { |x| x > 1 }.size)
        p([1, 2, 3, 4, 5].lazy.drop(1).take(2).size)
        p((1..Float::INFINITY).lazy.size)
        p((1..Float::INFINITY).lazy.take(3).size)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Enumerator::Chain\n[1, 2, 3, 4]\n[1, 2, 3, 4, 5]\n3\n[9, 7, 8]\n[2, 3]\n\
         Enumerator::Product\n[[1, 3, 4], [1, 3, 5], [2, 3, 4], [2, 3, 5]]\n4\n\
         3\nnil\n2\nInfinity\n3\n"
    );
}

#[test]
fn break_with_a_value_makes_the_iterator_call_return_it() {
    // `break <v>` in a block makes the whole Enumerable call evaluate to <v>
    // (CRuby TAG_BREAK), not the partial accumulator -- across value-producing
    // (map/select/reject/reduce/count/find) and self-returning
    // (each_with_index) iterators. Bare break -> nil.
    let result = run_ruby(
        r#"
        a = [1, 2, 3]
        p a.map { |x| break 99 if x == 2; x * 10 }
        p a.select { |x| break :s if x == 2; x.odd? }
        p a.reject { |x| break 7 if x == 2; false }
        p a.reduce(0) { |s, x| break 100 if x == 2; s + x }
        p a.count { |x| break 5 if x == 2; true }
        p a.find { |x| break(-1) if x == 2; false }
        p a.each_with_index { |x, i| break i if x == 2 }
        p a.map { |x| break if x == 2; x }
        p a.map { |x| x + 1 }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "99\n:s\n7\n100\n5\n-1\n1\nnil\n[2, 3, 4]\n");
}

#[test]
fn each_slice_and_frozen_hash_filter_error_messages() {
    // each_slice(0) says "invalid slice size", each_cons(0) just "invalid
    // size"; an in-place Hash filter on a frozen receiver raises FrozenError.
    let result = run_ruby(
        r#"
        begin; [1, 2, 3].each_slice(0).to_a; rescue ArgumentError => e; puts e.message; end
        begin; [1, 2, 3].each_cons(0).to_a; rescue ArgumentError => e; puts e.message; end
        begin; {a: 1}.freeze.reject! { |k, v| true }; rescue => e; puts e.class; end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "invalid slice size\ninvalid size\nFrozenError\n"
    );
}

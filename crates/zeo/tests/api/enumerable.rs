use crate::support::run_ruby_packages;

// ---- Enumerable implemented in Rust (zeo_rt::enumerable) ----

#[test]
fn rust_enumerable_matches_real_ruby_across_all_receiver_kinds() {
    // One oracle-verified sweep (real ruby 4.0.6, byte-for-byte) covering
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
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "4\n3\n6\n16\n6\n106\ntrue\n6\n3.5\n1\n9\nc\n2\n1\n2\n2\n2\ntrue\nfalse\ntrue\nfalse\ntrue\n0:10\n1:20\n3\n2\ntrue\n4\n3\n3\n15\ntrue\n5\n15\n4\n3\n4\n6\ntrue\ntrue\ntrue\ntrue\nfalse\n"
    );
}

// ---------------------------------------------------------------------------
// Enumerable Tier A breadth (the enum.c architecture: every
// method drives the receiver's own #each). Oracle: ruby 4.0.6.
// ---------------------------------------------------------------------------

use crate::support::{run_ruby};

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

#[test]
fn small_api_matchdata_size_set_join_complex_i_method_source_location() {
    // Batch 3 small wins: MatchData#size/#length, Set#join, the Complex::I
    // imaginary-unit constant (and that it multiplies to -1), and Method's
    // source_location/super_method (nil for a builtin, matching CRuby's C-method).
    let result = run_ruby(
        r#"
        require "set"
        m = "2026-06".match(/(\d+)-(\d+)/)
        p m.size
        p m.length
        p Set[1, 2, 3].join("-")
        p Complex::I
        p(Complex::I * Complex::I)
        p 5.method(:+).source_location
        p 5.method(:+).super_method
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "3\n3\n\"1-2-3\"\n(0+1i)\n(-1+0i)\nnil\nnil\n"
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
    let result = run_ruby(
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
    let result = run_ruby(
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
        // MatchData indexes like an array `[full, g1, g2, g3]`, so `md[1, 2]`
        // is `[g1, g2]` and `md[1..]` is `[g1, g2, g3]` (verified against
        // ruby 4.0.5 -- the previous expectation dropped the first capture).
        "0.5\n5.0e-07\n[\"2024\", \"01\"]\n[\"2024\", \"01\", \"31\"]\ntrue\n5.0e-07\n1.0e+20\n"
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
fn numeric_long_tail_ranges_bignum_iteration_and_hex_float() {
    let result = run_ruby(
        r##"
        # Symbol range iterates by name succession.
        p (:a..:e).to_a
        # Float range: O(1) min/max, drift-free step, float bsearch, and each
        # raises (can't iterate a float range).
        p (1.0..3.0).min
        p (1.0..3.0).max
        p (1.0..3.0).step(0.5).to_a
        p (0.0..10.0).bsearch { |x| x >= 3.5 }
        p(begin; (1.0..3.0).to_a; rescue => e; e.message; end)
        # Negative integer step walks a descending range.
        p (10..2).step(-2).to_a
        # downto/upto beyond i64 iterate as BigInt, and .size is exact.
        big = 2 ** 100
        p big.downto(big - 2).to_a
        p big.downto(big - 2).size
        # downto with a Float limit yields integers.
        p 5.downto(2.0).to_a
        # C99 hex-float strings.
        p Float("0x1p4")
        p Float("0xa")
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[:a, :b, :c, :d, :e]\n1.0\n3.0\n[1.0, 1.5, 2.0, 2.5, 3.0]\n3.5\n\"can't iterate from Float\"\n[10, 8, 6, 4, 2]\n[1267650600228229401496703205376, 1267650600228229401496703205375, 1267650600228229401496703205374]\n3\n[5, 4, 3, 2]\n16.0\n10.0\n",
    );
}

#[test]
fn find_ifnone_tally_hash_and_min_max_by_count() {
    let result = run_ruby(
        r#"
        p [1, 2, 3].find(-> { -1 }) { |x| x > 10 }
        p [1, 20, 3].find(-> { -1 }) { |x| x > 10 }
        p [1, nil, 3].find(-> { :fallback }) { |x| x.nil? }
        h = Hash.new(0)
        p [1, 1, 2, 3, 3, 3].tally(h)
        p h
        p [5, 5, 6].tally({ 5 => 10 })
        p %w[bbbb a ccc dd].max_by(2, &:length)
        p %w[bbbb a ccc dd].min_by(2, &:length)
        p [1, 2, 3].min_by(0) { |n| n }
        begin
          [1, 2, 3].max_by(-1) { |n| n }
        rescue ArgumentError => e
          p e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "-1\n20\nnil\n{1 => 2, 2 => 1, 3 => 3}\n{1 => 2, 2 => 1, 3 => 3}\n{5 => 12, 6 => 1}\n[\"bbbb\", \"ccc\"]\n[\"a\", \"dd\"]\n[]\n\"negative size (-1)\"\n",
    );
}

/// `[]`-style compound assignment with MORE than one index. `[]`/`[]=` are
/// ordinary methods, so `a[i, j] += rhs` is a two-argument `[]` paired with a
/// three-argument `[]=` -- Array's `(start, length)` splice form. Each index
/// binds to its own hidden local so a side-effecting index runs exactly once
/// across the read and the write, the same guarantee the receiver already had.
#[test]
fn compound_assignment_through_a_multi_argument_index() {
    let result = run_ruby(
        r#"
        a = [1, 2, 3, 4]
        a[1, 2] += ["x"]
        p a

        b = [1, 2, 3]
        b[0, 2] ||= 9
        p b

        $i = 0
        $j = 0
        def idx; $i += 1; 0; end
        def len; $j += 1; 1; end
        c = [1, 2, 3]
        c[idx, len] = [9]
        p c
        p [$i, $j]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2, 3, \"x\", 4]\n[1, 2, 3]\n[9, 2, 3]\n[1, 1]\n"
    );
}

/// Array error messages that CRuby spells precisely: a too-small negative index
/// to `[]=` carries `; minimum: -N` (the number, so an empty array reads
/// `minimum: 0`, not `-0`), and `drop`/`take` name the method that was actually
/// called rather than always saying "take".
#[test]
fn array_index_and_size_error_messages_match_cruby() {
    let result = run_ruby(
        r##"
        def msg
          yield
        rescue => e
          puts "#{e.class}: #{e.message}"
        end

        msg { [1, 2, 3][-999] = 9 }
        msg { [][-1] = 9 }
        msg { [1, 2].drop(-1) }
        msg { [1, 2].take(-1) }
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "IndexError: index -999 too small for array; minimum: -3\n\
         IndexError: index -1 too small for array; minimum: 0\n\
         ArgumentError: attempt to drop negative size\n\
         ArgumentError: attempt to take negative size\n"
    );
}

#[test]
fn symbol_to_proc_is_lambda_and_hash_values_at_uses_default() {
    // :name.to_proc.lambda? is true (CRuby). Hash#values_at routes each key
    // through [], so a missing key yields the hash's default, not bare nil.
    let result = run_ruby(
        r#"
        p :upcase.to_proc.lambda?
        p ["x", "y"].map(&:upcase)
        p Hash.new(0).values_at(:x, :y)
        p({ a: 1 }.values_at(:a, :z))
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\n[\"X\", \"Y\"]\n[0, 0]\n[1, nil]\n");
}

#[test]
fn dig_raises_typeerror_through_non_diggable_intermediate() {
    // Array/Hash#dig recurse through each intermediate's OWN #dig (CRuby's
    // rb_obj_dig), so digging past a non-diggable (an Integer) raises
    // TypeError rather than silently indexing its bits via Integer#[].
    let result = run_ruby(
        r#"
        def cls; begin; yield; rescue => e; e.class; end; end
        p(cls { [1, [2]].dig(1, 0, 3) })
        p [1, [2]].dig(1, 0)
        p [1, [2, [3]]].dig(1, 1, 0)
        p [[nil]].dig(0, 0, 5)
        p [{ a: 7 }].dig(0, :a)
        p({ a: { b: 1 } }.dig(:a, :b))
        p(cls { { a: 5 }.dig(:a, :b) })
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "TypeError\n2\n3\nnil\n7\n1\nTypeError\n"
    );
}

#[test]
fn uniq_uses_eql_identity_for_user_class_defining_only_eq() {
    // uniq (and Array#eql?) dedup by eql?/hash, not ==. A class that defines
    // only == keeps every instance distinct (Object#eql? is identity), so
    // two equal-by-== Points both survive uniq -- unlike include?/index,
    // which do use ==.
    let result = run_ruby(
        r#"
        class Point
          attr_reader :x
          def initialize(x) = @x = x
          def ==(o) = o.is_a?(Point) && @x == o.x
        end
        pts = [Point.new(1), Point.new(2), Point.new(3)]
        p pts.include?(Point.new(2))
        p pts.index(Point.new(3))
        p [Point.new(1), Point.new(1), Point.new(2)].uniq.map(&:x)
        p [1.0, 1, 1, 2].uniq
        p [1].eql?([1])
        p [1].eql?([1.0])
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\n2\n[1, 1, 2]\n[1.0, 1, 2]\ntrue\nfalse\n"
    );
}

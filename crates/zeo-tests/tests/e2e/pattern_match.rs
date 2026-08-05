use crate::support::run_ruby;

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
fn case_in_range_pattern_covers_float_poly_and_open_bounds() {
    // A range pattern is Range#=== (rb_case_eq/range_covers), so a Float or
    // Poly scrutinee, an exclusive bound, and a beginless/endless range all
    // work -- an as_int_unchecked path would panic on non-Int scrutinees.
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
fn case_in_capture_of_a_bare_bind_can_write_the_same_subject_to_two_names() {
    // A real latent hazard this test guards against: `Capture(Bind(x), y)`
    // writes the SAME scrutinee into two different locals -- if either write
    // MOVED instead of CLONED the scrutinee, the second write would fail to
    // compile ("use of moved value"). See `emit_pattern_match`'s docs.
    let result = run_ruby("case 5\nin x => y\n  puts x\n  puts y\nend\n");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\n5\n");
}

#[test]
fn time_asctime_to_a_to_r_round_xmlschema_deconstruct() {
    let result = run_ruby(
        r#"
        t = Time.at(1_700_000_000.5).utc
        puts t.asctime
        p t.to_a
        p Time.at(100).to_r
        p Time.at(1_700_000_000.7654321).round(3).subsec
        puts t.xmlschema(3)
        p t.deconstruct_keys([:year, :month])
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Tue Nov 14 22:13:20 2023\n[20, 13, 22, 14, 11, 2023, 2, 318, false, \"UTC\"]\n\
         (100/1)\n(153/200)\n2023-11-14T22:13:20.500Z\n{year: 2023, month: 11}\n"
    );
}

#[test]
fn struct_values_values_at_dig_and_filtered_deconstruct_keys() {
    let result = run_ruby(
        r#"
        Point = Struct.new(:x, :y)
        p Point.new(3, 4).values
        p Point.new(3, 4).values_at(0, -1)
        Config = Struct.new(:name, :opts)
        c = Config.new("web", { port: 8080 })
        p c.dig(:opts, :port)
        p c.dig(:opts, :missing)
        S = Struct.new(:a, :b, :c)
        p S.new(1, 2, 3).deconstruct_keys([:a, :c])
        p S.new(1, 2, 3).deconstruct_keys([:z, :a])
        p S.new(1, 2, 3).deconstruct_keys([:a, :b, :c, :d])
        D = Data.define(:a, :b)
        p D.new(a: 1, b: 2).deconstruct_keys([:a])
        p D.new(a: 1, b: 2).deconstruct_keys(nil)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[3, 4]\n[3, 4]\n8080\nnil\n{a: 1, c: 3}\n{}\n{}\n{a: 1}\n{a: 1, b: 2}\n"
    );
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

/// SEMANTICS FLIP: String patterns to split/gsub used to be
/// compile-time rejections ("pass a Regexp literal instead"); the String
/// table rows now implement them for real, so the static regexp path falls
/// through to dynamic dispatch instead. Oracle-verified.
#[test]
fn string_patterns_to_split_and_gsub_now_work() {
    let result = run_ruby(
        r#"
        puts "a,b".split(",").inspect
        puts "a,b".gsub(",", ";")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[\"a\", \"b\"]\na;b\n");
}

#[test]
fn matchdata_offsets_match_and_deconstruction() {
    // MatchData begin/end, bytebegin/byteend, match, inspect, deconstruct, and
    // deconstruct_keys with CRuby's key rules.
    let result = run_ruby(
        r#"
        m = "hello world".match(/(\w+)(\s+)(\w+)/)
        p [m.begin(1), m.end(1), m.begin(3)]
        md = "a1b2".match(/(\d)(\w)/); p [md.bytebegin(1), md.byteend(1)]
        m2 = "abc".match(/(a)(b)(c)/)
        p m2.inspect
        p m2.deconstruct
        p m2.match(2)
        n = "abc".match(/(?<x>b)(?<y>c)/)
        p [n.deconstruct_keys([:x]), n.deconstruct_keys([:y, :x]), n.deconstruct_keys(nil), n.deconstruct_keys([:x, :y, :z])]
        p n.named_captures(symbolize_names: true)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[0, 5, 6]\n[1, 2]\n\"#<MatchData \\\"abc\\\" 1:\\\"a\\\" 2:\\\"b\\\" 3:\\\"c\\\">\"\n\
         [\"a\", \"b\", \"c\"]\n\"b\"\n[{x: \"b\"}, {y: \"c\", x: \"b\"}, {x: \"b\", y: \"c\"}, {}]\n\
         {x: \"b\", y: \"c\"}\n"
    );
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
    assert_eq!(
        result.stdout,
        "true\nfalse\ntrue\ntrue\nfalse\ntrue\ntrue\n"
    );
}

#[test]
fn array_pattern_trailing_comma_is_an_implicit_rest() {
    let result = run_ruby(
        r#"
        case [0, 1, 2, 3]
        in [0, 1, ] then puts "matched"
        else puts "no"
        end
        case [5]
        in [0, 1, ] then puts "wrong"
        else puts "too short"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "matched\ntoo short\n");
}

use crate::support::{run_ruby};

#[test]
fn hello_prints_a_symbol() {
    let result = run_ruby("puts :ok");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "ok\n");
    assert_eq!(result.stderr, "");
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

// --- Phase 12.7: Regexp -----------------------------------------------
//
// Backed by the `regex` crate, not Ruby's own Onigmo engine -- see
// `hir::HirNode::RegexpLit`'s docs for the documented semantic gap (no
// backreferences/lookaround INSIDE a pattern) and `zeo_rt::regexp`'s
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
fn split_with_no_match_returns_the_whole_string_unsplit() {
    // The empty-haystack-returns-an-empty-array case is exercised directly
    // at the `zeo_rt::regexp_split` unit-test level instead (see
    // `zeo-rt/src/regexp.rs`) -- `puts`-ing it here would conflate this
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

// ---------------------------------------------------------------------------
// Phase 17.1-D -- String + Symbol Tier A breadth. Oracle: ruby 4.0.5.
// ---------------------------------------------------------------------------

/// The String Tier A surface: case/strip families, split shapes, chomp,
/// indexing forms, sub/gsub (String + block), tr/delete/squeeze/count,
/// lenient conversions, succ carry, padding, and `%` formatting.
#[test]
fn string_breadth_matches_the_oracle() {
    let result = run_ruby(
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
fn rational_and_complex_parse_string_arguments() {
    let result = run_ruby(
        r##"
        p Rational("3/4")
        p Rational("  -5/2 ")
        p Rational("6")
        p Rational("2.5")
        p Rational(3)
        p Complex("2+3i")
        p Complex("1+2i")
        p Complex("3")
        p Complex("-i")
        p Complex("4i")
        def t; yield; rescue => e; "#{e.class}"; end
        p t { Rational("abc") }
        p t { Complex("xyz") }
        p t { Rational(1, 0) }
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "(3/4)\n(-5/2)\n(6/1)\n(5/2)\n(3/1)\n(2+3i)\n(1+2i)\n(3+0i)\n(0-1i)\n(0+4i)\n\"ArgumentError\"\n\"ArgumentError\"\n\"ZeroDivisionError\"\n",
    );
}

#[test]
fn string_to_i_base_zero_auto_detects_the_prefix() {
    let result = run_ruby(
        r#"
        puts "0xff".to_i(0)
        puts "0b101".to_i(0)
        puts "0o755".to_i(0)
        puts "0777".to_i(0)
        puts "42".to_i(0)
        puts "-0xff".to_i(0)
        puts "+0b101".to_i(0)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "255\n5\n493\n511\n42\n-255\n5\n");
}

#[test]
fn last_paren_backreference_and_adjacent_interpolation() {
    // `$+` is the highest-numbered group that PARTICIPATED, skipping
    // declared-but-unmatched ones (rb_reg_match_last, re.c:2093), and is nil
    // when only group 0 matched -- it never reports the whole match.
    //
    // Backslash-continued adjacent literals parse as an InterpolatedStringNode
    // whose own parts are InterpolatedStringNodes, so parts must flatten
    // recursively rather than being treated as leaves.
    let result = run_ruby(
        r##"
        "abc123" =~ /([a-z]+)(\d+)/
        puts $1
        puts $2
        puts $+
        "abc" =~ /([a-z]+)(\d+)?/
        puts $+
        "xyz" =~ /xyz/
        p $+
        "b" =~ /(a)|(b)|(c)/
        p $+
        def svg(px, inner)
          "<a width='#{px}' " \
          "height='#{px}'>#{inner}</a>"
        end
        def tail(n)
          "n=#{n}" \
          " done"
        end
        puts svg(16, "x")
        puts tail(7)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "abc\n123\n123\nabc\nnil\n\"b\"\n<a width='16' height='16'>x</a>\nn=7 done\n",
    );
}

#[test]
fn tr_squeeze_negation_split_block_and_chomp_paragraph() {
    // `^`-negated tr/squeeze sets, split's block form (yields fields, returns
    // the receiver), and chomp("") paragraph mode.
    let result = run_ruby(
        r#"
        p "abc".tr("^a", "x")
        p "hello".tr("^aeiou", ".")
        p "aaabbbccc".squeeze("^a")
        p "aaa^^^bbb".squeeze("^")
        r = []
        "a,b,c".split(",") { |p| r << p.upcase }
        p r
        acc = 0
        ret = "aa-bbb-c".split("-") { |p| acc += p.length }
        p acc
        p ret.equal?("aa-bbb-c".dup) == false && ret == "aa-bbb-c"
        p "hello\r\n\r\n".chomp("")
        p "hello\r".chomp("")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"axx\"\n\".e..o\"\n\"aaabc\"\n\"aaa^bbb\"\n\
         [\"A\", \"B\", \"C\"]\n6\ntrue\n\"hello\"\n\"hello\\r\"\n",
    );
}

#[test]
fn succ_kind_flip_carry_and_match_block() {
    // succ carries across a separator only when the alnum kind is preserved
    // ("1.9"->"2.0") and otherwise inserts ("a-9"->"a-10"); String#match with a
    // block yields the MatchData and answers the block value (nil on a miss).
    let result = run_ruby(
        r#"
        p "1.9".succ
        p "a-9".succ
        p "zz9".succ
        p "Az9".succ
        p "foobar".match(/(o+)/) { |m| m[1].upcase }
        p "foobar".match(/xyz/) { |m| m[1] }
        p("count=42".match(/(\d+)/) { |m| m[1].to_i * 2 })
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"2.0\"\n\"a-10\"\n\"aaa0\"\n\"Ba0\"\n\"OO\"\nnil\n84\n",
    );
}

#[test]
fn awk_split_limits_and_gsub_block_backref() {
    // awk-mode split honors limit (1 = whole, cap = verbatim remainder, nonzero
    // keeps a trailing empty); $1 is live inside a gsub/sub block.
    let result = run_ruby(
        r#"
        s = " one two  three "
        sep = " "
        p s.split(sep, 1)
        p s.split(sep, 2)
        p s.split(sep, -1)
        p "   ".split(sep, -1)
        p "trail   ".split(sep, 3)
        p "a1b2c3".gsub(/(\d)/) { ($1.to_i * 2).to_s }
        p "x9".sub(/(\d)/) { ($1.to_i + 1).to_s }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[\" one two  three \"]\n\
         [\"one\", \"two  three \"]\n\
         [\"one\", \"two\", \"three\", \"\"]\n\
         [\"\"]\n\
         [\"trail\", \"\"]\n\
         \"a2b4c6\"\n\
         \"x10\"\n",
    );
}

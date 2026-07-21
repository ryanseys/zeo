use crate::support::{run_ruby};

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

/// The Numeric/Integer/Float Tier A breadth: divmod matrices, the rounding
/// families, gcd/lcm/digits/chr/ord, predicates, step/times/upto/downto,
/// exact Float#to_r, and bignum-capable Float#to_i.
#[test]
fn numeric_breadth_matches_the_oracle() {
    let result = run_ruby(
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

// ---------------------------------------------------------------------------
// Phase 17.1-G -- Kernel breadth: the multi-arg print family, the sprintf
// engine, rand/srand (property-asserted: our PRNG is deliberately not
// MT19937), catch/throw, and the user-def-wins interception order fix.
// Oracle: ruby 4.0.5.
// ---------------------------------------------------------------------------

#[test]
fn kernel_breadth_matches_the_oracle() {
    let result = run_ruby(
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
fn matchdata_unknown_named_group_raises_index_error() {
    // An unknown named-group key raises IndexError (previously a Rust panic);
    // a known name resolves its capture.
    let result = run_ruby(
        r##"
        m = "2026-06".match(/(?<y>\d+)-(?<mo>\d+)/)
        p m[:mo]
        begin
          m[:nope]
        rescue IndexError => e
          puts "#{e.class}: #{e.message}"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"06\"\nIndexError: undefined group name reference: nope\n"
    );
}

#[test]
fn regexp_octal_escape_is_not_a_backreference() {
    // A Ruby octal escape `\033` is the ESC byte, not a group-0 backreference;
    // it must compile and match rather than raising "Invalid back reference".
    let result = run_ruby(
        r#"
        s = "a\e[31mred\e[0mb"
        puts s.gsub(/\033\[[0-9;]*[A-Za-z]/, "")
        puts ("x\033y" =~ /\033/).inspect
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "aredb\n1\n");
}

#[test]
fn scan_with_a_block_yields_matches_and_returns_the_receiver() {
    // The block form yields each match (a String, or an Array of groups) and
    // returns the RECEIVER, not the array of matches.
    let result = run_ruby(
        r##"
        out = []
        r = "hello world".scan(/\w+/) { |w| out << w.upcase }
        p out
        p r
        pairs = []
        "a1b2".scan(/([a-z])(\d)/) { |l, d| pairs << [l, d] }
        p pairs
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[\"HELLO\", \"WORLD\"]\n\"hello world\"\n[[\"a\", \"1\"], [\"b\", \"2\"]]\n"
    );
}

#[test]
fn regexp_introspection_linear_time_and_class_backrefs() {
    // linear_time? is false only for a real (non-class) backreference. A
    // `\1`/`\k` INSIDE a character class is octal/literal, not a backref, and
    // must compile; a forward backref (`/[\]]\1(a)/`) constructs (never matches).
    let result = run_ruby(
        r##"
        p Regexp.linear_time?(/abc/)
        p Regexp.linear_time?(/(a)\1/)
        p Regexp.linear_time?(/[\1]/)
        p Regexp.linear_time?(/[a-z\k<x>]/)
        p Regexp.linear_time?(/[\1](a)\1/)
        p Regexp.linear_time?(/[\]]\1(a)/)
        p(/[\1]/.match?("\x01"))
        p(/[\]]\1(a)/.match("]a").nil?)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nfalse\ntrue\ntrue\nfalse\nfalse\ntrue\ntrue\n"
    );
}

#[test]
fn class_of_a_failed_match_is_nilclass_not_matchdata() {
    // str.match(re) types as MatchData but returns nil on no match, so .class
    // must be read at runtime rather than constant-folded to MatchData.
    let result = run_ruby(
        r#"
        p "hi".match(/h/).class
        p "hi".match(/z/).class
        m = "hi".match(/z/)
        p m.class
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "MatchData\nNilClass\nNilClass\n");
}

#[test]
fn split_named_backref_rindex_and_match_position() {
    // Empty-pattern split drops the leading zero-width field; \k<name> expands
    // in a replacement (unknown name -> IndexError); rindex/rpartition find the
    // rightmost anchored match start; match? honors a start position.
    let result = run_ruby(
        r#"
        p "abc".split(//)
        p "abc".split(//, -1)
        puts "foobar".gsub(/(?<x>o+)/, "[\\k<x>]")
        r = (begin; "x".gsub(/(?<a>x)/, "\\k<y>"); rescue => e; e.class; end)
        p r
        p "hello123world".rindex(/\d+/)
        p "hello123world".rpartition(/\d+/)
        p(/hello/.match?("hello world", 6))
        p(/world/.match?("hello world", 6))
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[\"a\", \"b\", \"c\"]\n\
         [\"a\", \"b\", \"c\", \"\"]\n\
         f[oo]bar\n\
         IndexError\n\
         7\n\
         [\"hello12\", \"3\", \"world\"]\n\
         false\ntrue\n",
    );
}

#[test]
fn named_captures_dup_names_backref_plus_and_nonstring_match() {
    // named_captures collects ALL indices for a name reused by several groups
    // (the engine collapses it, so this parses the source); \+ expands to the
    // highest participating group; Regexp#=~ against a non-String raises
    // TypeError (nil still answers nil).
    let result = run_ruby(
        r##"
        p /(?<a>x)(?<b>y)/.named_captures
        p /(?<a>x)(?<a>z)/.named_captures
        p /(?<a>x)(?<a>z)/.names
        p "ab".sub(/(a)(b)?/, '\+')
        p "a.".sub(/(a)(b)?/, '\+')
        p "1a 2. 3c".gsub(/(\d)([a-z])?/, '<\+>')
        r = (begin; /p/ =~ 5; rescue TypeError => e; "TE: " + e.message; end); p r
        p(/l/ =~ "hello")
        p(/z/ =~ "hello")
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "{\"a\" => [1], \"b\" => [2]}\n\
         {\"a\" => [1, 2]}\n\
         [\"a\"]\n\
         \"b\"\n\"a.\"\n\"<a> <2>. <c>\"\n\
         \"TE: no implicit conversion of Integer into String\"\n2\nnil\n"
    );
}

#[test]
fn posix_bracket_validation_and_regexp_error_messages() {
    // An unknown POSIX class name is a RegexpError (invalid POSIX bracket type),
    // while valid ones compile; an unterminated char class reports CRuby's
    // message shape, not the engine's raw multiline parse error.
    let result = run_ruby(
        r##"
        p "Hi 12".scan(/[[:alpha:]]+/)
        r = (begin; Regexp.new("[[:bogus:]]"); "ok"; rescue RegexpError => e; e.message; end); p r
        r2 = (begin; Regexp.new("[invalid"); "ok"; rescue RegexpError => e; e.message; end); p r2
        p Regexp.new("hello").match?("say hello")
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[\"Hi\"]\n\
         \"invalid POSIX bracket type: /[[:bogus:]]/\"\n\
         \"unterminated character class: /[invalid/\"\n\
         true\n"
    );
}

use crate::support::{run_ruby};

#[test]
fn arith_adds_integer_literals() {
    let result = run_ruby("puts 1 + 1");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\n");
}

#[test]
fn float_literals_and_arithmetic() {
    let result = run_ruby(
        r#"
        puts 1.5 + 2.5
        puts 3.0 - 1
        puts 2.0 * 3
        puts 7.0 / 2
        puts 7.5 % 2
        puts 2.0 ** 3
        puts 1.0 == 1
        puts 1.5 < 2.5
        puts(1.5 <=> 2.5)
        puts(-1.5)
        puts 1.0
        puts 100.0
        puts 3.14
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "4.0\n2.0\n6.0\n3.5\n1.5\n8.0\ntrue\ntrue\n-1\n-1.5\n1.0\n100.0\n3.14\n"
    );
}

#[test]
fn float_and_int_mixed_arithmetic_promotes_to_float() {
    let result = run_ruby(
        r#"
        class Adder
          def add(a, b)
            a + b
          end
        end
        puts Adder.new.add(1, 2.5)
        puts Adder.new.add(2.5, 1)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3.5\n3.5\n");
}

#[test]
fn math_hyperbolic_gamma_and_frexp_family() {
    // Backed by the system libm CRuby also calls, so values match exactly;
    // gamma uses an exact factorial table + ±0/±inf/negative-integer rules.
    let result = run_ruby(
        r#"
        puts Math.tanh(1.0).round(10)
        puts Math.acosh(2.0).round(10)
        p Math.atanh(1.0)
        puts Math.gamma(6.0)
        p Math.gamma(0.0)
        p Math.lgamma(-1.0)
        p Math.frexp(8.0)
        puts Math.ldexp(0.75, 3)
        begin; Math.gamma(-2.0); rescue Math::DomainError => e; puts e.message; end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "0.761594156\n1.3169578969\nInfinity\n120.0\nInfinity\n[Infinity, 1]\n[0.5, 4]\n6.0\n\
         Numerical argument is out of domain - gamma\n"
    );
}

#[test]
fn regex_backreferences_and_lookaround_via_fancy_engine() {
    // Constructs the linear-time `regex` crate can't do; a backtracking engine
    // transparently backs them. The fast path stays unaffected.
    let result = run_ruby(
        r#"
        puts("hello" =~ /(\w)\1/)             # 2
        p("abc" =~ /(\w)\1/)                  # nil
        puts "foobar".match?(/foo(?=bar)/)    # true
        puts "foobaz".match?(/foo(?=bar)/)    # false
        puts "$100".gsub(/(?<=\$)\d+/, "N")   # $N
        puts "catfish".match?(/cat(?!fish)/)  # false
        puts "abc".match?(/a(?#c)bc/)         # true
        puts "book".match?(/(?<c>o)\k<c>/)    # true
        # fast path unaffected
        m = "2024-01-15".match(/(\d+)-(\d+)-(\d+)/)
        puts m[2]                             # 01
        p "a,b,c".split(/,/)                  # ["a", "b", "c"]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "2\nnil\ntrue\nfalse\n$N\nfalse\ntrue\ntrue\n01\n[\"a\", \"b\", \"c\"]\n"
    );
}

#[test]
fn bitwise_operators_on_integers() {
    let result = run_ruby(
        r#"
        a = 12
        b = 10
        puts a & b
        puts a | b
        puts a ^ b
        puts a << 2
        puts a >> 2
        puts ~a
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "8\n14\n6\n48\n3\n-13\n");
}

#[test]
fn numeric_and_time_fractional_and_rounding_coercions() {
    // Correctness fixes: Time.utc/local accept a fractional-microsecond 7th
    // argument (Float/Rational) and keep the sub-microsecond nanoseconds;
    // Rational#round honors the `half:` keyword (:up/:even/:down); Float
    // round/truncate with an extreme ndigits stays finite instead of NaN; and
    // Enumerator.new(callable).size invokes the callable lazily. Byte-verified
    // against ruby 4.0.5.
    let result = run_ruby(
        r#"
        p Time.utc(2001, 2, 3, 4, 5, 6, 500.5).nsec
        p Time.utc(2020, 1, 1, 0, 0, 0, Rational(1, 2)).nsec
        p Rational(5, 2).round(half: :even)
        p Rational(5, 2).round(half: :down)
        p Rational(5, 2).round(half: :up)
        p Rational(-5, 2).round(half: :even)
        p 1.23.round(400)
        p 1.23.round(-400)
        p 2.5.truncate(1000)
        p Enumerator.new(lambda { 42 }) { |y| y << 1 }.size
        p Enumerator.new(5) { |y| y << 1 }.size
        p Enumerator.new { |y| y << 1 }.size
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "500500\n500\n2\n2\n3\n-2\n1.23\n0\n2.5\n42\n5\nnil\n"
    );
}

// ---------------------------------------------------------------------------
// Phase 17.1-C -- the numeric tower: full-bignum Integer, Rational, Complex,
// the coercion matrix, literals, Math, Kernel conversions, Float/Math
// constants. Oracle: ruby 4.0.5, byte-identical.
// ---------------------------------------------------------------------------

/// Bignum end-to-end: overflow promotion + demotion round trips, big
/// literals (decimal/hex/binary/underscored), interpolation, hash keys,
/// Enumerable, is_a?, and the i64::MIN / -1 overflow edge.
#[test]
fn bignum_integers_promote_demote_and_interoperate() {
    let result = run_ruby(
        r##"
        r = 1
        i = 2
        while i <= 25
          r = r * i
          i += 1
        end
        puts r
        puts 2 ** 100
        puts 100000000000000000000 + 1
        puts 0xff
        puts 0b1010
        puts 1_000_000
        big = 9_223_372_036_854_775_807
        puts big + 1
        puts big + 1 - 1
        puts (big + 1) > big
        puts (big + 1).class
        puts "v=#{2 ** 70}"
        h = { 2 ** 70 => :big }
        puts h[2 ** 70]
        puts [2 ** 70, 1, 2 ** 65].min
        puts (2 ** 70).is_a?(Numeric)
        puts 2 ** 70 == 2 ** 70
        puts(-9223372036854775808 / -1)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "15511210043330985984000000\n\
         1267650600228229401496703205376\n\
         100000000000000000001\n\
         255\n10\n1000000\n\
         9223372036854775808\n\
         9223372036854775807\n\
         true\n\
         Integer\n\
         v=1180591620717411303424\n\
         big\n1\ntrue\ntrue\n\
         9223372036854775808\n"
    );
}

/// Rational: literals, reduction, exact arithmetic across the Int lane,
/// `2 ** -2`, `quo`, Float promotion, rounding family, and the
/// ZeroDivisionError channel.
#[test]
fn rationals_are_exact_and_oracle_faithful() {
    let result = run_ruby(
        r##"
        r = 3r
        puts r.class
        p r
        puts r
        p 1.5r
        p Rational(4, 8)
        p Rational(1, 2) + Rational(1, 3)
        p Rational(1, 2) * 3
        p Rational(1, 2) / Rational(3, 4)
        p Rational(3, 4) ** 2
        p 2 ** -2
        p 1.quo(3)
        p Rational(1, 2) + 0.5
        puts Rational(1, 2) < Rational(2, 3)
        puts Rational(1, 2) == 0.5
        puts Rational(1, 3).to_f
        puts Rational(7, 2).to_i
        p Rational(-7, 2).floor
        p Rational(-7, 2).ceil
        p Rational(7, 2).round
        p Rational(3, 4).numerator
        p Rational(3, 4).denominator
        begin
          Rational(1, 0)
        rescue ZeroDivisionError => e
          puts "ZeroDivisionError: #{e.message}"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Rational\n(3/1)\n3/1\n(3/2)\n(1/2)\n(5/6)\n(3/2)\n(2/3)\n(9/16)\n(1/4)\n(1/3)\n1.0\n\
         true\ntrue\n0.3333333333333333\n3\n-4\n-3\n4\n3\n4\n\
         ZeroDivisionError: divided by 0\n"
    );
}

/// `t + n` answers a Time; `t - other_time` answers a Float of seconds; `t -
/// n` answers a Time. The argument's type picks.
#[test]
fn time_arithmetic_picks_by_argument_type() {
    let result = run_ruby(
        r#"
        t = Time.at(1700000000).getutc
        p (t + 60).to_i
        p (t - 60).to_i
        p (Time.at(100) - Time.at(40))
        p (Time.at(100) - Time.at(40)).class
        p (t + 60).class
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "1700000060\n1699999940\n60.0\nFloat\nTime\n"
    );
}

/// A Float epoch is stored EXACTLY (Ruby keeps the double's true rational,
/// denominator a power of two), and `nsec` is a TRUNCATED VIEW of it. This is
/// observable: `Time.at(10.8) - 0.9` is nsec 900000000, which a `(sec, nsec)`
/// representation gets wrong (899999999) by dropping 10.8's sub-nanosecond
/// tail before subtracting. All oracle-read.
#[test]
fn time_keeps_a_float_epoch_exactly() {
    let result = run_ruby(
        r#"
        p Time.at(0.5).subsec
        p Time.at(10.8).subsec
        p Time.at(10.8).nsec
        p (Time.at(10.8) - 0.9).nsec
        p (Time.at(10.8) - 0.9).subsec
        p Time.at(1.25).to_f
        p Time.at(1700000000).getutc.subsec
        p (Time.at(100) + -1.3).usec
        p (Time.at(100) - 1.3).usec
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "(1/2)\n(225179981368525/281474976710656)\n800000000\n900000000\n(8106479329266899/9007199254740992)\n1.25\n0\n699999\n699999\n"
    );
}

/// A negative epoch floors: second -1 plus a POSITIVE sub-second remainder,
/// never second 0 minus half.
#[test]
fn time_normalizes_a_negative_float_epoch() {
    let result = run_ruby(
        r#"
        p Time.at(-0.5).to_i
        p Time.at(-0.5).nsec
        p Time.at(-1).to_i
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "-1\n500000000\n-1\n");
}

#[test]
fn numeric_coerce_protocol_for_user_types() {
    let result = run_ruby(
        r#"
        class Money
          attr_reader :cents
          def initialize(c); @cents = c; end
          def coerce(o); [Money.new(o * 100), self]; end
          def +(o); Money.new(@cents + o.cents); end
          def cents_s; @cents.to_s; end
        end
        puts((5 + Money.new(250)).cents_s)
        puts((2 + 3))
        begin; 1 + "x"; rescue TypeError => e; puts e.message; end
        class Bad; def coerce(o); 42; end; end
        begin; 1 + Bad.new; rescue TypeError; puts "bad"; end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "750\n5\nString can't be coerced into Integer\nbad\n"
    );
}

#[test]
fn integer_bit_reference_operator() {
    let result = run_ruby(
        r#"
        p 0b1011[0]
        p 0b1011[2]
        p 5[0, 2]
        p 0b1101[1, 3]
        p 255[0..3]
        p 255[4..]
        p (-2)[0]
        p (-2)[1]
        p 10[100]
        p((1 << 100)[100])
        begin
          255[..3]
        rescue ArgumentError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "1\n0\n1\n6\n15\n15\n0\n1\n0\n1\nThe beginless range for Integer#[] results in infinity\n"
    );
}

#[test]
fn float_adjacent_representable_values() {
    let result = run_ruby(
        r#"
        p 1.0.next_float > 1.0
        p 1.0.prev_float < 1.0
        p 1.0.next_float.prev_float == 1.0
        p 3.14.next_float.class
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\ntrue\nFloat\n");
}

#[test]
fn numeric_coerce_div_and_predicates() {
    let result = run_ruby(
        r#"
        p 7.coerce(2)
        p 7.coerce(2.0)
        p 7.ceildiv(2)
        p((-7).ceildiv(2))
        p 5.i
        p 10.finite?
        p 10.infinite?
        p 2.0.coerce(3)
        p 7.0.div(2)
        p 5.0.i
        p 1.0.next_float > 1.0
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[2, 7]\n[2.0, 7.0]\n4\n-3\n(0+5i)\ntrue\nnil\n[3.0, 2.0]\n3\n(0+5.0i)\ntrue\n"
    );
}

#[test]
fn rational_complex_numeric_leaf_methods() {
    let result = run_ruby(
        r#"
        r = Rational(3, 2)
        p r.finite?
        p r.infinite?
        p r.coerce(2)
        p r.coerce(2.0)
        p r.div(1)
        p Rational(7, 2).div(2)
        p r.i
        c = Complex(6, 0)
        p c.finite?
        p c.infinite?
        p c.to_f
        p c.to_i
        p c.to_r
        p c.coerce(3)
        p Complex(3, 4).finite?
        p Complex(3, 4).numerator
        p Complex(3, 4).denominator
        p Complex(Rational(2, 3), Rational(3, 4)).numerator
        p Complex(Rational(2, 3), Rational(3, 4)).denominator
        begin
          Complex(3, 4).to_f
        rescue RangeError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nnil\n[(2/1), (3/2)]\n[2.0, 1.5]\n1\n1\n(0+(3/2)*i)\ntrue\nnil\n6.0\n6\n(6/1)\n[(3+0i), (6+0i)]\ntrue\n(3+4i)\n1\n(8+9i)\n12\ncan't convert 3+4i into Float\n"
    );
}

#[test]
fn numeric_rect_polar_and_collection_leaf_methods() {
    let result = run_ruby(
        r#"
        p 5.rect
        p 5.polar
        p((-5).polar)
        p 2.5.polar
        p 5.rationalize
        p Rational(3, 2).polar
        p(nil =~ /x/)
        p [1, 2, 3, 4].rfind { |x| x.even? }
        p [10, 20, 30].fetch_values(0, 2)
        p([10, 20, 30].fetch_values(0, 5) { |i| i * 100 })
        p({ a: 1, b: 2 }.to_proc.call(:b))
        p({ a: 1, b: 2 }.transform_keys!(&:to_s))
        p((1..5).overlap?(5..8))
        p((1...5).overlap?(5..8))
        p((1..5).overlap?(6..8))
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[5, 0]\n[5, 0]\n[5, 3.141592653589793]\n[2.5, 0]\n(5/1)\n[(3/2), 0]\nnil\n4\n[10, 30]\n[10, 500]\n2\n{\"a\" => 1, \"b\" => 2}\ntrue\nfalse\nfalse\n"
    );
}

#[test]
fn numeric_tower_exactness() {
    let result = run_ruby(
        r#"
        p 42.size
        p (2**64).size
        p (2**64 - 1).size
        p (2**128).size
        p 0.3.rationalize
        p 2.5.rationalize
        p 3.14159.rationalize
        p 1.333.rationalize(0.01)
        p Rational(2.5)
        p Rational(1.5, 0.5)
        begin
          Complex(nil)
        rescue TypeError => e
          puts e.message
        end
        p 2 ** Complex(0, 1)
        p Complex(6, 0).to_r
        p Complex(3, 4).numerator
        p 7.div(Rational(2))
        p 10.div(Rational(3, 2))
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "8\n9\n8\n17\n(3/10)\n(5/2)\n(314159/100000)\n(4/3)\n(5/2)\n(3/1)\ncan't convert nil into Complex\n(0.7692389013639721+0.6389612763136348i)\n(6/1)\n(3+4i)\n3\n6\n"
    );
}

#[test]
fn integer_sqrt_is_exact_including_bignums() {
    let result = run_ruby(
        r#"
        p Integer.sqrt(0)
        p Integer.sqrt(8)
        p Integer.sqrt(9)
        p Integer.sqrt(10**20)
        p Integer.sqrt(2**100)
        begin
          Integer.sqrt(-4)
        rescue Math::DomainError => e
          puts e.message
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "0\n2\n3\n10000000000\n1125899906842624\nNumerical argument is out of domain - \"isqrt\"\n"
    );
}

#[test]
fn rational_round_family_takes_precision() {
    let result = run_ruby(
        r#"
        r = Rational(157, 50)
        p r.round(2)
        p r.round(-1)
        p r.round
        p r.floor(1)
        p r.ceil(1)
        p r.truncate(1)
        p Rational(-7, 2).round
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        // floor/ceil/truncate WITH a precision keep the fraction: 157/50 is
        // 3.14, so floor(1)=3.1=(31/10), ceil(1)=3.2=(16/5), truncate(1)=(31/10)
        // (verified against ruby 4.0.5 -- the previous expectation wrongly kept
        // (157/50) for all three).
        "(157/50)\n0\n3\n(31/10)\n(16/5)\n(31/10)\n-4\n"
    );
}

#[test]
fn numeric_step_accepts_by_and_to_keywords() {
    let result = run_ruby(
        r#"
        1.step(by: 2, to: 10) { |i| print i, " " }
        puts
        1.step(10, 2) { |i| print i, " " }
        puts
        1.step(to: 5) { |i| print i, " " }
        puts
        10.step(by: -3, to: 1) { |i| print i, " " }
        puts
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "1 3 5 7 9 \n1 3 5 7 9 \n1 2 3 4 5 \n10 7 4 1 \n"
    );
}

#[test]
fn non_finite_float_literals_compile() {
    // `1e400` overflows to Infinity at parse time. It cannot be emitted as a
    // Rust float TOKEN (`Literal::f64_suffixed` asserts `is_finite()` and
    // panics inside proc-macro2), so codegen must emit the `f64` constant
    // path instead -- the value is perfectly ordinary Ruby.
    let result = run_ruby(
        r##"
        big = 1e400
        p big
        p(-1e400)
        p big.infinite?
        p (big - big).nan?
        p [1e400, -1e400].max
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Infinity\n-Infinity\n1\ntrue\nInfinity\n");
}

#[test]
fn integer_digits_accepts_a_bignum_base() {
    // A bignum base is valid (the value simply fits in one digit when it is
    // smaller than the base); only radix < 2 is an ArgumentError.
    let result = run_ruby(
        r#"
        b = 2 ** 70
        p 255.digits(b)
        p 255.digits(b).class
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[255]\nArray\n");
}

#[test]
fn pack_integer_directive_coerces_a_float() {
    // An integer pack directive truncates a Float toward zero (CRuby coerces
    // through an exact Integer, so a value past the i64 range wraps modulo
    // 2**64 rather than saturating); NaN/Infinity raise FloatDomainError.
    let result = run_ruby(
        r#"
        p [1.5].pack("C*").bytes
        p [-3.75, 200.9].pack("c2").unpack("c2")
        p [2.0e19].pack("Q").unpack1("Q")
        p [-2.0e19].pack("q").unpack1("q")
        p [1.0e300].pack("Q").unpack1("Q")
        begin; [Float::NAN].pack("C"); rescue FloatDomainError => e; puts "nan: #{e}"; end
        begin; [-Float::INFINITY].pack("q"); rescue FloatDomainError => e; puts "inf: #{e}"; end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1]\n[-3, -56]\n1553255926290448384\n-1553255926290448384\n0\n\
         nan: NaN\ninf: -Infinity\n"
    );
}

#[test]
fn pack_base64_count_controls_wrapping() {
    // `m0` is one unbroken run with no trailing newline (RFC 4648); a bare `m`
    // wraps at 45 input bytes / 60 columns with a trailing newline; empty
    // input yields "" for both.
    let result = run_ruby(
        r#"
        p ["hello world"].pack("m0")
        p [""].pack("m0")
        p ["a\x00b".dup].pack("m0")
        p ["hi"].pack("m")
        p [""].pack("m")
        p [("A" * 50)].pack("m")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"aGVsbG8gd29ybGQ=\"\n\"\"\n\"YQBi\"\n\"aGk=\\n\"\n\"\"\n\
         \"QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFB\\nQUFBQUE=\\n\"\n"
    );
}

#[test]
fn unpack_offset_keyword_starts_mid_string() {
    // `unpack`/`unpack1` accept `offset:`; `offset == bytesize` yields the
    // empty tail (nil), past-the-end raises, negative raises.
    let result = run_ruby(
        r#"
        pair = [1.5, 2.25].pack("G2")
        p pair.unpack1("G", offset: 8)
        p pair.unpack("G", offset: 8)
        p "abc".unpack("C", offset: 3)
        begin; "abc".unpack("C", offset: 5); rescue ArgumentError => e; puts e.message; end
        begin; "abc".unpack("C", offset: -1); rescue ArgumentError => e; puts e.message; end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "2.25\n[2.25]\n[nil]\noffset outside of string\noffset can't be negative\n"
    );
}

#[test]
fn float_numerator_denominator_on_non_finite() {
    // Infinity/NaN have no rational form, so `numerator` returns the float
    // itself and `denominator` returns 1 -- CRuby never raises here.
    let result = run_ruby(
        r#"
        p Float::INFINITY.numerator
        p Float::INFINITY.denominator
        p Float::NAN.numerator
        p((-Float::INFINITY).numerator)
        p 0.5.numerator, 0.5.denominator
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Infinity\n1\nNaN\n-Infinity\n1\n2\n");
}

#[test]
fn format_prints_nonfinite_floats_with_ruby_casing() {
    // %f/%e/%g print Inf/-Inf/NaN (Ruby's casing), not C's lowercase inf.
    let result = run_ruby(
        r#"
        puts format("%.3f", Float::INFINITY)
        puts format("%f", -Float::INFINITY)
        puts format("%.2f", Float::NAN)
        puts format("%e", Float::INFINITY)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "Inf\n-Inf\nNaN\nInf\n");
}

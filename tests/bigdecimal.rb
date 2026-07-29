# bigdecimal -- the native core: construction, engineering-notation to_s,
# exact arithmetic, division to the documented precision rule, rounding
# modes, mode/limit state, conversions, and the error shapes.
require "bigdecimal"

p BigDecimal::VERSION
p [BigDecimal::BASE, BigDecimal.double_fig]
p [BigDecimal::ROUND_MODE, BigDecimal.mode(BigDecimal::ROUND_MODE), BigDecimal.mode(BigDecimal::EXCEPTION_ALL), BigDecimal.limit]

p [BigDecimal("1.5"), BigDecimal("1.5").to_s, BigDecimal("1.5").inspect]
p [BigDecimal("-0.003"), BigDecimal("123456789.123456789"), BigDecimal("1e100")]
p [BigDecimal("0"), BigDecimal("-0"), BigDecimal("0").to_s]
p [BigDecimal("Infinity"), BigDecimal("-Infinity"), BigDecimal("NaN")]
p [BigDecimal(42), BigDecimal(-7), BigDecimal(10**30)]
p [BigDecimal(3.14), BigDecimal(3.14, 3), BigDecimal(1.5, 2), BigDecimal(1e100)]
p BigDecimal(Rational(1, 3), 5)
p [BigDecimal("1_000"), BigDecimal(" 1.5 "), BigDecimal(".5"), BigDecimal("1.5e3")]
p [BigDecimal("1.5").class, BigDecimal("1.5").frozen?]

s = BigDecimal("1234.5678")
p [s.to_s("F"), s.to_s("E"), s.to_s(3), s.to_s("+"), s.to_s("3F")]
p BigDecimal("0.000012345").to_s("F")
p BigDecimal("1e30").to_s("F")
p BigDecimal("-1234.5678").to_s
p BigDecimal("-1234.5678").to_s("F")

p BigDecimal("2") + BigDecimal("3.5")
p BigDecimal("2") * BigDecimal("3.5")
p BigDecimal("1") / BigDecimal("3")
p BigDecimal("10") / 3
p BigDecimal("100") / BigDecimal("7")
p BigDecimal("1") / BigDecimal("3.00000000000000000001")
p BigDecimal("1.2345") / 3
p BigDecimal(1) / 0
p BigDecimal("1e50") + 1
p [BigDecimal("1.23456").mult(BigDecimal("2.34567"), 4), BigDecimal("1.23456").add(BigDecimal("2.34567"), 4), BigDecimal("1.23456").sub(BigDecimal("2.34567"), 4)]
p [BigDecimal("10").div(3), BigDecimal("10").div(3, 5), BigDecimal("10").div(3).class]
d = BigDecimal("10").divmod(3)
p [d, d[0].class, d[1].class]
p [BigDecimal("-10").divmod(3), BigDecimal("-10") % 3, BigDecimal("-10").remainder(3)]
p [BigDecimal("10.5") % 3, BigDecimal("10.5").divmod(3)]

p [BigDecimal("1.5") <=> 2, BigDecimal("1.5") == 1.5, BigDecimal("2") > 1, BigDecimal("1.5").eql?(1.5)]
p [1 + BigDecimal("0.5"), 1.5 + BigDecimal("1"), BigDecimal("1") + 0.5, BigDecimal("1") * 2r]
p BigDecimal("1.5").coerce(1)
p [BigDecimal("NaN") == BigDecimal("NaN"), BigDecimal("NaN") <=> 1, BigDecimal("Infinity") > 10**100]
p [BigDecimal("1.5") <=> "x", BigDecimal("1.5") == "x"]

p [BigDecimal("2.5").round, BigDecimal("2.5").round(half: :even), BigDecimal("2.345").round(2, :down)]
p [BigDecimal("1.23456789").round(3), BigDecimal("123.45").round(-1)]
p [BigDecimal("3.7").floor, BigDecimal("3.2").ceil, BigDecimal("-3.7").truncate]
p [BigDecimal("2.5").ceil(1).class, BigDecimal("2.5").ceil.class, BigDecimal("123.45").round(-1).class]
p [BigDecimal("3.75").fix, BigDecimal("3.75").frac, BigDecimal("-1.5").abs]

p [BigDecimal("1.5").to_i, BigDecimal("1.5").to_f, BigDecimal("1.5").to_r]
begin
  BigDecimal("Infinity").to_i
rescue FloatDomainError => e
  p [e.class, e.message]
end
p [BigDecimal("1.5").zero?, BigDecimal("0").zero?, BigDecimal("1.5").nonzero?, BigDecimal("0").nonzero?]
p [BigDecimal("1.5").finite?, BigDecimal("NaN").nan?, BigDecimal("-Infinity").infinite?]
p [BigDecimal("-1.5").sign, BigDecimal("0").sign, BigDecimal("NaN").sign, BigDecimal("Infinity").sign]
p [BigDecimal("123.45").exponent, BigDecimal("123.45").precision, BigDecimal("123.45").n_significant_digits]
p [BigDecimal("1e20").precision, BigDecimal("1e-20").precision, BigDecimal("100").precision]
p BigDecimal("123.45").split
p BigDecimal("123.45").precision_scale

begin
  BigDecimal("bogus")
rescue ArgumentError => e
  p [e.class, e.message]
end
begin
  BigDecimal(nil)
rescue TypeError => e
  p [e.class, e.message]
end
begin
  BigDecimal(0.1, 20)
rescue ArgumentError => e
  p [e.class, e.message]
end
begin
  BigDecimal(Rational(1, 3))
rescue ArgumentError => e
  p [e.class, e.message]
end
p [BigDecimal("1.5", exception: false), BigDecimal("x", exception: false), BigDecimal(nil, exception: false)]

# Mode and limit are process-wide; each experiment restores what it touched.
begin
  BigDecimal.mode(BigDecimal::EXCEPTION_ZERODIVIDE, true)
  BigDecimal(1) / 0
rescue FloatDomainError => e
  p [e.class, e.message]
ensure
  BigDecimal.mode(BigDecimal::EXCEPTION_ZERODIVIDE, false)
end
begin
  BigDecimal.mode(BigDecimal::EXCEPTION_NaN, true)
  BigDecimal("NaN") + 1
rescue FloatDomainError => e
  p [e.class, e.message]
ensure
  BigDecimal.mode(BigDecimal::EXCEPTION_NaN, false)
end
p BigDecimal.limit(5)
p BigDecimal("1") / BigDecimal("3")
p BigDecimal("2") + BigDecimal("1.111111111")
BigDecimal.limit(0)
p BigDecimal.save_limit {
  BigDecimal.limit(3)
  BigDecimal("1") / BigDecimal("3")
}
p BigDecimal.limit
p BigDecimal.save_rounding_mode {
  BigDecimal.mode(BigDecimal::ROUND_MODE, :down)
  BigDecimal("2.789").round(1)
}
p BigDecimal.mode(BigDecimal::ROUND_MODE)
p BigDecimal.mode(BigDecimal::ROUND_MODE, :half_even)
p BigDecimal("0.25").round(1)
BigDecimal.mode(BigDecimal::ROUND_MODE, :half_up)

p BigDecimal.interpret_loosely("1.5whatever")
p BigDecimal.interpret_loosely("junk")

# bigdecimal's RUBY half, compiled from the vendored gem: power/sqrt (in
# lib/bigdecimal.rb), BigMath (math.rb), and util's to_d family. All of it
# runs over the native core -- this golden is the north-star check that the
# real gem code compiles and answers to the digit.
require "bigdecimal"
require "bigdecimal/math"
require "bigdecimal/util"

p BigDecimal("2") ** 10
p BigDecimal("2").power(-2)
p BigDecimal("2").power(10, 5)
p BigDecimal("1.5").power(3)
p BigDecimal("3") ** -3
p BigDecimal("2") ** 0.5
p BigDecimal("2").power(BigDecimal("0.5"))
p BigDecimal("0.333333333333333333333333333333333").power(2)

p BigDecimal("2").sqrt(10)
p BigDecimal("2").sqrt(0)
p BigDecimal("2").sqrt(40)
begin
  BigDecimal("2").sqrt(-1)
rescue ArgumentError => e
  p [e.class, e.message]
end
begin
  BigDecimal("-1").sqrt(10)
rescue FloatDomainError => e
  p [e.class, e.message]
end

p BigMath.sqrt(BigDecimal("2"), 10)
p BigMath.exp(BigDecimal("1"), 20)
p BigMath.exp(BigDecimal("-1"), 20)
p BigMath.log(BigDecimal("2"), 20)
p BigMath.log(BigDecimal("10"), 30)
p BigMath::PI(20)
p BigMath::E(20)
p BigMath.sin(BigDecimal("1"), 20)
p BigMath.cos(BigDecimal("1"), 20)
p BigMath.atan(BigDecimal("1"), 20)
p BigMath.cbrt(BigDecimal("27"), 10)
p BigMath.log2(BigDecimal("8"), 10)
p BigMath.log10(BigDecimal("1000"), 10)

p "1.23".to_d
p 1.5.to_d
p 1.5.to_d(2)
p 42.to_d
p (10**30).to_d
p Rational(1, 3).to_d(10)
p "x".to_d
p "1.5abc".to_d
p BigDecimal("1.5").to_digits
p BigDecimal("1234.5678").to_digits
p 0.5.to_d(0)

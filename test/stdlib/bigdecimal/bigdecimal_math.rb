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
__END__
0.1024e4
0.25e0
0.1024e4
0.3375e1
0.37037037037037037037037037037037e-1
0.14142135623730950488016887242097e1
0.14142135623730950488016887242097e1
0.111111111111111111111111111111110888888888888888888888888888888889e0
0.1414213562e1
0.1414213562373095e1
0.141421356237309504880168872420969807857e1
[ArgumentError, "Negative precision for sqrt"]
[FloatDomainError, "sqrt of negative value"]
0.1414213562e1
0.27182818284590452354e1
0.3678794411714423216e0
0.69314718055994530942e0
0.230258509299404568401799145468e1
0.31415926535897932385e1
0.27182818284590452354e1
0.84147098480789650665e0
0.5403023058681397174e0
0.78539816339744830962e0
0.3e1
0.3e1
0.3e1
0.123e1
0.15e1
0.15e1
0.42e2
0.1e31
0.3333333333e0
0.0
0.15e1
"1.5"
"1234.5678"
0.5e0

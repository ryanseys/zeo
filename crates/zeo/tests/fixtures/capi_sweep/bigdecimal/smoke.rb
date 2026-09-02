require "bigdecimal"
require "bigdecimal/util"

a = BigDecimal("1.1")
b = BigDecimal("2.2")
puts (a + b).to_s, (a * b).to_s, (b / a).to_s, (a - b).to_s
puts BigDecimal("1") / BigDecimal("3")
puts BigDecimal("123.456").round(1).to_s, BigDecimal("2").sqrt(10).to_s
puts "3.14".to_d.to_f, BigDecimal("1e-20").exponent, a.class, a.precision
puts BigDecimal("0.1") + BigDecimal("0.2") == BigDecimal("0.3")
puts BigDecimal("-0").sign, BigDecimal("NaN").nan?, BigDecimal("Infinity").infinite?

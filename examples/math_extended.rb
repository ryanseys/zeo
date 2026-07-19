# Math hyperbolic/inverse-hyperbolic, gamma/lgamma, erf/erfc, frexp/ldexp --
# backed by the same system libm CRuby's Math uses, so values match exactly.
rd = ->(x) { x.round(10) }

puts rd.(Math.sinh(1.0))
puts rd.(Math.cosh(1.0))
puts rd.(Math.tanh(1.0))
puts rd.(Math.asinh(2.0))
puts rd.(Math.acosh(2.0))
puts rd.(Math.atanh(0.5))
p Math.atanh(1.0)            # pole -> Infinity, not a DomainError

# gamma: exact factorials for small positive integers, ±0/±inf special cases.
puts Math.gamma(6.0)         # 120.0
puts rd.(Math.gamma(0.5))
p Math.gamma(0.0)            # +Infinity
p Math.gamma(172).infinite? # overflow -> Infinity

# lgamma -> [log|gamma|, sign]; poles give [Infinity, ±1].
p Math.lgamma(0.5).map { |v| v.is_a?(Float) ? rd.(v) : v }
p Math.lgamma(-1.0)

# frexp/ldexp are exact inverses.
p Math.frexp(8.0)           # [0.5, 4]
puts Math.ldexp(0.75, 3)    # 6.0

puts rd.(Math.erf(0.5))
puts rd.(Math.erfc(0.5))

# Domain errors (verbatim CRuby message).
begin
  Math.gamma(-2.0)
rescue Math::DomainError => e
  puts e.message
end

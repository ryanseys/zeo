# Numeric-tower exactness: Integer#size, Float#rationalize, Rational/Complex.

# Integer#size: 8 bytes up to a machine word, then the magnitude byte-width.
p 42.size
p (2**64).size
p (2**64 - 1).size
p (2**128).size

# Float#rationalize finds the simplest rational rounding back to the double.
p 0.3.rationalize
p 0.1.rationalize
p 2.5.rationalize
p 3.14159.rationalize
p(-0.75.rationalize)
p 1.333.rationalize(0.01)

# Rational(Float) keeps the exact dyadic value.
p Rational(2.5)
p Rational(0.3)
p Rational(1.5, 0.5)

# Complex construction, projection, and general exponentiation.
begin
  Complex(nil)
rescue TypeError => e
  puts e.message
end
p 2 ** Complex(0, 1)
p Complex(2, 0) ** Complex(1, 1)
p Complex(6, 0).to_r
p Complex(3, 4).numerator

# Integer#div with a Rational divisor stays exact (floored).
p 7.div(Rational(2))
p 10.div(Rational(3, 2))

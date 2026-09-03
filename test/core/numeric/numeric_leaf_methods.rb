# Rational and Complex leaf methods, plus the Numeric rect/polar family.

# Rational predicates, coercion, floored division, imaginary lift.
r = Rational(3, 2)
puts r.finite?
p r.infinite?
p r.coerce(2)
p r.coerce(2.0)
p r.div(1)
p Rational(7, 2).div(2)
p r.i

# Complex finiteness, coercion, and the real-projection conversions.
c = Complex(6, 0)
puts c.finite?
p c.infinite?
p c.to_f
p c.to_i
p c.to_r
p c.coerce(3)
p Complex(3, 4).finite?
begin
  Complex(3, 4).to_f
rescue RangeError => e
  puts e.message
end

# numerator/denominator scale both components to a shared denominator.
p Complex(3, 4).numerator
p Complex(3, 4).denominator
p Complex(Rational(2, 3), Rational(3, 4)).numerator
p Complex(Rational(2, 3), Rational(3, 4)).denominator

# rect / rectangular / polar are shared by every real Numeric.
p 5.rect
p 5.polar
p((-5).polar)
p 2.5.polar
p 5.rationalize
p Rational(3, 2).polar

# nil never matches a pattern.
p(nil =~ /x/)
__END__
true
nil
[(2/1), (3/2)]
[2.0, 1.5]
1
1
(0+(3/2)*i)
true
nil
6.0
6
(6/1)
[(3+0i), (6+0i)]
true
can't convert 3+4i into Float
(3+4i)
1
(8+9i)
12
[5, 0]
[5, 0]
[5, 3.141592653589793]
[2.5, 0]
(5/1)
[(3/2), 0]
nil

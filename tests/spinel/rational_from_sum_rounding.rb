# A Rational that arrives boxed -- the result of Array#sum or reduce(:+) over
# Rational elements -- answers the rounding surface and converts back through
# Kernel#Rational like a statically typed one.
r = [Rational(1, 2), Rational(1, 4)].sum
p r
p r.round
p r.floor
p r.ceil
p r.truncate
p r.round(2)
p r.to_f
p r.numerator
p r.denominator

p Rational(r, 2)
p Rational(r)
p Rational(r, r)

q = [Rational(3, 2), Rational(1, 2)].reduce(:+)
p q
p q.round
p q.floor

neg = [Rational(-3, 4), Rational(0, 1)].sum
p neg.round
p neg.floor
p neg.ceil
p neg.truncate

vals = [Rational(1, 2), 3, 2.5]
vals.each { |v| p Rational(v, 2) }

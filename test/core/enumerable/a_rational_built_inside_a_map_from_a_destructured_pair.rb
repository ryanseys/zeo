# map over [numerator, denominator] pairs, reducing each by its gcd before
# Rational.
# (spinel issue #3184)
fracs = [[6, 8], [10, 15]]
p fracs.map { |n, d| g = n.gcd(d); Rational(n / g, d / g) }
__END__
[(3/4), (2/3)]

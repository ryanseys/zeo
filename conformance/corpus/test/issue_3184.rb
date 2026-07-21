fracs = [[6, 8], [10, 15]]
p fracs.map { |n, d| g = n.gcd(d); Rational(n / g, d / g) }

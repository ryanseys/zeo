# An exact receiver reaching an operator or a numeric method through a block
# parameter is boxed, and the poly path answered it in floats or not at all.
# `**` evaluated in doubles (a different answer as well as a different class),
# `%` truncated to Integer, and divmod / modulo / quo / abs had no arm at all
# so they raised NoMethodError on methods Rational answers (#3510, #3512).
p([Rational(3, 2)].map { |a| a**2 })
p([Rational(7, 2)].map { |a| a % 2 })
p([Rational(-7, 2)].map { |a| a**3 })
p([Complex(1, 2)].map { |a| a**2 })
p([Complex(1, 2)].map { |a| a**3 })

p([Rational(7, 2)].map { |a| a.divmod(2) })
p([Rational(7, 2)].map { |a| a.modulo(2) })
p([Rational(7, 2)].map { |a| a.quo(2) })
p([Rational(-7, 2)].map { |a| a.abs })
p([Rational(7, 2)].map { |a| a.abs })

# the neighbours that were already right stay right
p([Rational(7, 2)].map { |a| a.floor })
p([Rational(7, 2)].map { |a| a.round })
p([Rational(7, 2)].map { |a| a.truncate })
p([Rational(7, 2)].map { |a| a.to_i })
p([Rational(3, 2)].map { |a| a + 1 })
p([Rational(3, 2)].map { |a| a * 2 })
p([Rational(3, 2)].map { |a| a / 2 })
p([Rational(3, 2)].map { |a| a - 1 })

# and the same operators on the kinds that share the path
p([2].map { |a| a**3 })
p([7].map { |a| a % 2 })
p([7].map { |a| a.divmod(2) })
p([7].map { |a| a.quo(2) })
p([-7].map { |a| a.abs })
p([2.5].map { |a| a**2 })
p([7.5].map { |a| a % 2 })
p([7.5].map { |a| a.divmod(2) })
p([-7.5].map { |a| a.abs })

# a monomorphic receiver was always exact; it has to stay that way
r = Rational(3, 2)
p r**2
p r % 1
p r.divmod(1)
p r.quo(2)
p r.abs
__END__
[(9/4)]
[(3/2)]
[(-343/8)]
[(-3+4i)]
[(-11-2i)]
[[1, (3/2)]]
[(3/2)]
[(7/4)]
[(7/2)]
[(7/2)]
[3]
[4]
[3]
[3]
[(5/2)]
[(3/1)]
[(3/4)]
[(1/2)]
[8]
[1]
[[3, 1]]
[(7/2)]
[7]
[6.25]
[1.5]
[[3, 1.5]]
[7.5]
(9/4)
(1/2)
[1, (1/2)]
(3/4)
(3/2)

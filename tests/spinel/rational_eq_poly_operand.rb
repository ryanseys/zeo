# Rational#== / #eql? against an operand whose type is not known here -- a
# Rational read out of an Array is the usual way that happens. A poly operand
# is not a non-numeric operand, but the arm treated it as one and answered a
# constant false, so the comparison was false one way round and true the other.
[Rational(5, 6)].each { |r| p(Rational(5, 6) == r) }
[Rational(5, 6)].each { |r| p(r == Rational(5, 6)) }
[Rational(5, 6)].each { |r| p(Rational(5, 6) != r) }
[Rational(5, 6)].each { |r| p(Rational(5, 6) === r) }

x = [Rational(5, 6)].first
p(Rational(5, 6) == x)
p(Rational(1, 2) == x)

# #eql? is #== plus a class check, so a poly operand must be asked at run time
# whether it IS a Rational
[Rational(5, 6)].each { |r| p(Rational(5, 6).eql?(r)) }
[Rational(1, 1)].each { |r| p(Rational(1, 1) == r) }
mixed = [Rational(1, 1), 1, "x"]
p(Rational(1, 1) == mixed[0])
p(Rational(1, 1) == mixed[1])
p(Rational(1, 1).eql?(mixed[0]))
p(Rational(1, 1).eql?(mixed[1]))
p(Rational(1, 1) == mixed[2])

p([Rational(5, 6)].select { |r| Rational(5, 6) == r }.size)
p([Rational(5, 6)].map { |r| Rational(5, 6) <=> r })

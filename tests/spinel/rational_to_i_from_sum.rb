# to_i on a Rational that arrives boxed (from sum / inject) truncates toward
# zero like the statically typed one.
r = [Rational(7, 2)].sum
p r
p r.to_i
p r.truncate
p r.floor
p r.to_f
q = [Rational(1, 2), Rational(1, 4)].inject(:+)
p q
p q.to_i
p Rational(-7, 2).to_i
n = [Rational(-7, 2)].sum
p n.to_i

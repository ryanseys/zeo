# Rational#round/floor/ceil/truncate take an optional precision: a positive
# ndigits answers a Rational, zero or negative an Integer.
r = Rational(157, 50)   # 3.14
p r.round
p r.round(2)
p r.round(1)
p r.round(-1)
p r.floor(1)
p r.ceil(1)
p r.truncate(1)
p Rational(-7, 2).round      # ties away from zero
p Rational(-7, 2).truncate   # toward zero
p Rational(25, 10).round(-1)
__END__
3
(157/50)
(31/10)
0
(31/10)
(16/5)
(31/10)
-4
-3
0

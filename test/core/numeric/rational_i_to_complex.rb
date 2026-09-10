# Rational#i answers Complex(0, self), and the imaginary part stays an exact
# Rational rather than collapsing to a Float.
p(Rational(3, 4).i)
p(Rational(-1, 2).i)
p(Rational(2, 1).i)

r = Rational(3, 4)
c = r.i
p c.class                # Complex
p c.real                 # exact
p c.imaginary            # (3/4), not 0.75
p c.imaginary == 0.75
p c.abs2 == 0.5625
__END__
(0+(3/4)*i)
(0-(1/2)*i)
(0+(2/1)*i)
Complex
0
(3/4)
true
true

# Rational#i -> Complex(0, self). spinel's Complex is float-backed, so the
# imaginary part renders as a Float where CRuby keeps the exact Rational
# (docs/limitations.md "Rational precision and Complex components").
p(Rational(3, 4).i)      # (0+0.75i)   CRuby: (0+(3/4)*i)
p(Rational(-1, 2).i)     # (0-0.5i)    CRuby: (0-(1/2)*i)
p(Rational(2, 1).i)      # (0+2.0i)    CRuby: (0+(2/1)*i)

r = Rational(3, 4)
c = r.i
p c.class                # Complex
p c.real                 # 0   -- the real part is exact
p c.imaginary            # 0.75  CRuby: (3/4)
# the VALUE is right even though the component class is not
p c.imaginary == 0.75
p c.abs2 == 0.5625

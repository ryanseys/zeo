# Arithmetic and abs/abs2/magnitude on a Complex read out of a poly container
# (the poly `+`/`-`/`*`/`/` and sp_poly_abs dispatch the Complex tag). Complex
# division with integer components follows spinel's float-backed Complex
# (a documented divergence from CRuby's rational components), so the divisor
# here is chosen to divide cleanly.
xs = [Complex(1, 2), Complex(3, -4)]
c = xs.first
p (c + Complex(5, 5))
p (c - Complex(1, 1))
p (c * 2)
p (c * Complex(2, 1))
p (Complex(4, 2) == xs.first * Complex(2, 0) + Complex(2, 2))
p (c + 3)
p c.abs
p c.abs2
p c.magnitude
p xs.reduce(Complex(0, 0)) { |a, z| a + z }
p xs.map { |z| z.abs }

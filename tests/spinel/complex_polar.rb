# Built-in `Complex` value type. Surfaced via optcarrot's
# `nestopia_palette`:
#   iq = Complex.polar(s, theta)
#   iq += Complex.polar(level1, theta2)
#   clr = y + (Complex.polar(g, t) * iq.conjugate).real
#
# Spinel models Complex as a 16-byte value type (sp_Complex with
# `re` and `im` fields). Allocation-free; passes by value.
# Methods: `Complex.polar(magnitude, angle)`, instance-side
# `.real / .imaginary / .conjugate`, plus `+ *` between two
# Complex values.

# Polar form: r=2, theta=0 → (2, 0)
a = Complex.polar(2.0, 0.0)
puts a.real            # 2.0
puts a.imaginary       # 0.0

# r=1, theta=PI/2 → (~0, 1)
b = Complex.polar(1.0, Math::PI / 2)
puts b.real.abs < 1e-9    # true (cos(pi/2) ~ 0)
puts (b.imaginary - 1.0).abs < 1e-9  # true

# Addition: a + b
c = a + b
puts (c.real - 2.0).abs < 1e-9       # true
puts (c.imaginary - 1.0).abs < 1e-9  # true

# Conjugate flips the sign of imag
d = b.conjugate
puts d.real.abs < 1e-9    # true
puts (d.imaginary + 1.0).abs < 1e-9  # true

# Multiplication: (1, 1) * (1, -1) = (1*1 - 1*-1, 1*-1 + 1*1) = (2, 0)
e = Complex.polar(Math.sqrt(2), Math::PI / 4)  # ~ (1, 1)
f = e * e.conjugate                            # |e|^2 = 2 + 0i
puts (f.real - 2.0).abs < 1e-9       # true
puts f.imaginary.abs < 1e-9          # true

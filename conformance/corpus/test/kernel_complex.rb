# Kernel#Complex(re, im) builds a Complex value (imaginary part defaults
# to 0). Previously the no-receiver Complex() call fell through to the
# integer 0 fallback.
p Complex(2, 3)
p Complex(1)
p Complex(2.5, -1.5)
p Complex(0, 1)
p(Complex(1, 2) + Complex(3, 4))
p(Complex(2, 3) * Complex(1, 1))

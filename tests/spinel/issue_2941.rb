# conjugate/conj on a Complex read out of a poly container: dispatch on the
# runtime tag (negate the imaginary part) instead of returning an opaque value.
xs = [Complex(1, 2), Complex(3, 4)]
p xs[1].conjugate.real.round(3)
p xs[0].conjugate.imaginary
p xs[1].conj.real

# conjugate and its alias conj negate the imaginary part of an array element.
# (spinel issue #2941)
xs = [Complex(1, 2), Complex(3, 4)]
p xs[1].conjugate.real.round(3)
p xs[0].conjugate.imaginary
p xs[1].conj.real
__END__
3
-2
3

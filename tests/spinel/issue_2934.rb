# numerator/denominator on a Rational read out of a poly container: the
# poly receiver must dispatch on the runtime tag, not return an opaque value.
arr = [Rational(1, 3), Rational(1, 2)]
p arr[0].numerator + arr[1].numerator
p arr[0].denominator
a = [Rational(3, 4), 7]
p a[1].numerator
p a[1].denominator

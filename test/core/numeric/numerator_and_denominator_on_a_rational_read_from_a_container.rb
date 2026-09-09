# Reading them off array elements, including an array mixing a Rational with an
# Integer.
# (spinel issue #2934)
arr = [Rational(1, 3), Rational(1, 2)]
p arr[0].numerator + arr[1].numerator
p arr[0].denominator
a = [Rational(3, 4), 7]
p a[1].numerator
p a[1].denominator
__END__
2
3
7
1

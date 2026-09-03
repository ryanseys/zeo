# Infinity/NaN have no rational form, so `numerator` returns the float
# itself and `denominator` returns 1 -- CRuby never raises here.

p Float::INFINITY.numerator
p Float::INFINITY.denominator
p Float::NAN.numerator
p((-Float::INFINITY).numerator)
p 0.5.numerator, 0.5.denominator
__END__
Infinity
1
NaN
-Infinity
1
2

# Float#numerator and #denominator raise FloatDomainError for Infinity and NaN, and answer for a finite Float.
# (spinel issue #3011)
r1 = (Float::INFINITY.numerator rescue $!.class); p r1
r2 = (Float::INFINITY.denominator rescue $!.class); p r2
r3 = (Float::NAN.numerator rescue $!.class); p r3
r4 = (Float::NAN.denominator rescue $!.class); p r4
p 0.5.numerator
p 0.5.denominator
p((-Float::INFINITY).numerator)
p 3.numerator, 3.denominator
p (1/3r).numerator, (1/3r).denominator
__END__
Infinity
1
NaN
1
1
2
-Infinity
3
1
1
3

r1 = (Float::INFINITY.numerator rescue $!.class); p r1
r2 = (Float::INFINITY.denominator rescue $!.class); p r2
r3 = (Float::NAN.numerator rescue $!.class); p r3
r4 = (Float::NAN.denominator rescue $!.class); p r4
p 0.5.numerator
p 0.5.denominator
p((-Float::INFINITY).numerator)
p 3.numerator, 3.denominator
p (1/3r).numerator, (1/3r).denominator

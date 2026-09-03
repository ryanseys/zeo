# Integer ** Complex (and a complex exponent) evaluate to Complex; Integer#fdiv
# and #div with a Rational argument are exact. (** with a Rational exponent is
# a Float by design -- see docs/limitations.md.)
p(3 ** Complex(0, 1))
p(2 ** Complex(1, 1))
p(7.fdiv(2r))
p(4.fdiv(Rational(1, 2)))
p(10.div(3r))
__END__
(0.4548324228266097+0.8905770416677471i)
(1.5384778027279442+1.2779225526272695i)
3.5
8.0
3

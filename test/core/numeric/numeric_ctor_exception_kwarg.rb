# From the spinel corpus (c55d9bdb).
# Complex() with an exception: keyword mis-routes the keyword into the
# numeric argument.
#
# `exception: false` is a keyword, not the second component: it counted as one
# and the value came out of the keyword hash's address (#3869).
p(Complex("1+2i", exception: false))
p(Complex("1+2i"))
p(Complex(1, 2))
p(Rational("1/2", exception: false))
p(Rational("1/2"))
p(Rational(1, 2))
p(Integer("42", exception: false))
p(Integer("abc", exception: false))
p(Float("1.5", exception: false))
__END__
(1+2i)
(1+2i)
(1+2i)
(1/2)
(1/2)
(1/2)
42
nil
1.5

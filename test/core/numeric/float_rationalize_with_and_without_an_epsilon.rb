# A Rational epsilon, a Float epsilon, and no argument.
p 0.333333.rationalize(Rational(1, 1000))
p 0.5.rationalize(Rational(1, 100))
p 0.333333.rationalize(0.001)
p 0.333333.rationalize
__END__
(1/3)
(1/2)
(1/3)
(333333/1000000)

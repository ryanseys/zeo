# `r += x`, `-=`, `*=` and `/=` each rebind the local to a reduced Rational.
t = Rational(0, 1)
t += Rational(1, 2)
p t
t -= Rational(1, 4)
p t
t *= 3
p t
t /= Rational(3, 2)
p t
__END__
(1/2)
(1/4)
(3/4)
(1/2)

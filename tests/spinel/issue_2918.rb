# Rational operator-assignment (`r += x`, `-=`, `*=`, `/=`) on a Rational local.
t = Rational(0, 1)
t += Rational(1, 2)
p t
t -= Rational(1, 4)
p t
t *= 3
p t
t /= Rational(3, 2)
p t

# Rational() with a bignum operand, and Complex#zero? on a poly receiver
p Rational(-(10**30), 3)
p Rational(10**30, -3)
p Rational(10**30)
p Rational(10**30, 10**28)
p Rational(3, 4)
p Rational(6, 4)

def sign(v) = "#{v.zero?} #{v.positive?} #{v.negative?}"
def sign_via(pair) = sign(pair[1])
puts sign_via([:x, Rational(-(10**30), 3)])
puts sign_via([:x, Rational(10**30, 3)])

def z(v) = v.zero?
def z_via(pair) = z(pair[1])
p z_via([:x, Complex(0, 0)])
p z_via([:x, Complex(1, 2)])
p z_via([:x, Complex(0, 1)])
r = (z_via([:x, "abc"]) rescue $!.class); p r
r2 = (z_via([:x, nil]) rescue $!.class); p r2

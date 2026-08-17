# A by-value struct (Range, Time, Rational, Complex) reaching a lambda or proc
# parameter rides the boxed side channel, as a Float already did: the mrb_int
# argument slot cannot hold one, and the emitted C did not compile (#3962).
f = ->(r) { r.cover?(5) }
p f.call(1..10)
g = proc { |r| r.to_a }
p g.call(1..3)
h = ->(r, v) { r.include?(v) }
p h.call(1..10, 3)
K = ->(r) { r.first }
p K.call(5..9)
t = ->(x) { x.to_i }
p t.call(Time.at(1234))
q = ->(x) { x.to_s }
p q.call(Rational(3,4))
c = ->(x) { x.real }
p c.call(Complex(1,2))
fr = ->(x) { x.first }
p fr.call(1.5..3.5)
p [1..3, 4..6].map { |r| r.sum }

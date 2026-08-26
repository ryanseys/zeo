# A container holding NaN compares EQUAL to itself in ruby: `Array#==`
# short-circuits on VALUE identity before it asks `==`, and two NaN
# flonums are the same VALUE. Zeo boxes a Float, so the short-circuit
# never fires and `[NaN] == [NaN]` answers false.
#
# Wrong answer, silently, for any container holding a NaN -- which is what
# a YAML or JSON round trip produces from `.nan`.
n = Float::NAN
p [n] == [n]
p [Float::NAN] == [Float::NAN]
p({ a: n } == { a: n })
p [[n]] == [[n]]
p n == n
p [n].include?(n)

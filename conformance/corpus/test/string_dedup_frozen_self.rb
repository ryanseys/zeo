# String#-@ on an already-frozen string returns the receiver itself (#2630).
a = "abc".freeze
p((-a).equal?(a))
c = -"xyz"
p((-c).equal?(c))

C = Data.define(:class, :hash)
c = C.new("k", 5)
p(c.class)
p(c.hash)
# a Struct member named after a builtin shadows it too
S = Struct.new(:class)
s = S.new("sv")
p(s.class)
# normal Data still reports its class and a consistent hash
P = Data.define(:x, :y)
p P.new(1, 2).class
p(P.new(1, 2).hash == P.new(1, 2).hash)

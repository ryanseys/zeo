# `[].dup` is a copy of nothing, so it is a new empty array -- and it has to be
# built at the target's type. Emitted as an ordinary call it took the empty
# literal's own default kind and wrote an sp_IntArray into a poly-array slot,
# where the frozen flag read out of the wrong field.
a = [].dup
p a.frozen?
p a
a << 1
a << :two
p a

b = []
p b.frozen?

c = [].clone
p c.frozen?
c << "x"
p c

d = [1, 2].dup
p d
p d.frozen?
e = [1, 2].freeze
p e.clone.frozen?
p e.dup.frozen?

# An empty receiver leaves the block's tail nothing to widen against, so a
# statically typed tail (`0`) reached the poly slot raw where a boxed value is
# what it holds.
a = []
a.map! { 0 }
p a

a << 1
a.map! { |x| x + 1 }
p a

b = []
b.map! { "s" }
p b

c = []
c.collect! { nil }
p c

d = [1, "x", 2.5]
d.map! { |v| v.to_s }
p d

e = []
e.map! { |x| x }
p e

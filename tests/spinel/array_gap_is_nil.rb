# `a[n] = v` past the end fills the gap with nil, not the element type's zero
# value (#3836).
a = ["x"]
a[3] = "y"
p a
b = [1]
b[3] = 5
p b
f = [1.0]
f[2] = 2.0
p f
p a[1]
p b[1]
p f[1]
p a.compact
p b.compact

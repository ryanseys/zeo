# The container fast paths (`a << x`, `a[i] = v`, `h[k] = v`, `a.length`,
# `a.pop`, `h[k]`) BORROW a plain hoisted receiver instead of cloning it --
# but the borrow is live while the arguments evaluate, and an argument that
# reassigns the receiver's own local is perfectly good Ruby. Each line below
# does exactly that, and each must answer what CRuby answers rather than
# failing to compile with E0506.
a = [1]
a << (a = [9]; 2)
p a
b = [1, 2]
b[0] = (b = [7, 8]; 5)
p b
h = { x: 1 }
h[:y] = (h = { z: 3 }; 4)
p h
c = [1, 2, 3]
p c.length + (c = []; 0)
d = [5]
p d[(d = [6, 7]; 0)]
e = [1]
p e.pop + (e = [2]; 0)
f = { a: 1 }
p f[(f = { b: 2 }; :a)].inspect

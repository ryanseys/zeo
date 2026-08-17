# An index write past the end grows the array, filling the gap with nil, the
# way every typed array already did. The poly array silently dropped the write.
a = []
a[0] = 1
p a
b = []
b[3] = "x"
p b
c = [1, 2, 3]
c[1] = 9
p c
c[-1] = 8
p c
d = []
d[0] = 1
d[2] = 3
p d
e = [1]
e[5] = :z
p e
p e.length


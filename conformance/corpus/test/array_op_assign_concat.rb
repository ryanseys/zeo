# `arr += other` on a typed array local — operator-assignment with `+`,
# which is `arr = arr + other` (a fresh concatenation). Previously only
# Int/Float/String/object locals had an op-assign path; a typed array
# local fell through to "unsupported operator assignment". `<<` and
# `concat` already worked; this covers `+=`.
a = [1, 2]
a += [3, 4]
p a

s = ["a"]
s += ["b", "c"]
p s

f = [1.5]
f += [2.5, 3.5]
p f

# genuinely heterogeneous (poly) array
h = [1, "a", :sym]
h += [2, "b"]
p h

# empty-literal RHS (must not build a wrong-kind array)
e = [1, 2]
e += []
p e

# poly array from a method whose element type the compiler widens
def ids
  r = []
  r << "x"
  r
end
q = ids
q += ids
puts q.length

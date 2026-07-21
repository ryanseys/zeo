def mark(h, k)
  h[k] = true
end
h = {}
mark(h, :a)
mark(h, :b)
p h
def put(m, k, v)
  m[k] = v
end
n = {}
put(n, 7, "x")
p n

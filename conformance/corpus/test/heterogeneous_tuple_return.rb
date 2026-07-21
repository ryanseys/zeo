def kv(s)
  return nil, nil if s.empty?
  return s, s
end

a, b = kv("x")
p [a, b]
c, d = kv("")
p [c, d]

def mixed(s)
  return 1, 1, 1 if s == "i"
  return "a", "b", "c" if s == "s"
  return nil, nil, nil
end

e, f, g = mixed("i")
p [e, f, g]
h, i, j = mixed("s")
p [h, i, j]
k, l, m = mixed("x")
p [k, l, m]

def partial(s)
  return s, 1 if s.empty?
  return s, "x"
end

n, o = partial("")
p [n, o]
q, r = partial("y")
p [q, r]

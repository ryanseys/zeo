# Two calls to a method that assigns one hash key leave both keys behind.
# (spinel issue #2894)
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
__END__
{a: true, b: true}
{7 => "x"}

# `def fill(h) = h[1] = 100` changes the caller's hash, with an Integer key and
# with a Symbol.
def fill(h)
  h[1] = 100
end
h = {}
fill(h)
p h
# also symbol key + direct-arg form
def fill2(m)
  m[:x] = 9
  m[:y] = 8
end
g = {}
fill2(g)
p g
__END__
{1 => 100}
{x: 9, y: 8}

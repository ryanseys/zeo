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

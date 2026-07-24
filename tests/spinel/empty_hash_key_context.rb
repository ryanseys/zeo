# An empty Hash literal bound to a local takes its variant from the key it is
# indexed with, not the StrPolyHash fallback.
h = {}
p h.fetch(1, "d")
g = {}
p g.fetch(:sym, "s")
begin
  k = {}
  k.fetch(:missing)
rescue KeyError => e
  p e.key
end
begin
  n = {}
  n.fetch(99)
rescue KeyError => e
  p e.key
end
s = {}
p s.fetch("str", 0)
# writes still drive the variant when present
w = {}
w[:a] = 1
p w

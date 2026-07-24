# Splatting an inline map-of-lambdas into a *args method whose vararg is
# captured by a returned closure (#3242): the rest-consumed splat is not
# hoisted as a normal arg (ill-typed dead temp + double evaluation), and a
# lambda that is a block's tail value escapes (poly params via the boxed ABI).
def alt(*parsers)
  ->(s) { parsers.map { |pr| pr.call(s) } }
end
g = alt(*(0..2).map { |d| ->(s) { s } })
p g.call("5")
ps = (0..2).map { |d| ->(s) { s } }
h = alt(*ps)
p h.call("7")

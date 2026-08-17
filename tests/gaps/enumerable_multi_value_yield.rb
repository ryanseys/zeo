# GAP -- imported from the spinel corpus at c55d9bdb.
# A block that yields several values is not auto-splatted into the
# collector, so each entry arrives as a nested array.
#
# `each` yielding more than one value per element: Enumerable packs them into
# one array element. The collector block took a single parameter, so every
# element kept only the first value (or nil, once the pair was destructured).
class Pairs
  include Enumerable
  def each
    yield :a, 1
    yield :b, 2
  end
end

p(Pairs.new.map { |k, v| [k, v] })
p(Pairs.new.map { |k, v| k })
p(Pairs.new.to_a)
p(Pairs.new.to_h)
p(Pairs.new.sort_by { |k, v| -v })
p(Pairs.new.select { |k, v| v > 1 })
p(Pairs.new.first)
p(Pairs.new.count)
Pairs.new.each { |k, v| p [k, v] }

# a single-value each keeps its flat elements
class Ones
  include Enumerable
  def each
    yield 1
    yield 2
  end
end

p Ones.new.to_a
p Ones.new.map { |x| x * 2 }
p Ones.new.sum

# `for` binds only as many values as it has index variables, so one variable
# keeps the first of a multi-value yield rather than the packed element
class OFor
  def each
    [[1, 2, 3], [4, 5, 6]].each { |a| yield(a[0], a[1], a[2]) }
  end
end

o = OFor.new
qs = []
for q in o
  qs << q
end
p qs
p q

rs = []
for a, b in o
  rs << [a, b]
end
p rs

ts = []
for t in Ones.new
  ts << t
end
p ts

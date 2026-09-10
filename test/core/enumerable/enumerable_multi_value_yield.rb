# `each` yielding more than one value per element. Enumerable packs them into
# one array element, so `map`/`to_a`/`sort_by` see pairs.
#
# `for` does not: it is `obj.each { |targets| }` with the ordinary block
# rules, so ONE target binds the first yielded value and drops the rest,
# where several pack and destructure.
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

# The rest of the target shapes take the FIRST yielded value too -- `for`
# is `each` with the ordinary block rules, and one parameter is one parameter.
class Two
  def each
    yield 1, 2
    yield 3, 4
  end
end
t = Two.new
$g = nil
for $g in t; end
p $g
class Box; attr_accessor :v; end
bx = Box.new
for bx.v in t; end
p bx.v
arr = [nil]
for arr[0] in t; end
p arr
for (a, b) in t
  p [a, b]
end
for c, in t
  p c
end

# A yield of ONE array still auto-splats into several targets, and stays
# whole under one.
class Wrapped
  def each
    yield [7, 8]
  end
end
for w in Wrapped.new
  p w
end
for x, y in Wrapped.new
  p [x, y]
end

# A yield of NO values binds nil either way.
class Empty
  def each
    yield
    yield
  end
end
es = []
for e in Empty.new
  es << e
end
p es
__END__
[[:a, 1], [:b, 2]]
[:a, :b]
[[:a, 1], [:b, 2]]
{a: 1, b: 2}
[[:b, 2], [:a, 1]]
[[:b, 2]]
[:a, 1]
2
[:a, 1]
[:b, 2]
[1, 2]
[2, 4]
3
[1, 4]
4
[[1, 2], [4, 5]]
[1, 2]
3
3
[3]
[1, 2]
[3, 4]
1
3
[7, 8]
[7, 8]
[nil, nil]

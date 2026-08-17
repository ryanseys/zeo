# The Enumerable and Integer names a boxed receiver shares with its typed self:
# a value that only reads poly (a container element, a widened local) had no arm
# for these and raised NoMethodError naming the very class that defines them.
a = [[3, 1, 2], nil][0]
p a.minmax
p a.tally
p a.product([1])
p a.combination(2).to_a
p a.partition { |x| x > 1 }
p a.group_by { |x| x.odd? }
p a.each_with_object([]) { |x, m| m << x }
p a.chunk_while { |x, y| y > x }.to_a
p a.slice_when { |x, y| y < x }.to_a

h = [{ a: 1, b: 2 }, nil][0]
p h.sum { |k, v| v }
p h.each_with_object({}) { |(k, v), m| m[v] = k }
p h.minmax
p h.group_by { |k, v| v.odd? }

r = [(1..5), nil][0]
p r.minmax
p r.tally.size

n = [42, nil][0]
p n.digits
p n.pred
p n.bit_length
p n.ceildiv(5)
p n.pow(2)
p n.gcdlcm(28)
p n.size

# the typed receivers keep answering as before
p [3, 1, 2].minmax
p({ a: 1 }.sum { |k, v| v })
p 42.digits

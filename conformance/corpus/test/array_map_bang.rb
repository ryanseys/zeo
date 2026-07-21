# Array#map! / #collect! transform each element in place and return the
# (mutated) receiver. Typed arrays are homogeneous, so the block must
# keep the element type; poly arrays accept any result.
a = [1, 2, 3]
r = a.map! { |x| x * 10 }
p a
p r

b = ["a", "b", "c"]
b.collect! { |s| s.upcase }
p b

c = [1.5, 2.5]
c.map! { |x| x + 1.0 }
p c

g = [1, "two", 3]
g.map! { |x| x }
p g

s = [:a, :b, :c]
s.map! { |x| x }
p s

# Empty array.
e = []
e.map! { |x| x * 2 }
p e

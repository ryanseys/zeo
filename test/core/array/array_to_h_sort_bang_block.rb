# #1224 I1: Array#to_h builds a hash from [key, value] pairs. Verified
# via lookups + length (Spinel's hash #inspect format is a separate,
# pre-existing concern and would only make this test fragile).
h1 = [[:a, 1], [:b, 2], [:c, 3]].to_h
p h1[:a]
p h1[:c]
p h1.length
h2 = [["x", 10], ["y", 20]].to_h
p h2["x"]
p h2["y"]
h3 = [[1, "one"], [2, "two"]].to_h
p h3[1]
p h3[2]

# #1224 I2: Array#sort! with a comparator block sorts in place (the
# block's <=> return drives the order); the block was previously ignored
# and a default ascending sort happened.
a = [3, 1, 4, 1, 5]
a.sort! { |x, y| y <=> x }
p a
b = [3, 1, 4, 1, 5]
b.sort! { |x, y| x <=> y }
p b
s = ["bb", "a", "ccc"]
s.sort! { |x, y| x.length <=> y.length }
p s
__END__
1
3
3
10
20
"one"
"two"
[5, 4, 3, 1, 1]
[1, 1, 3, 4, 5]
["a", "bb", "ccc"]

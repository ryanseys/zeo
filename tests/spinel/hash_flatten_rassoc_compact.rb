# Hash#flatten / #rassoc / #compact on typed hashes. flatten interleaves
# keys and values into a flat boxed array; rassoc finds the first pair by
# value (nil on a miss); compact drops nil-valued entries while preserving
# the hash variant. compact is checked via lookups rather than #inspect,
# whose symbol-key spacing differs across Ruby versions.
h = {a: 1, b: 2, c: 3}
p h.flatten
p h.rassoc(2)
p h.rassoc(99)

s = {"x" => 10, "y" => 20}
p s.flatten
p s.rassoc(20)
p s.rassoc(0)

ss = {a: "one", b: "two"}
p ss.flatten
p ss.rassoc("two")

# compact on a poly-valued hash drops the nil entry.
pp = {a: 1, b: nil, c: 3}
c = pp.compact
p c.size
p c[:a]
p c.key?(:b)
p c[:c]

# compact on a hash whose values can't be nil is a straight copy.
ii = {1 => 10, 2 => 20}
ci = ii.compact
p ci.size
p ci[1]
p ci[2]

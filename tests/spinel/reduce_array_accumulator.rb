# `acc + elem` on an array accumulator answers a BOXED array -- the concat runs
# through the poly adder -- which cannot be assigned to the array pointer the
# accumulator slot holds, so the C would not compile at all.
g = [[1, 2], [3, 4]]
p g.reduce([]) { |acc, r| acc + r }
p g.inject([]) { |acc, r| acc + r }

p({ a: [1, 2], b: [3] }.reduce([]) { |acc, (_k, v)| acc + v })

p [[1], [2, 3]].reduce([]) { |acc, r| acc + r }
p [["a"], ["b"]].reduce([]) { |acc, r| acc + r }
p [[1, 2]].reduce([9]) { |acc, r| acc + r }

# the scalar accumulators are unchanged
p [1, 2, 3].reduce(0) { |a, x| a + x }
p [1, 2, 3].reduce("") { |a, x| a + x.to_s }
p [1, 2, 3].reduce(:+)

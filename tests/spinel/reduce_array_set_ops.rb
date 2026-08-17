# Folding an array of arrays with `&` or `|`: the block parameters are boxed
# when the receiver is a local, and the poly operator read them as integer
# bitwise arithmetic, so the fold answered 0 (#3966).
l = [[1, 2], [2, 3], [2, 5]]
p l.reduce { |a, b| a & b }
p l.reduce { |a, b| a | b }
p l.inject { |a, b| a & b }
p l.reduce([1, 2, 9]) { |a, b| a & b }
p l.reduce([]) { |a, b| a | b }
s = [%w[a b], %w[b c]]
p s.reduce { |a, b| a & b }
p s.reduce { |a, b| a | b }
m = [[1, 2], ["a", 2]]
p m.reduce { |a, b| a & b }
p [1, 3, 7].reduce { |a, b| a & b }
p [1, 2, 4].reduce { |a, b| a | b }
p [true, true].reduce { |a, b| a & b }
p [[1, 2]].reduce { |a, b| a & b }
p l.reduce(:&)
p l.reduce(:|)

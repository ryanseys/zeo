# An empty `Range#sum { block }` whose block value is poly (a product of
# values read from nested poly containers) must accumulate via the block, not
# drop it and emit a block-less int-array sum.
aug = [[2.0, 3.0, 0.0]]
x = Array.new(1, 0.0)
s = (1...1).sum { |j| aug[0][j] * x[j] }
x[0] = (aug[0][1] - s) / aug[0][0]
p x
# non-empty poly-block range sum
t = (0...2).sum { |j| aug[0][j] * 2 }
p t

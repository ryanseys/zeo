# `(1...1).sum { ... }` answers the block's zero rather than dropping the block.
# (spinel issue #2913)
aug = [[2.0, 3.0, 0.0]]
x = Array.new(1, 0.0)
s = (1...1).sum { |j| aug[0][j] * x[j] }
x[0] = (aug[0][1] - s) / aug[0][0]
p x
# non-empty poly-block range sum
t = (0...2).sum { |j| aug[0][j] * 2 }
p t
__END__
[1.5]
10.0

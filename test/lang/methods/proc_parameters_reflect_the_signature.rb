p proc { |x, y| }.parameters
p lambda { |x, y| }.parameters
p proc { |a, b = 1, *c, d, k:, m: 2, **n, &blk| }.parameters
p ->(a, b) { }.parameters
p proc { |*| }.parameters
p proc { |**| }.parameters
__END__
[[:opt, :x], [:opt, :y]]
[[:req, :x], [:req, :y]]
[[:opt, :a], [:opt, :b], [:rest, :c], [:opt, :d], [:keyreq, :k], [:key, :m], [:keyrest, :n], [:block, :blk]]
[[:req, :a], [:req, :b]]
[[:rest, :*]]
[[:keyrest, :**]]

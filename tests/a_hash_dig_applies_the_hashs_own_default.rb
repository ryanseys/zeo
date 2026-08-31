p Hash.new(0).dig(:x)
p Hash.new { |h, k| "blk#{k}" }.dig(:x)

h = Hash.new { |hh, k| hh[k] = Hash.new(9) }
p h.dig(:a, :b)

p Hash.new({ z: 3 }).dig(:x, :z)
p({ a: { b: 1 } }.dig(:a, :b))
p({ a: 1 }.dig(:b))
p [1, 2].dig(5)

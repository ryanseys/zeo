# Blockless Kernel#loop yields an infinite Enumerator; with_index over a
# generator-backed enum is lazy, and Enumerator#find drives via #next so
# infinite sources terminate on the block's break and finite sources
# return nil on no match (#3236).
r = loop.with_index.find { |_, i| break i if i == 3 }
p r
e = loop
p e.class
a = [10, 20, 30].each
p a.find { |v| v > 15 }
p a.rewind.find { |v| v > 99 }
p [1, 2, 3].each.with_index.find { |v, i| v + i == 5 }

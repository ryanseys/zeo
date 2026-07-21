# Hash#fetch(key) { |k| ... } yields the looked-up key to the block on a
# miss; on a hit it returns the stored value and the block is not run.
# (int-keyed hashes; the param is the int key.)
h = { 1 => 10, 2 => 20 }
puts h.fetch(2) { |k| k * 100 }
puts h.fetch(99) { |k| k * 100 }
counts = { 5 => 1 }
puts counts.fetch(7) { |missing| missing + 1 }
puts "done"

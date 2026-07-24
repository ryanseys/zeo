# Hash#map on sym_int_hash with a block. Pre-fix the receiver
# fell through to a stale `_t = 0` IntArray pointer and crashed
# on .inspect.
h = {a: 1, b: 2}
puts h.map { |k, v| v * 10 }.inspect
puts h.map { |k, v| "#{k}=#{v}" }.inspect

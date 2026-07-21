# Hash#transform_keys with a sym → string block: result is a
# str_*_hash (key-type swap). Same-type blocks preserve the variant.
h = {a: 1, b: 2}
r1 = h.transform_keys { |k| k.to_s }
puts r1.inspect
puts r1["a"]
puts r1["b"]

# Same-type block: sym → sym keeps sym_int_hash
r2 = {a: 1, b: 2}.transform_keys { |k| k }
puts r2.inspect
puts r2[:a]

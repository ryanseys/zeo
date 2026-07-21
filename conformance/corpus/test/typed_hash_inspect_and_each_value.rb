# Issue #851: typed-hash inspect for sym_int_hash, plus
# Hash#each_value across common typed hashes.
h = {a: 1, b: 2}
puts h.inspect
h.each_value { |v| puts v * 10 }

# str_int_hash.
counts = {"x" => 5, "y" => 7}
counts.each_value { |v| puts v + 1 }

# inspect on str_int_hash.
puts counts.inspect

# Hash#[] op= for string-valued and poly-valued typed hashes.

# sym_str_hash: += str → concat.
h = {a: "hello", b: "world"}
h[:a] += "!"
puts h[:a]

# str_str_hash: += str → concat.
h2 = {"a" => "hello", "b" => "world"}
h2["a"] += "!"
puts h2["a"]

# sym_poly_hash: += int on a hash whose values are mixed.
h3 = {a: 1, b: 2.0}
h3[:a] += 10
puts h3[:a]

# str_poly_hash: += int.
h4 = {"a" => 1, "b" => 2.0}
h4["a"] += 10
puts h4["a"]

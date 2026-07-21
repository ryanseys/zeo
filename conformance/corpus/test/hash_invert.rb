# Hash#invert — swap keys and values.
# str_int_hash → poly_poly_hash, str_str_hash → str_str_hash,
# sym_int_hash → poly_poly_hash, int_str_hash → str_str_hash.
# All typed hash variants must compile and produce correct output.

h1 = {"a" => 1, "b" => 2}
p h1.invert

h2 = {a: 1, b: 2}
p h2.invert

h3 = {"a" => "b", "c" => "d"}
p h3.invert

h4 = {1 => "a", 2 => "b"}
p h4.invert

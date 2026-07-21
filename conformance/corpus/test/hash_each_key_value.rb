# Hash#each_key / #each_value with a block across every typed-hash
# variant spinel offers. Keys and values are yielded in insertion
# order. Poly (boxed-object) slots are printed via to_s so array
# payloads render on a single line.

# sym_int_hash
h_sym_int = { a: 1, b: 2, c: 3 }
h_sym_int.each_key { |k| puts k }
puts "-"
h_sym_int.each_value { |v| puts v }
puts "=="

# sym_str_hash
h_sym_str = { x: "one", y: "two" }
h_sym_str.each_key { |k| puts k }
puts "-"
h_sym_str.each_value { |v| puts v }
puts "=="

# sym_poly_hash (mixed scalar values box to sp_RbVal)
h_sym_poly = { a: 1, b: "two" }
h_sym_poly.each_key { |k| puts k }
puts "-"
h_sym_poly.each_value { |v| puts v.to_s }
puts "=="

# sym_poly_hash (object values)
h_sym_poly_obj = { a: [1], b: [2, 3] }
h_sym_poly_obj.each_key { |k| puts k }
puts "-"
h_sym_poly_obj.each_value { |v| puts v.to_s }
puts "=="

# str_int_hash
h_str_int = { "a" => 1, "b" => 2 }
h_str_int.each_key { |k| puts k }
puts "-"
h_str_int.each_value { |v| puts v }
puts "=="

# str_str_hash
h_str_str = { "a" => "x", "b" => "y" }
h_str_str.each_key { |k| puts k }
puts "-"
h_str_str.each_value { |v| puts v }
puts "=="

# str_poly_hash (mixed scalar values)
h_str_poly = { "a" => 1, "b" => "two" }
h_str_poly.each_key { |k| puts k }
puts "-"
h_str_poly.each_value { |v| puts v.to_s }
puts "=="

# str_poly_hash (object values)
h_str_poly_obj = { "a" => [1], "b" => [2, 3] }
h_str_poly_obj.each_key { |k| puts k }
puts "-"
h_str_poly_obj.each_value { |v| puts v.to_s }
puts "=="

# int_str_hash
h_int_str = { 1 => "one", 2 => "two" }
h_int_str.each_key { |k| puts k }
puts "-"
h_int_str.each_value { |v| puts v }
puts "=="

# int_int_hash
h_int_int = { 1 => 10, 2 => 20 }
h_int_int.each_key { |k| puts k }
puts "-"
h_int_int.each_value { |v| puts v }
puts "=="

# poly_poly_hash (heterogeneous object keys and values)
h_poly_poly = {}
h_poly_poly[[1, 2]] = "ab"
h_poly_poly[[3, 4]] = "cd"
h_poly_poly["s"] = 99
h_poly_poly[5] = [10, 20]
h_poly_poly.each_key { |k| puts k.to_s }
puts "-"
h_poly_poly.each_value { |v| puts v.to_s }
puts "=="

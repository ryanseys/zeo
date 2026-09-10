# Hash#each_key and #each_value with a block, over hashes of every key and
# value type. Keys and values come out in insertion order. A value that is
# itself a container prints through to_s, so it renders on one line.

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
__END__
a
b
c
-
1
2
3
==
x
y
-
one
two
==
a
b
-
1
two
==
a
b
-
[1]
[2, 3]
==
a
b
-
1
2
==
a
b
-
x
y
==
a
b
-
1
two
==
a
b
-
[1]
[2, 3]
==
1
2
-
one
two
==
1
2
-
10
20
==
[1, 2]
[3, 4]
s
5
-
ab
cd
99
[10, 20]
==

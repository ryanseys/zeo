h_sym_poly = { a: "x", b: 1, c: :sym }
p h_sym_poly
puts h_sym_poly.inspect

h_str_poly = { "a" => "x", "b" => 1, "c" => :sym }
p h_str_poly
puts h_str_poly.inspect

h_sym_poly2 = { a: nil, b: 1 }
p h_sym_poly2

h_str_poly2 = { "k" => nil, "n" => 1 }
p h_str_poly2

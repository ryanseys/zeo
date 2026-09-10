h_si = { a: 1, b: 2, c: 3 }
v1 = h_si.delete(:a)
puts v1
puts h_si.size

h_ss = { x: "one", y: "two" }
v2 = h_ss.delete(:x)
puts v2
puts h_ss.size

# Deleting a key the hash does not hold answers nil, whatever the value type
# of the keys it does hold.
h2 = { a: 1 }
puts h2.delete(:b)
puts h2.size

# Multi-value sym_poly_hash
h_sp = { a: 1, b: "two", c: :sym }
v3 = h_sp.delete(:b)
puts v3.inspect
puts h_sp.size
__END__
1
2
one
1

1
"two"
2

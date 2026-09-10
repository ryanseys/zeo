# Hash#value? and its alias has_value? answer true when any value in the hash
# equals the argument, across every value type a hash can hold.

# sym_int_hash
h = { a: 1, b: 2 }
puts h.value?(2)
puts h.value?(99)
puts h.has_value?(1)

# str_str_hash
h2 = { "x" => "alpha", "y" => "beta" }
puts h2.value?("alpha")
puts h2.value?("zzz")

# str_int_hash
h3 = { "one" => 1, "two" => 2 }
puts h3.value?(1)
puts h3.value?(3)
__END__
true
false
true
true
false
true
false

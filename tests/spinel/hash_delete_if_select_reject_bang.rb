# Hash#delete_if / #select! / #reject! on typed hashes mutate
# the receiver in place.

h = {a: 1, b: 2, c: 3, d: 4}
h.delete_if { |k, v| v % 2 == 0 }
puts h.inspect

h2 = {a: 1, b: 2, c: 3}
h2.select! { |k, v| v % 2 == 0 }
puts h2.inspect

h3 = {a: 1, b: 2, c: 3}
h3.reject! { |k, v| v > 1 }
puts h3.inspect

# str_int_hash variant.
h4 = {"x" => 10, "y" => 20, "z" => 30}
h4.delete_if { |k, v| v >= 20 }
puts h4.inspect

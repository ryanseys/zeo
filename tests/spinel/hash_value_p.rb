# Issue #738 (partial -- value? / has_value?). Hash#value?(v) returns
# true if any value in the hash equals v, else false. spinel used to
# fall through to the unresolved-call warning.

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

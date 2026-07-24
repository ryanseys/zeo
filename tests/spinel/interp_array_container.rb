# String interpolation of typed-array containers.
#
# `93d04586` fixed Symbol interpolation (`"#{:foo}"` -> `"foo"`).
# This extends the same idea to typed array containers: `#{arr}`
# previously fell through the default int path and printed the
# pointer as a long long (e.g. "4318536016") instead of CRuby's
# `Array#to_s` shape.
#
# Each Array variant has an existing `sp_*Array_inspect` runtime
# helper that emits the bracketed form `[elem, elem, ...]` byte-
# identical to CRuby; we just route to it from compile_interpolated.

# int_array
ints = [1, 2, 3]
puts "ints: #{ints}"

# str_array
strs = ["a", "b", "c"]
puts "strs: #{strs}"

# sym_array
syms = [:x, :y, :z]
puts "syms: #{syms}"

# float_array
floats = [1.5, 2.5, 3.5]
puts "floats: #{floats}"

# poly_array (heterogeneous)
mix = [1, "two", :three, nil]
puts "mix: #{mix}"

# Empty arrays
puts "empty ints: #{[]}"

# Nested arrays
puts "nested: #{[[1, 2], [3, 4]]}"

# nil interpolation (CRuby: empty string, not "0")
n = nil
puts "n: '#{n}'"

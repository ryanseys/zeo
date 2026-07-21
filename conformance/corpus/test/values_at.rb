# Issue #742 (partial -- values_at). `arr.values_at(i, j, ...)`
# picks elements at the given indices and returns a fresh array of
# the same element type. spinel used to fall through to the
# unresolved-call warning for every array variant.

# sym_array
puts [:a, :b, :c].values_at(0, 2).inspect

# int_array
puts [1, 2, 3, 4, 5].values_at(0, 2, 4).inspect

# str_array
puts ["x", "y", "z"].values_at(1).inspect

# Single index, multiple of same value.
puts [10, 20, 30].values_at(1, 1, 1).inspect

# combination + permutation remain unimplemented and stay out
# of scope for this fix.

# Issue #725. `Array#compact` on a poly_array (heterogeneous elements
# with possible nils) used to fall through to an unresolved-call
# warning and then segfault when the caller read the result as a
# poly_array pointer. The runtime gets sp_PolyArray_compact (drops
# SP_TAG_NIL elements) + a codegen arm so `arr.compact` returns a
# fresh poly_array.

puts [1, nil, 2, nil, 3].compact.inspect

# Heterogeneous (mix of int + string + nil) still works.
a = [1, nil, "two", nil, :three, nil]
puts a.compact.inspect

# No-nil array passes through unchanged.
puts [1, 2, 3].compact.inspect

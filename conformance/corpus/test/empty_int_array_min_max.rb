# Issue #745. `[].min` / `[].max` used to read uninitialized memory
# in `sp_IntArray_min` / `_max` (no empty-array guard before reading
# `a->data[a->start]`). After #832 both return nil (int? sentinel
# SP_INT_NIL); inspect renders as "nil" per MRI.

# `[1].first(0)` produces an empty int-typed array, preserving the
# element type. Both min/max should return safely now.
a = [1].first(0)
puts a.min.inspect
puts a.max.inspect

# Non-empty: unchanged.
b = [3, 1, 4, 1, 5, 9, 2, 6]
puts b.min
puts b.max

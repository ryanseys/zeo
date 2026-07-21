# Issue #654: `arr.map { ... }.join(sep)` on a poly-typed
# receiver (e.g. parameter with `= nil` default that widens to
# sp_RbVal) silently emitted `sp_StrArray * _t = 0` for the map
# result, then `sp_StrArray_join(NULL, sep)` returned empty in
# the standalone repro and segfaulted on the downstream pointer
# deref in real-blog AOT.
#
# Fix: compile_map_expr now has a `rt == "poly"` arm that emits
# runtime tag dispatch on cls_id (PolyArray / IntArray / StrArray
# / FloatArray) and routes the per-element fetch through the
# matching `_get` helper. The block param is sp_RbVal; the
# accumulator picks its variant (StrArray when the block returns
# string, etc.) from the block's last expression's static type.

def joined(arr = nil)
  return "(empty)" if arr.nil?
  arr.map { |x| "v=#{x}" }.join(",")
end

puts joined(["a", "b"])
puts joined(["x"])
puts joined([])
puts joined(nil)

# Heterogeneous (poly-array) elements via the same poly param.
def joined_int(arr = nil)
  return "(none)" if arr.nil?
  arr.map { |x| "n=#{x}" }.join(";")
end

puts joined_int([1, 2, 3])

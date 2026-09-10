# `rows = []` followed by `rows << <container>` (where the pushed
# value is a Hash or another Array, not a primitive) used to leave
# `rows` typed as int_array — sp_IntArray_push then errored with
# "incompatible pointer to integer conversion".
#
# #412 added the int_array → typed-array upgrade in scan_locals's
# push observation for primitive element types (str_array,
# float_array, sym_array, obj_X_ptr_array, poly_array). Container
# element types (Hash, nested Array) fell through to the bottom
# default and stayed at int_array.
#
# What decides the element type is the push: an array of hashes, or an array
# of arrays, is an array whose elements are found at runtime, the same
# as the literal `[{...}, {...}]` inference at
# infer_array_elem_type_from_ids).

def build_hash_rows
  rows = []
  rows << { "k" => "v" }
  rows << { "k" => "v2" }
  rows.length
end

def build_nested_arrays
  outer = []
  inner = [1, 2]
  outer << inner
  outer << [3, 4]
  outer.length
end

puts build_hash_rows         # 2
puts build_nested_arrays     # 2
__END__
2
2

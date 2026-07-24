# A poly-typed receiver indexed by an int? value (from a shipped int_int_hash
# lookup) must dispatch correctly. Before the codegen index-parity fix,
# emit_poly_builtin_dispatch / compile_poly_method_call skipped every arm when
# the index type was int? (not int), collapsing the result to nil. This repro
# needs NO int-array []->int? flip: the int? comes purely from int_int_hash#[].

h = { 0 => 2, 1 => 3 }   # int_int_hash; #[] returns int?
k = h[0]                  # int? (value 2)

# A heterogeneous literal makes `box` a poly_array; its element read is `poly`.
box = [[10, 20, 30, 40, 50], "tail"]
tbl = box[0]              # poly receiver (an int_array at runtime)

p tbl[k]                  # poly recv [] with int? index -> want 30

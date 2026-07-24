# `arr.fill(val)` on a poly_array used to lower as
# `sp_PolyArray_set(arr, i, val)` with the raw scalar `val`. The
# storage is sp_RbVal — passing an unboxed int / str / float
# fails to compile (`incompatible type for argument 3 of
# sp_PolyArray_set`).
#
# Fix: wrap the value via `box_value_to_poly` first when the
# receiver is `poly_array`. Other typed-array fills (int / float
# / str / sym / ptr) keep their raw scalar path.

def make_arr
  arr = [1, "two", 3.14]   # heterogeneous → poly_array
  arr.fill(42)
  arr
end

p make_arr   # [42, 42, 42]

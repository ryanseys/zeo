# #496. `a[1..-2]` returned [] in spinel where CRuby returns
# [a[1], a[2], a[3]]. Codegen emitted `sp_IntArray_slice(arr, 1,
# -2 - 1 + 1)` (length = -2), and the runtime's `len <= 0` early
# return produced an empty array. Fix: new `_slice_range` /
# `_sub_range_r` runtime helpers take (start, end, exclusive)
# and normalize negative end against the collection length
# before computing slice length.
#
# Test covers the four receiver shapes that route through the
# fixed emit sites: int_array, float_array, str_array, and
# string. Exclusive (`...`) and inclusive (`..`) forms both
# exercise negative endpoints.

a = [0, 1, 2, 3, 4]
p a[1..-2]   # inclusive: [1, 2, 3]
p a[1...-1]  # exclusive: [1, 2, 3]
p a[-3..-1]  # both negative: [2, 3, 4]
p a[0..-1]   # whole array: [0, 1, 2, 3, 4]
p a[2..-3]   # single element (-3 -> 2, so a[2..2]): [2]
p a[-4..-2]  # negative start, negative end: [1, 2, 3]

f = [0.5, 1.5, 2.5, 3.5]
p f[1..-2]   # [1.5, 2.5]

s_arr = ["a", "b", "c", "d", "e"]
p s_arr[1..-2]   # ["b", "c", "d"]

s = "hello"
p s[1..-2]   # "ell"
p s[1...-1]  # "ell"
p s[-3..-1]  # "llo"
p s[-4..-2]  # negative start + end: "ell"

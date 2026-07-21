# a[-100] with a 3-element array must not segfault.
# Before the fix, sp_IntArray_get adjusted the negative index but
# didn't bounds-check the result, reading far below the buffer.
# The int? nil-display gap is a known limitation; this test only
# verifies that OOB access doesn't crash.

a = [1, 2, 3]
# these used to segfault — now they return SP_INT_NIL (non-crashing)
a[-100]
a[100]
puts a[-1]
puts a[2]

sa = ["a", "b", "c"]
puts sa[-100].inspect
puts sa[100].inspect

fa = [1.0, 2.0, 3.0]
fa[-100]
fa[100]
puts fa[-1]
puts fa[1]

# Issue #659: 2-arg `recv[start, length]` on a poly receiver.
# The single-arg `[]` dispatcher in emit_poly_builtin_dispatch
# dropped the second argument: each typed-array arm called
# `*_Array_get(p, 0)` and there was no SP_TAG_STR arm at all.
# A String-typed runtime value silently returned the empty
# string, and an Array-typed value returned just element 0
# instead of a length-N slice.
#
# Fix: add a 2-arg branch that
#   (a) emits SP_TAG_STR -> sp_str_sub_range for the String recv
#   (b) emits *_Array_slice for each typed-array recv
# Both only fire when is_poly_ret is set (the result is itself
# either a String or an Array; the concrete-typed dispatch
# already covers single-element returns elsewhere).
#
# Also: sp_poly_puts now iterates Array tags one element per line
# matching MRI's `puts arr` semantics, instead of printing the
# array's inspect form.

def tr(s, length: 30)
  s[0, length]
end

puts tr("Hello world this is a long string")
puts tr([1, 2, 3, 4, 5])
puts tr([10.5, 20.5, 30.5])
puts tr(["a", "b", "c"])

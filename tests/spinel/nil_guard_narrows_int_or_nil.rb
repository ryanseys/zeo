# #550. Mirror of d3ee2b2 (#493) for the nil-vs-not case on
# int|nil locals. After `return -1 if h.nil?`, the rest of the
# scope sees `h` as non-nil; for a local whose writer was a
# known int-or-nil source (String#index, rindex, find_index),
# the non-nil arm is mrb_int. The function's return type
# narrows accordingly, and the trailing `h + 1` lowers via
# integer arithmetic on the unboxed payload instead of routing
# through sp_poly_add.
#
# Cascade: the 0210389 String#index return-type change widened
# all downstream consumers to sp_RbVal. Before this fix, every
# `h = s.index(needle); ...; h + 1` had to be hand-wrapped in a
# pure-int helper (Tep.str_find) to avoid the poly widening.
# With the nil-guard narrow, the idiomatic form works directly.
#
# Discriminator: `lookup` consumes find_or_neg's return as an
# int-typed array index. Pre-fix the function returns sp_RbVal
# and arr[poly] does not compile cleanly (spinel emits no
# implicit poly->int unbox at the index site). Post-fix the
# return is mrb_int and arr[i] is direct integer indexing.

ARR = [10, 20, 30, 40, 50, 60]

def find_or_neg(s, needle)
  h = s.index(needle)
  return -1 if h.nil?
  h + 1
end

def lookup(s, needle)
  i = find_or_neg(s, needle)
  return 0 if i < 0
  ARR[i]
end

puts find_or_neg("abcdef", "b")
puts find_or_neg("abcdef", "z")
puts lookup("abcdef", "b")
puts lookup("abcdef", "a")
puts lookup("abcdef", "z")

# A Hash literal whose keys are the consecutive non-negative
# integers 0..N-1 lowers to an internal Array. The runtime has no
# `int_<X>_hash` slot, so a literal like
#
#   {0 => [...], 1 => [...]}
#
# previously fell through to the str_int_hash default and the
# `[]` lookup miscompiled (warning: cannot resolve call to
# 'length' on int — emitting 0 — followed by a runtime segfault
# when the lookup chain dereferences the literal `0` as a pointer).
#
# Detection is AST-shape-only (IntegerNode keys with literal values
# 0,1,...N-1 in source order). Any deviation (gap key, duplicate
# key, non-integer key, splat) opts back into the regular
# str_int_hash codegen, so existing hash literals are unchanged.

TBL = {0 => [3, 7, 2, 6], 1 => [4, 0, 5, 1]}
puts TBL[0].length      # 4
puts TBL[1][2]          # 5

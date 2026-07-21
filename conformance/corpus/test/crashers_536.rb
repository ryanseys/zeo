# #536. Three crashers:
#
# 1. `[].sample` on an empty IntArray emitted `rand() %
#    sp_IntArray_length(...)` which becomes `rand() % 0` (SIGFPE
#    under -O0, UB elsewhere). The receiver was also evaluated
#    twice, allocating a fresh IntArray on each call. Fix: per-
#    prefix `sp_<X>_sample` helper that guards `len <= 0` and
#    returns the slot's zero value (0 / 0.0 / NULL / sp_box_nil).
#
# 2. `[1,2,3,2].union` (no args) compiled the call as
#    `sp_IntArray_union(arr, 0)`. The runtime then dereferenced
#    the null second-arg pointer. Fix: NULL-guard the second-arg
#    walk in `sp_IntArray_union` / `sp_FloatArray_union` /
#    `sp_StrArray_union` so the no-args form reduces to dedup.
#
# 3. `{ 0 => false, a: 1 }` (mixed IntegerNode + SymbolNode keys)
#    fell through to `str_poly_hash` and codegen handed the
#    integer key directly to `sp_StrPolyHash_set`, which read it
#    as `const char *` -- segfault on the next access. Fix:
#    detect mixed-shape keys and use `poly_poly_hash` so both key
#    and value carry their own tag.

# Empty Array#sample returns nil, matching CRuby (was the zero value; #2322).
puts [].sample.inspect

# No-args Array#union reduces to dedup.
puts [1, 2, 3, 2].union.inspect

# Mixed-key hash literal builds via poly_poly_hash.
h = { 0 => false, a: 1 }
puts h.size
puts h[:a]

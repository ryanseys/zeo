# GAP -- imported from the spinel corpus at fa06b601.
#
# `sort_by` (and its `min_by`/`max_by`/`minmax_by` siblings) raise
# `ArgumentError: comparison failed` for block shapes CRuby sorts.
#
# The keys the block returns compare fine on their own, so the failure is in
# how the sort compares the computed keys rather than in the comparison
# itself -- CRuby sorts by the key with `<=>` and zeo reaches a pair it
# cannot order.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix, not zeo's divergence above.
#
# min_by / max_by / minmax_by / sort_by over block shapes their arms refused.
# Each refusal dropped the call to the unresolved-call gate, so the METHOD was
# reported undefined -- or, where the arm did emit, it emitted C that did not
# compile (#4006's leftovers).
#
# 1. A block with NO parameter. The winning element was read back from `lv_` +
#    the parameter name, and with no parameter that was a bare `lv_`, which is
#    not an identifier: `[3,1,2].min_by { 5 }` never compiled.
p [3, 1, 2].min_by { 5 }
p [3, 1, 2].max_by { 5 }
p [3, 1, 2].minmax_by { 5 }
p [3, 1, 2].sort_by { 5 }

# 2. A key whose value IS nil. It types VOID/NIL, which is not a C type to hold
#    a key in -- but nil is a key: every element ties, so CRuby answers the
#    first one and leaves the order alone.
p [3, 1, 2].min_by {}
p [3, 1, 2].max_by {}
p [3, 1, 2].minmax_by {}
p [3, 1, 2].sort_by {}
p [3, 1, 2].min_by { |x| nil }
p [3, 1, 2].sort_by { |x| nil }
a = [3, 1, 2]
a.sort_by! {}
p a

# minmax_by with a BOXED key (String, Array, nil) fell through to the
# single-winner pass, which answers a bare min rather than [min, max]
p [3, 1, 2].minmax_by { |x| x.to_s }
p [3, 1, 2].minmax_by { |x| [x] }
p ["bb", "a", "ccc"].minmax_by { |s| s.length }

# nil orders against nil and only against nil: CRuby's NilClass defines #<=>
# and not #<=, so these answer while a mixed pair still raises
b = [nil, nil]
p b.min
p b.max
p b.sort
begin
  p [nil, 1].min
rescue ArgumentError => e
  p e.class
end

# the ordinary forms keep working
p [3, 1, 2].min_by { |x| -x }
p [3, 1, 2].sort_by { |x| -x }
p [3, 1, 2].minmax_by { |x| x }
p ["bb", "a", "ccc"].sort_by { |s| s.length }

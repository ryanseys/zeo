# `min_by` / `max_by` / `minmax_by` / `sort_by` over the block shapes their
# arms once refused: a block with NO parameter, a key that IS nil, a key
# whose type differs per element, and a key CRuby cannot order.
#
# The last one to close was the nil key, and it was not the by-key drivers
# at all: `rb_cmp` had no `Object#<=>` fallback, so a pair of `nil`s was
# INCOMPARABLE. Every element ties on nil, so `sort_by {}` raised
# "comparison failed" -- and so did `[nil, nil].sort`, `[true, true].sort`
# and any other pair of equal values with no ordering arm of its own.

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

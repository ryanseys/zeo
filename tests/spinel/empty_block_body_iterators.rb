# `{ }` and `{ nil }` are the same block, and CRuby answers accordingly. The
# iterator arms each read the block body's TAIL to type their result, and most
# of them declined outright when there was none -- so the call fell through to
# the unresolved-call gate and reported the METHOD as undefined, which is the
# one thing it is not (#4006):
#
#   p [].filter_map{}   # undefined method 'filter_map' for an instance of Array
#
# An empty body now carries the nil it means, once, rather than a dozen arms
# each special-casing it.
p [].filter_map {}
p [1, 2].filter_map {}
p [1, 2].select {}
p [1, 2].reject {}
p [1, 2].find {}
p [1, 2].detect {}
p [1, 2].flat_map {}
p [1, 2].group_by {}
p [1, 2].partition {}
p [1, 2].take_while {}
p [1, 2].drop_while {}
p [1, 2].map {}
p [1, 2].each {}
p [1, 2].each_with_index {}
p [1, 2].each_with_object([]) {}
p [1, 2].count {}
p [1, 2].all? {}
p [1, 2].any? {}
p [1, 2].none? {}

# a nil body is a VALUE in a predicate slot, not the absence of one: `void _tN`
# is not a declaration, so `select { nil }` never compiled either
p [1, 2].select { nil }
p [1, 2].reject { nil }
p [1, 2].map { nil }
p [1, 2].count { nil }

# sum accumulates BOXED here: the answer is the init for an empty receiver and
# a TypeError for a non-empty one, never nil -- typing it nil folded the call
# away and printed "nil" for `[].sum {}`
p [].sum {}
begin
  p [1].sum {}
rescue TypeError => e
  p e.class
end

# the same through other receivers
p({ a: 1 }.filter_map {})
p({ a: 1 }.group_by {})
p (1..3).filter_map {}
p (1..3).select {}

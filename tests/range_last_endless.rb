# `last(n)` on an endless range raises rather than trying to reach a tail that
# is not there. The guard has to live on Range's own row: `last(n)` answers by
# materializing, and the `to_a` it used to reach went through Enumerable, which
# walks `each` and so routed straight past Range's endless check.
#
# `first(n)` and `min` are the counterpart -- they count UP from begin, so an
# endless range answers them.
r = ((1..).last(2) rescue $!.class)
p r
p((1..).first(2))
p((1..).min)
p((1..5).last(2))
p((1..5).last)
p((1...5).last(2))

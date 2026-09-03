# This golden reads `GC.stat[:heap_live_slots]`, which only the allocation
# registry can answer, and only `ZEO_GC=1` arms it. CRuby always tracks, so the
# oracle needs no counterpart.
#@ zeo-env: ZEO_GC=1
# The boxed-argument and result channels a proc call publishes through were GC
# roots nothing cleared, so the last call's arguments and result stayed
# reachable for the rest of the program. The callee reads them out and drops
# the references now.
#
# `GC.stat[:heap_live_slots]` is what makes that measurable rather than merely
# asserted, and it is why this golden carries a `.gc` sidecar: the count comes
# from the allocation registry, which only `ZEO_GC=1` arms. Without it the key
# is absent, because a heap with no record of an allocation cannot answer
# truthfully and inventing a number is worse than the `nil` a caller can read
# as "this heap cannot say".
#
# A direct liveness test would not be portable in the other direction: zeo's
# finalizers fire promptly where CRuby's have not run yet, so the honest
# version of THAT test fails on ruby. A slot count is the portable spelling of
# the same property -- a dropped structure stops being live.
#
# Spinel's original read `GC.stat["bytes"]`. CRuby's `GC.stat` is keyed by
# SYMBOL and has no "bytes" statistic, so a String subscript is nil there and
# the comparison raises `NoMethodError` on ruby too.
def build(len)
  n = len.to_i
  a = []
  i = 0
  while i < n
    a << [i, i]
    i += 1
  end
  a
end

sink = proc { |x| x.size }

def feed(s)
  big = build(120_000)
  s.call(big) == 120_000
end

base = GC.stat[:heap_live_slots]
p feed(sink)
GC.start
GC.start
# the argument is gone, not merely unreferenced by the program
p GC.stat[:heap_live_slots] < base + 100_000

# the result channel holds its value only until the next call, not forever
maker = proc { |n| build(n) }
def take(m)
  m.call(60_000).size == 60_000
end
b2 = GC.stat[:heap_live_slots]
p take(maker)
noop = proc { |z| z }
p noop.call(1) == 1
GC.start
GC.start
p GC.stat[:heap_live_slots] < b2 + 100_000

# the arguments themselves still arrive intact
two = proc { |a, b| [a, b] }
p two.call("x", [1, 2])
rest = proc { |*a| a }
p rest.call(1, "b", :c)
nested = proc { |v| two.call(v, v) }
p nested.call(7)
__END__
true
true
true
true
true
["x", [1, 2]]
[1, "b", :c]
[7, 7]

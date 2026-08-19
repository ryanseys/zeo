# GAP -- imported from the spinel corpus at c55d9bdb.
#
# What this file was FILED for is fixed: the boxed-argument and result
# channels a proc call publishes through were GC roots nothing cleared, so the
# last call's arguments and result stayed reachable for the rest of the
# program. The callee reads them out and drops the references, and an
# `ObjectSpace.define_finalizer` on the argument now fires after the call --
# checked directly, which is the only way to see it without a slot count.
#
# What is left is `GC.stat[:heap_live_slots]`, which is nil: zeo is
# `Arc`-refcounted with no collector, and `GC` answers nil for every statistic
# this heap has no truthful value for rather than inventing one (see
# `builtins/gc.rs`'s module docs and docs/COMPATIBILITY.md). Reporting a live
# count truthfully means an increment on every heap allocation and a decrement
# in every payload's `Drop` -- a real cost on exactly the path zeo already
# loses to CRuby on, for a statistic almost nothing reads.
#
# It cannot be rewritten around either: any direct liveness test diverges
# between a refcounting and a tracing heap in the other direction. zeo's
# finalizer fires promptly where CRuby's has not run yet, so the honest
# version of this test would fail on ruby.
#
# Spinel's version read `GC.stat["bytes"]`. CRuby's GC.stat is keyed by SYMBOL
# and has no "bytes" statistic, so a String subscript there is nil and the
# comparison raises NoMethodError there too. The portable spelling of the same
# property -- a dropped structure stops being live -- is the live-slot count.
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

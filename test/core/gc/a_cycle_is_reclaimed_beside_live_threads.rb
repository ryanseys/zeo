# This golden needs the cycle collector armed: without `ZEO_GC=1` zeo has no
# collector at all and both counts read 2. CRuby always collects, so the oracle
# needs no counterpart.
#@ zeo-env: ZEO_GC=1
# Reference-count reconciliation reads every node's owner count and compares
# it against the edges it can enumerate. A thread mutating the heap while that
# runs would make the two disagree, and the pass would read a live node as
# garbage -- the one direction that can corrupt. So a collection stops every
# other Ruby thread first, and before this it simply refused to run whenever
# it was not alone.
#
# Both shapes below leaked under `ZEO_GC=1`: one busy worker thread was enough
# to make `GC.start` a no-op.
#
# The threads park at a `check_ints` checkpoint, never at an allocation site.
# An allocation can happen inside a container's own guard -- growing a Hash
# while its lock is held -- and stopping the world there would hand the
# collector a locked node it has to read.
#
# A request that cannot stop the world in time is ABANDONED rather than
# waited on, which is why the elapsed check is here: a thread that never
# reaches a checkpoint must cost a bounded pause and a missed collection, not
# a hang. That is exactly what leaked before, so it is never a regression.
class Node
  attr_accessor :peer
end

def scrub(n) = n.zero? ? [0] * 512 : scrub(n - 1)

def pair(seen)
  a = Node.new
  b = Node.new
  a.peer = b
  b.peer = a
  seen[a] = true
  seen[b] = true
  nil
end

def timed
  t0 = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  yield
  Process.clock_gettime(Process::CLOCK_MONOTONIC) - t0
end

# Busy workers: they reach a checkpoint on every loop iteration.
busy = ObjectSpace::WeakMap.new
workers = 3.times.map { Thread.new { 200_000.times.sum { |j| j % 7 } } }
pair(busy)
scrub(60)
elapsed = timed do
  GC.start
  GC.start
end
puts "busy collected=#{busy.size == 0} bounded=#{elapsed < 3.0}"
p workers.map(&:value).uniq.size

# A sleeping thread parks too: `Kernel#sleep` re-checks its interrupts on
# every wake, so the request reaches it immediately rather than at its
# deadline.
sleeping = ObjectSpace::WeakMap.new
napper = Thread.new { sleep 3 }
pair(sleeping)
scrub(60)
elapsed = timed do
  GC.start
  GC.start
end
puts "sleeping collected=#{sleeping.size == 0} bounded=#{elapsed < 3.0}"
napper.join
__END__
busy collected=true bounded=true
1
sleeping collected=true bounded=true

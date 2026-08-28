# Creating the first Thread sizes the object-heap collection budget for the
# worker pool. It used to MULTIPLY the current threshold by the pool size --
# and the pool is min(cores, SPINEL_WORKERS), not the thread count the program
# asked for, so the damage scaled with the machine. A program that had already
# grown its heap before starting a thread got N x whatever the adaptive
# threshold had become: 76MB -> 2.4GB on a 32-core box, with no collection
# involved, and the churn that followed never collected again (#4146).
#
# Asserted as properties rather than numbers: the absolute threshold depends on
# the core count and the allocator's own tuning, and pinning it would make this
# a test of the machine.

def th = GC.stat["threshold"]
def cyc = GC.stat["cycle"]

# grow the heap first, so the adaptive threshold is well above its base
keep = []
600.times { |i| keep << Array.new(4096, i * 1.0) }
before = th

Thread.new { 1 + 1 }.join
after = th

# The thread allocated nothing, so the threshold must not have been scaled by
# the pool size. Some headroom for the workers is intended; a multiple of the
# GROWN threshold is not.
p after >= before
p after < before * 2

# And collection still happens against it: churn well past the threshold and
# the cycle count has to move.
c0 = cyc
3000.times do
  a = Array.new(8192, 1.0)
  a[0] = 2.0
end
p cyc > c0

# The live set is what it was, so the heap is bounded near it rather than
# climbing to the ceiling.
p GC.stat["bytes"] < before * 4

p keep.length
p keep[0].length

# The collector recomputes its live-byte counter from the live set and THEN
# sweeps, so a dead object's finalizer subtracted buffer bytes the fresh total
# had never counted. Churning large arrays walked the counter below zero; the
# retune read the wrapped value and set a threshold near SIZE_MAX, after which
# `bytes >= threshold` never fires again -- the collector switched itself off
# for the life of the process and RSS grew without bound (#4073).
#
# The live set here is a handful of tiny pairs. What matters is that the
# threshold stays a real number and the heap stays bounded. GC.stat's keys are
# spinel's own, like gc_stat_string_heap.rb's.
N     = 20_000
ITERS = 400

live = []
ITERS.times do |i|
  s = Array.new(N, 0.0)
  sbin = Array.new(N, 0)
  cnt = Array.new(256, 0)
  j = 0
  while j < N
    b = (j & 255)
    sbin[j] = b
    s[j] = b * 0.5
    cnt[b] += 1
    j += 197
  end
  live.push([cnt[0], s[0]]) if (i % 64).zero?
end

g = GC.stat
puts "retained: " + live.length.to_s
puts "threshold positive: " + (g["threshold"] > 0 ? "yes" : "no")
puts "collector still running: " + (g["cycle"] > 8 ? "yes" : "no")
puts "heap bounded: " + (g["bytes"] < 400_000_000 ? "yes" : "no")

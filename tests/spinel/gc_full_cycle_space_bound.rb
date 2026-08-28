# The adaptive full-cycle interval bounds the TIME between full sweeps and
# nothing bounded the SPACE. Once the cadence has ratcheted up, a phase that
# promotes heavily piles garbage onto an old list nothing walks: full_runs
# stops, old_bytes climbs to many times the live set, and the object threshold
# retunes off that inflated total (#4076). Two phases are needed -- the first
# ratchets the cadence honestly, the second changes the promotion rate under it.
RETAIN = 1_000
WARM   = 200_000
N      = 8_000
HOLD   = 16
ITERS  = 400

retained = []
RETAIN.times { |i| retained.push(Array.new(64, i * 1.0)) }
WARM.times { Array.new(64, 0.0) }

ITERS.times do |i|
  bufs = []
  HOLD.times { |h| bufs.push(Array.new(N, h * 1.0)) }
  j = 0
  while j < N
    bufs[j % HOLD][j] += 1.0
    j += 397
  end
  bufs = nil
end

# Before the space bound this run reached full=5 with old at 14.7 MB; with it,
# full=8 and old at 5.9 MB against a live set of about 1 MB.
g = GC.stat
puts "live set: " + retained.length.to_s
puts "full sweeps kept running: " + (g["full_runs"] > 6 ? "yes" : "no")
puts "old generation bounded: " + (g["old_bytes"] < 10_000_000 ? "yes" : "no")

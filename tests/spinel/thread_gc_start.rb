# An explicit GC.start in a program that has run threads. The collection has to
# take the stop-the-world barrier: its parallel sweep hands one slot to each
# other worker and waits for them, which only happens if they are parked at the
# barrier. Run straight from the mutator, the collector waited on sweeps nobody
# would do and the program hung.

t = Thread.new { 1 + 1 }
t.join
GC.start
p :after_join

def work(n, threads)
  c = Array.new(n) { Array.new(n, 0.0) }
  mutex = Mutex.new
  done = 0
  threads.times.map do |w|
    Thread.new do
      (0...n).each { |i| c[i][w % n] = (i * w).to_f }
      mutex.synchronize { done += 1 }
    end
  end.each(&:join)
  done
end

3.times do
  p work(8, 4)
  GC.start
  junk = (1..200).map { |i| [i.to_s, i] }
  GC.start
  p junk.length
end

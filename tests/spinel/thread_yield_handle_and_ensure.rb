# A Mutex yielded to a block from inside Thread.new. Two separate faults met
# here: the proc ABI packs arguments into an mrb_int[] and only knew how to
# launder strings, arrays, hashes and objects through it -- every runtime
# handle (Mutex, IO, Thread, Queue, Dir, ...) went in and came back out with no
# cast at all -- and the ensure funnel that Mutex#synchronize emits returned an
# sp_RbVal straight out of the mrb_int proc function.
def race(label)
  h = { "a" => 0 }
  lock = Mutex.new
  ts = 2.times.map do
    Thread.new do
      yield(lock, h)
    end
  end
  ts.each(&:join)
  puts "#{label}: #{h["a"]}"
end

race("counter") { |lock, h| lock.synchronize { h["a"] += 1 } }

# the same handle yielded WITHOUT a thread still works
def direct
  lock = Mutex.new
  yield(lock)
end
direct { |l| puts l.class.to_s }

# other handle kinds through the same path
def with_queue
  q = Queue.new
  yield(q)
  q.size
end
puts with_queue { |q| q.push(1); q.push(2) }

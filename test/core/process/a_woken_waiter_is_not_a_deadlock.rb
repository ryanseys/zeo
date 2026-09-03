# A wake is progress. The deadlock detector must not call a thread blocked
# once its condition is already true.
#
# The verdict asks whether every LIVE ruby thread is registered in a wait
# only another ruby thread can end. A woken thread stays registered for as
# long as the scheduler takes to run it -- and on a busy machine that is
# milliseconds. So a `SizedQueue(1)` with one producer and one consumer
# reads as "both blocked" in the gap between the pop that frees a slot and
# the producer actually running again, and the run reported `fatal: No
# live threads left. Deadlock?` for a program that was working.
#
# It surfaced as a golden that passed alone and failed under a full
# sixteen-job suite, which is exactly the shape a timing verdict fails in.
# Every wake site now says so (`note_progress`), and the verdict needs
# eight consecutive all-blocked polls with no wake between them. A real
# deadlock has nobody to wake it EVER, so the longer streak costs a true
# verdict nothing.
#
# The rows below are the shapes that hand a wait back and forth many times
# over. None of them is a deadlock and none may report one.

results = []

# A one-slot queue: every push waits for a pop and every pop for a push.
q = SizedQueue.new(1)
consumer = Thread.new { 200.times.map { q.pop } }
200.times { |i| q << i }
results << consumer.value.sum

# Two threads handing one mutex back and forth.
m = Mutex.new
n = 0
ts = 2.times.map do
  Thread.new { 200.times { m.synchronize { n += 1 } } }
end
ts.each(&:join)
results << n

# A plain Queue drained by two consumers, closed to release them.
work = Queue.new
seen = Queue.new
2.times do
  Thread.new do
    while (job = work.pop)
      seen << job
    end
  end
end
100.times { |i| work << i }
sleep 0.05 until seen.size == 100
work.close
results << seen.size

# A condition variable: the wait is untimed and only a signal ends it.
cv = ConditionVariable.new
lock = Mutex.new
ready = false
waiter = Thread.new do
  lock.synchronize do
    cv.wait(lock) until ready
    :signalled
  end
end
sleep 0.01
lock.synchronize do
  ready = true
  cv.signal
end
results << waiter.value

# A real deadlock is still a deadlock, and still prompt.
verdict = begin
  full = SizedQueue.new(1)
  full << 1
  full << 2
rescue Exception => e
  "#{e.class}: #{e.message.lines.first.chomp}"
end
results << verdict

p results
__END__
[19900, 400, 100, :signalled, "fatal: No live threads left. Deadlock?"]

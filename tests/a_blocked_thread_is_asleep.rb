# A thread waiting on a blocking primitive is "sleep", not "run". zeo used to
# answer "run" for every one of them, because only the `Thread.stop` latch
# said "sleep" -- and that latch cannot be reused, since the stop loop spins
# on it.

# Wait until `t` reaches a settled word, so the reading is not a race.
def settle(t)
  200.times do
    break unless t.status == "run"
    sleep 0.005
  end
  [t.status, t.stop?]
end

q = Queue.new
p settle(Thread.new { q.pop })

m = Mutex.new
m.lock
p settle(Thread.new { m.lock })

p settle(Thread.new { sleep 30 })

r, w = IO.pipe
p settle(Thread.new { r.read(1) })

sq = SizedQueue.new(1)
sq.push(:one)
p settle(Thread.new { sq.push(:two) })

# `Thread.stop` still reads the same way, and so does a running thread.
stopped = Thread.new { Thread.stop }
p settle(stopped)

running = Thread.new { loop {} }
p [running.status, running.stop?]

# A finished thread, for contrast.
done = Thread.new { 1 }
done.join
p [done.status, done.stop?]

# The MAIN thread is not exempt: another thread reading it while it sleeps
# sees "sleep" too.
main = Thread.main
seen = Thread.new do
  sleep 0.005 while main.status == "run"
  main.status
end
sleep 0.2
p seen.value

# A dead thread answers for itself with nobody joining it: killed is false,
# killed by a raise is nil.
killed = Thread.new { sleep 30 }
sleep 0.05
killed.kill
raised = Thread.new { sleep 30 }
raised.report_on_exception = false
sleep 0.05
raised.raise(RuntimeError, "boom")
sleep 0.2
p [killed.status, killed.alive?]
p [raised.status, raised.alive?]

[running, stopped].each(&:kill)

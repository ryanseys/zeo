# Monitor is a REENTRANT mutex, and that is the whole reason it exists: a
# synchronized method calling another synchronized method of the same object
# is the ordinary way to write one. Spinel aliased Monitor to Mutex, so the
# inner acquire raised "deadlock; recursive locking" where CRuby just goes
# deeper. A Mutex must still raise there, so reentrancy is per-object.
require "monitor"

m = Monitor.new
m.synchronize { m.synchronize { m.synchronize { puts "three deep" } } }
puts "released"

# the lock is genuinely released at the outermost exit
m.synchronize { puts "re-acquired" }

# ... and a Mutex still refuses
mx = Mutex.new
begin
  mx.synchronize { mx.synchronize { puts "unreachable" } }
rescue ThreadError => e
  puts e.message
end

# a monitor still excludes another thread
counter = 0
lock = Monitor.new
ts = 4.times.map do
  Thread.new do
    100.times { lock.synchronize { counter += 1 } }
  end
end
ts.each(&:join)
puts counter

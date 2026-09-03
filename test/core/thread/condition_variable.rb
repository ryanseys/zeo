# ConditionVariable — thread coordination paired with a Mutex.

cv = ConditionVariable.new
# (Class#name is `Thread::ConditionVariable` upstream; zeo simplifies the
# nested `Thread::*` sync classes to top-level, exactly as it does for Mutex.)
p cv.class.name.include?("ConditionVariable")   # true

# signal / broadcast return self, and are no-ops with no waiters.
p cv.signal.equal?(cv)             # true
p cv.broadcast.equal?(cv)          # true

# wait with a short timeout returns without a signaller (times out).
m = Mutex.new
m.synchronize { cv.wait(m, 0.01) }
puts "timed out and returned"

# Producer/consumer handoff: the worker waits until the main broadcasts.
mutex = Mutex.new
ready = false
worker = Thread.new do
  mutex.synchronize do
    until ready
      cv.wait(mutex)
    end
  end
  puts "worker observed ready"
end

# Let the worker reach its wait, then flip the flag and wake it.
mutex.synchronize do
  ready = true
  cv.broadcast
end
worker.join
puts "done"
__END__
true
true
true
timed out and returned
worker observed ready
done

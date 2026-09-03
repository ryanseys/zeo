# signal/broadcast return self; wait releases the mutex, parks the thread
# until broadcast, then re-acquires. The handoff is deterministic via the
# shared `ready` flag under the mutex.

cv = ConditionVariable.new
puts cv.signal.equal?(cv)
puts cv.broadcast.equal?(cv)
# a lone wait with a timeout returns (does not hang)
m0 = Mutex.new
m0.synchronize { cv.wait(m0, 0.01) }
puts "timeout-ok"

mutex = Mutex.new
ready = false
worker = Thread.new do
  mutex.synchronize do
    cv.wait(mutex) until ready
  end
  puts "woke"
end
mutex.synchronize do
  ready = true
  cv.broadcast
end
worker.join
puts "joined"
__END__
true
true
timeout-ok
woke
joined

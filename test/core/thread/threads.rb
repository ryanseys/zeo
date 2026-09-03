# Thread / Mutex / Queue -- green-thread concurrency

# join-synchronized ordering
t = Thread.new do
  puts "in thread"
end
t.join
puts "after join"

# constructor args bind to block params; value returns the result
puts Thread.new(20, 22) { |a, b| a + b }.value

# an uncaught exception re-raises at join
bad = Thread.new { raise "thread boom" }
begin
  bad.join
rescue RuntimeError => e
  puts "joined error: #{e.send(:message)}"
end

# a mutex-protected shared counter across two threads
m = Mutex.new
count = 0
t1 = Thread.new { 500.times { m.synchronize { count += 1 } } }
t2 = Thread.new { 500.times { m.synchronize { count += 1 } } }
t1.join
t2.join
puts count

# mutex error semantics
mu = Mutex.new
begin
  mu.unlock
rescue ThreadError => e
  puts e.send(:message)
end
mu.lock
begin
  mu.lock
rescue ThreadError => e
  puts e.send(:message)
end
mu.unlock

# queue producer/consumer rendezvous
q = Queue.new
producer = Thread.new do
  q.push 1
  q.push 2
  q.push 3
  q.close
end
consumer = Thread.new do
  total = 0
  loop do
    v = q.pop
    break if v.nil?
    total += v
  end
  total
end
producer.join
puts consumer.value

# push to a closed queue raises
begin
  q.push 99
rescue ClosedQueueError => e
  puts e.send(:message)
end
__END__
in thread
after join
42
joined error: thread boom
1000
Attempt to unlock a mutex which is not locked
deadlock; recursive locking
6
queue closed
#@ stderr
#<Thread:0xADDR core/thread/threads.rb:14 run> terminated with exception (report_on_exception is true):
core/thread/threads.rb:14:in 'block in <main>': thread boom (RuntimeError)

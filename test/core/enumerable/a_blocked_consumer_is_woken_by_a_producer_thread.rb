# The consumer genuinely BLOCKS on the empty pop (a Condvar wait) and is
# woken by the producer's push -- exercising the real cross-thread wakeup
# path. The value is deterministic regardless of which thread runs first.

q = Queue.new
consumer = Thread.new { q.pop }
producer = Thread.new { q.push 42 }
puts consumer.value
producer.join
__END__
42

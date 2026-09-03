t = Thread.new { 1 }
p t.class
p t.frozen?
p t.nil?
t.join
m = Mutex.new
p m.class, m.nil?
q = Queue.new
p q.class, q.nil?
__END__
Thread
false
false
Thread::Mutex
false
Thread::Queue
false

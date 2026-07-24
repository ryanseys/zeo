# SizedQueue back-pressure and Mutex#try_lock on the cooperative scheduler.

# A SizedQueue(2) whose producer pushes 5 items: #push blocks once two are
# buffered until the consumer pops, so the queue never exceeds its bound.
q = SizedQueue.new(2)
p q.max                    # 2
p q.class                  # SizedQueue
p q.is_a?(Queue)           # true (SizedQueue < Queue)
producer = Thread.new do
  5.times { |i| q.push(i) }
  q.close
end
got = []
while (v = q.pop)
  got << v
end
producer.join
p got                      # [0, 1, 2, 3, 4]

# spare-capacity ops: <<-chaining, size, pop, max
s = SizedQueue.new(3)
p s.empty?                 # true
s << "a" << "b"
p s.size                   # 2
p s.pop                    # "a"
p s.max                    # 3

# max= raises the bound
s.max = 5
p s.max                    # 5

# Mutex#try_lock acquires only when free
m = Mutex.new
p m.try_lock               # true
p m.try_lock               # false (already held)
m.unlock
p m.try_lock               # true again
m.unlock

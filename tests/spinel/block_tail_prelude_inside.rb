# A block emitter that renders its tail as an EXPRESSION must catch that
# expression's statement-shaped setup itself: g_pre at that point is the buffer
# for the whole enclosing line, so anything routed there runs OUTSIDE the
# block. `h[k] += 1` puts its whole read-modify-write in the prelude and leaves
# only the read inside -- under Mutex#synchronize that dropped the update out
# of the critical section, and concurrent increments went missing.
N = 3000

def guarded(n)
  h = { "a" => 0 }
  l = Mutex.new
  q = Queue.new
  n.times { q << 1 }
  4.times { q << nil }
  ts = 4.times.map do
    Thread.new do
      loop do
        v = q.pop
        break if v.nil?
        l.synchronize { h["a"] += 1 }
      end
    end
  end
  ts.each(&:join)
  h["a"]
end

puts guarded(N)

# do...end reaches the same emitter
def guarded_do(n)
  h = { "b" => 0 }
  l = Mutex.new
  n.times { l.synchronize do h["b"] += 1 end }
  h["b"]
end
puts guarded_do(500)

# File.open has the same shape: the setup would run before the file is opened
path = "spinel_blocktail_#{Process.pid}.tmp"
File.write(path, "xyz")
acc = { "n" => 0 }
r = File.open(path) { |f| acc["n"] += f.read.length }
puts r
puts acc["n"]
File.delete(path)

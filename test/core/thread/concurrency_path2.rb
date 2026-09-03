# Queue and Mutex methods on a dynamically-typed (Poly) receiver: held in an
# Array/Hash/ivar so the compiler can't statically know the type and must
# dispatch through the runtime method table (Path-2).

# A Queue stored in an Array element -- `each`'s block param is Poly.
qs = [Queue.new]
qs.each do |q|
  q.push(10)
  q << 20
  q.enq(30)
end
q = qs.first
p q.length
p q.size
p q.empty?
p q.pop
p q.pop
p q.closed?
q.close
p q.closed?
p q.pop            # closed + one left -> 30
p q.pop            # closed + empty -> nil

# A Mutex stored in a Hash value -- Poly receiver again.
locks = { m: Mutex.new }
m = locks[:m]
p m.locked?
p m.owned?
result = m.synchronize do
  p m.locked?
  p m.owned?
  42
end
p result
p m.locked?

# send-based dispatch also lands on Path-2.
p q.send(:size)
p m.send(:locked?)
__END__
3
3
false
10
20
false
true
30
nil
false
false
true
true
42
false
0
false

# A Queue/Mutex held in a collection is a dynamically-typed (Poly)
# receiver, so these methods dispatch through the runtime Path-2 tables
# rather than the static Path-1 codegen arm. Both must agree.

qs = [Queue.new]
qs.each do |q|
  q.push(10)
  q << 20
  q.enq(30)
end
q = qs.first
p q.length
p q.empty?
p q.pop
q.close
p q.closed?
p q.pop
p q.pop

locks = { m: Mutex.new }
m = locks[:m]
p m.locked?
r = m.synchronize do
  p m.owned?
  42
end
p r
p m.locked?
p q.send(:size)
__END__
3
false
10
true
20
30
false
true
42
false
0

# A Struct held in a local is the same class as one held in a constant, so its
# construction types the members the same way. It used to leave every member
# poly, which put each field read and the arithmetic on it through the boxed
# path even when every construction passed the same scalar type.
st = Struct.new :x, :y
a = st.new(1.5, 2.5)
b = st.new(0.5, 4.0)
p [a.x - a.y, b.x + b.y]
p [a.x.class, a.y.class]

pt = Struct.new :name, :count
q = pt.new("a", 2)
p [q.name * q.count, q.count + 1]

# a member left unsupplied is nil, which keeps the member wide
part = Struct.new :a, :b
r = part.new(1)
p [r.a, r.b]

# assignment through the writer still lands
w = st.new(1.0, 2.0)
w.x = 9.5
p w.x + w.y
p w.to_a
p w == st.new(9.5, 2.0)

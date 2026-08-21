# A local holding an anonymous Struct class, reassigned to a SECOND one. No
# single class resolves for it any more, so `s.new` dispatches at run time --
# and the switch had no arm for a Struct at all, so the call answered nil.
# The write that followed then took the nil box's cls_id, which is 0, as a real
# class id and stored through its NULL pointer: a segfault, and the program
# printed nothing (#4048).
s = Struct.new :a
x = s.new
x.a = 0
s = Struct.new
puts "OK"
p x.a

t = Struct.new(:a)
y = t.new
y.a = 7
t = Struct.new(:b)
z = t.new
z.b = 9
p [y.a, z.b]

# a nil receiver for a member write is a NoMethodError, not a crash
begin
  n = [nil, Struct.new(:a).new(1)][0]
  n.a = 5
rescue NoMethodError => e
  p e.class
end

# the single-class local, unchanged
u = Struct.new(:a)
w = u.new
w.a = 3
p w.a
p u.new.a

# and the constant spelling
S = Struct.new(:a, :b)
p S.new.to_a
p S.new(1, 2).to_a

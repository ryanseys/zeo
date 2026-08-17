S = Struct.new(:a, :b)
s = S.new(1, 2)
t = s.class.new(3, 4)
p [t.a, t.b]
p t.class

D = Data.define(:x, :y)
d = D.new(x: 1, y: 2)
e = d.class.new(x: 5, y: 6)
p [e.x, e.y]

class Plain
  def initialize(v); @v = v; end
  def v; @v; end
end
pl = Plain.new(1)
p pl.class.new(9).v

Str = Struct.new(:name)
sn = Str.new("a")
p sn.class.new("b").name

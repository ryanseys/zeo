Pt = Struct.new(:x, :y)
a = Pt.new(1, 2).freeze
p(begin; a.x = 5; rescue => e; e.class; end)
b = Pt.new(1, 2).freeze
p(begin; b[0] = 5; rescue => e; e.class; end)
c = Pt.new(3, 4)
c.x = 9; p c.x
d = Pt.new(3, 4)
d[1] = 9; p d.y

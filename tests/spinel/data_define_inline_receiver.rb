a = Data.define(:x, :y).new(1, 2)
p a
p a.x
p a.y
p(Data.define(:x, :y).class)
p(Data.define(:x, :y).members)
b = Struct.new(:a, :b).new(3, 4)
p b.a
p b.b
p(Struct.new(:m, :n).members)
# named form still works
Pt = Data.define(:lat, :lng)
p Pt.new(10, 20)

k = Struct.new(:a)
p k.name
d = Data.define(:z)
v = d.name
p v
K = Struct.new(:a)
p K.name
p Struct.new(:a).name
i = k.new(1)
p i.a

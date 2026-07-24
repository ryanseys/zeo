name = :port
S = Struct.new(name)
p S.members
p S.new(5).port
a = :x; b = :y
T = Struct.new(a, b)
p T.members
p T.new(1, 2).y
U = Struct.new(:direct, b)
p U.members
f = :label
D = Data.define(f)
p D.members
p D.new(label: "hi").label

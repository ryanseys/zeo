# an empty array literal as a Data/Struct member takes the member's type
P = Data.define(:name, :deps)
a = P.new(name: "x", deps: ["d1"])
b = P.new(name: "y", deps: [])
p [a.deps, b.deps]
S1 = Struct.new(:name, :deps)
c1 = S1.new("x", [:s1])
d1 = S1.new("y", [])
p [c1.deps, d1.deps]
S2 = Struct.new(:v)
p [S2.new([1.5]).v, S2.new([]).v]
S3 = Struct.new(:v)
p [S3.new([[1]]).v, S3.new([]).v]
S4 = Struct.new(:v)
p [S4.new([1]).v, S4.new([]).v]

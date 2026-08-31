module PB; def size = super * 10; end
class Array; prepend PB; end
p [1, 2].size
SB = Struct.new(:a)
p SB.new(1).a
D2 = Data.define(:x)
p D2.new(x: 2).x

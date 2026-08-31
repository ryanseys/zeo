S1 = Struct.new(:a, :b)
K = Class.new(S1)
o = K.new(1, 2)
p [o.a, o.b, o.to_a]
class L < S1; end
p [L.new(3, 4).a, L.new(3, 4).to_a]

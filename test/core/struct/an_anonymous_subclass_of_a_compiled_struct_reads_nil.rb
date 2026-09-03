S1 = Struct.new(:a, :b)
K = Class.new(S1)
o = K.new(1, 2)
p [o.a, o.b, o.to_a]
class L < S1; end
p [L.new(3, 4).a, L.new(3, 4).to_a]
o.a = 9
p [o.a, o[:a], o.to_a]
p Class.new(K).new(7, 8).a
M = Class.new(S1) do
  def sum = a + b
end
p [M.new(2, 3).sum, M.new(2, 3).a]
KI = Struct.new(:x, :y, keyword_init: true)
p Class.new(KI).new(x: 1, y: 2).x
D1 = Data.define(:p, :q)
p Class.new(D1).new(p: 1, q: 2).p
p Class.new(Struct.new(:z)).new(6).z
__END__
[1, 2, [1, 2]]
[3, [3, 4]]
[9, 9, [9, 2]]
7
[5, 2]
1
1
6

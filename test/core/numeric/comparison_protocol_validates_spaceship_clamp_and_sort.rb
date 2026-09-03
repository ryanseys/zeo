class Temp
  include Comparable
  attr_reader :deg
  def initialize(d); @deg = d; end
  def <=>(o); (deg - o.deg).to_f; end
end
p [Temp.new(3), Temp.new(1), Temp.new(2)].sort.map(&:deg)
p Temp.new(5) < Temp.new(9)
p Temp.new(5).clamp(Temp.new(1), Temp.new(9)).deg
p Temp.new(5) == Temp.new(5)

class Ver
  include Comparable
  def initialize(n); @n = n; end
  attr_reader :n
  def <=>(o); o.is_a?(Ver) ? (n <=> o.n) : nil; end
end
a = Ver.new(1)
p a == Ver.new(2)
p a == a
p(begin; a < "x"; rescue => e; e.class; end)

p 5.clamp(1, nil)
p 5.clamp(nil, 3)
p(begin; 5.clamp(10, 1); rescue => e; e.message; end)
p(begin; 5.clamp(1...10); rescue => e; e.message; end)

x = "a"
p(begin; [1, x, 2].min; rescue => e; e.message; end)
p(begin; [1, x, 2].max; rescue => e; e.class; end)
p({ b: 2, a: 1, c: 3 }.sort)
__END__
[1, 2, 3]
true
5
true
false
true
ArgumentError
5
3
"min argument must be less than or equal to max argument"
"cannot clamp with an exclusive range"
"comparison of String with 1 failed"
ArgumentError
[[:a, 1], [:b, 2], [:c, 3]]

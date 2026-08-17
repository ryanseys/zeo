# `case obj when other` asks `other === obj`, which for a class defining #==
# is that method; comparing the C structs did not compile for a value type and
# compared addresses for a heap object (#3820).
class Eq1
  def initialize(v); @v = v; end
  def v; @v; end
  def ==(o); v == o.v; end
end
a = Eq1.new(1)
b = Eq1.new(1)
c = Eq1.new(2)
p(case b when a then "eq" else "ne" end)
p(case c when a then "eq" else "ne" end)
case b
when a then puts "stmt eq"
else puts "stmt ne"
end

class Ce
  def initialize(v); @v = v; end
  def v; @v; end
  def ===(o); v == o.v; end
end
x = Ce.new(5)
y = Ce.new(5)
p(case y when x then "ceq" else "cne" end)

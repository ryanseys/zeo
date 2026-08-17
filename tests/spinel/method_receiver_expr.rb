class Calc
  def initialize(b); @b = b; end
  def add(n); @b + n; end
end
p Calc.new(3).method(:add).receiver.class
m = Calc.new(4).method(:add)
p m.receiver.add(1)
p m.receiver.class.to_s

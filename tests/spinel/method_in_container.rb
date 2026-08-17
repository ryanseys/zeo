class Calc
  def initialize(b); @b = b; end
  def add(n); @b + n; end
end
ms = [Calc.new(3).method(:add)]
p ms[0].class
p ms[0].name
p ms[0].owner.to_s
p ms[0].receiver.class.to_s
p ms[0].parameters
p ms[0].arity
p ms[0].call(1)
p ms[0].unbind.class
p ms[0].to_proc.call(1)
h = { a: Calc.new(3).method(:add) }
p h[:a].name
p h[:a].call(2)

class Calc
  def kw(a, b: 2); a + b; end
  def opt(a, b = 1); a + b; end
  def initialize(base = 0); @base = base; end
  def add(n); @base + n; end
end
p Calc.new.method(:kw).call(1, b: 5)
p Calc.new.method(:kw).call(1)
p Calc.new.method(:opt).call(1)
p Calc.new.method(:opt).call(1, 9)
um = Calc.new(1).method(:add).unbind
p um.bind(Calc.new(10)).call(5)
p um.arity
p um.owner.to_s
p um.bind_call(Calc.new(2), 3)

module Greet
  def hi(n); "hi #{n}"; end
end
class Person
  include Greet
end
um2 = Greet.instance_method(:hi)
p um2.arity
p um2.bind(Person.new).call("x")

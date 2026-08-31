module PB; def inspect = "P" + super; end
class Array; prepend PB; end
p [1, 2].inspect

class Integer
  def inspect = "I" + super
end
p 5.inspect

module PT; def to_s = "T" + super; end
class Hash; prepend PT; end
p({ a: 1 }.to_s)

p Kernel.instance_method(:inspect).bind(5).call
p 5.inspect, [1, 2].inspect, ({ a: 1 }).inspect, "x".inspect, (1..2).inspect

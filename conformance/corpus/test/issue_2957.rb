class BO < BasicObject
  def initialize; @x = 0; end
end
b001 = BO.new; p(b001.instance_exec(1, 2, 3) { |*xs| xs })
b002 = BO.new; p(b002.instance_exec(9) { |*xs| xs })
b003 = BO.new; p(b003.instance_exec(2, 3) { |a, *rest| [a, rest] })
b004 = BO.new; p(b004.instance_exec { |*xs| xs })

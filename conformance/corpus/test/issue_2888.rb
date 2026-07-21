class Circle
  def initialize(r) = @r = r
  def area = 3.14 * @r * @r
end
class Square
  def initialize(s) = @s = s
  def area = @s * @s
end
REG = { "c" => Circle, "s" => Square }
p REG["c"].new(2).area
p REG["s"].new(3).area
%w[c s].each { |k| p REG[k].new(4).area }

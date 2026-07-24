# An object whose ivar comes from a destructured-pair block param, summed via a
# block and rounded (Refs #2913 sum, #2926 round).
class Shape
  def initialize(s) = @s = s
  def area = @s * 3.14
end

shapes = [["a", 2], ["b", 3]].map { |_, s| Shape.new(s) }
p shapes.sum { |x| x.area }.round(2)

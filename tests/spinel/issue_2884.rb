class Point
  attr_reader :x
  def initialize(x) = @x = x
  def ==(o) = o.is_a?(Point) && @x == o.x
end
pts = [Point.new(1), Point.new(2), Point.new(3)]
p pts.include?(Point.new(2))
p pts.include?(Point.new(9))
p pts.index(Point.new(3))
p pts.index(Point.new(9))
# uniq / Hash keys use eql? (identity for a class defining only ==), not ==
p [Point.new(1), Point.new(1), Point.new(2)].uniq.map(&:x)

class Shape
  include Comparable
  def area; 0; end
  def <=>(other); area <=> other.area; end
end
class Rectangle < Shape
  def initialize(a); @a = a; end
  def area; @a; end
end
class Circle < Shape
  def initialize(a); @a = a; end
  def area; @a; end
end

shapes = [Rectangle.new(3), Circle.new(1), Rectangle.new(2)]
p shapes.sort.map(&:area)
p shapes.min.area
p shapes.max.area
p(Rectangle.new(4) > Rectangle.new(2))
p(Rectangle.new(4) < Circle.new(9))
p(Circle.new(1).clamp(Rectangle.new(2), Rectangle.new(5)).area)

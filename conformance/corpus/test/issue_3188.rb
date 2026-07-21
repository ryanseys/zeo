class Shape
  include Comparable
  def area; raise NotImplementedError; end
  def <=>(other); area <=> other.area; end
end
class Rectangle < Shape
  def initialize(w, h); @w = w; @h = h; end
  def area; @w * @h; end
end
class Square < Rectangle
  def initialize(s); super(s, s); end
end
p(Square.new(4) > Rectangle.new(2, 3))
p([Rectangle.new(3, 4), Square.new(5)].sort.map(&:area))

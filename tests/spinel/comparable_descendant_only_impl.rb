# A base-class method calling a name defined ONLY in descendants (here
# Comparable's <=> reading `area`) types as the unify of the descendant
# returns; it previously stayed unknown and emit_boxed discarded the
# virtual dispatch's value through the effect-comma nil (#3237).
class Shape
  include Comparable
  def <=>(other) = area <=> other.area
end
class Rect < Shape
  def initialize(a) = (@a = a)
  def area = @a
end
class Circ < Shape
  def initialize(a) = (@a = a)
  def area = @a * 1.0
end
p(Rect.new(4) > Rect.new(2))
p(Rect.new(3) <=> Circ.new(2))
p(Circ.new(5) < Circ.new(6))
p(Rect.new(2) == Circ.new(2))

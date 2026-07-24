# Set membership uses #eql?/#hash (like CRuby's Hash-backed Set), so an
# equal-but-distinct value object is found, and 1 / 1.0 are distinct members.
require "set"

class Point
  attr_reader :x, :y
  def initialize(x, y); @x = x; @y = y; end
  def hash = [x, y].hash
  def eql?(other) = other.is_a?(Point) && x == other.x && y == other.y
end

s = Set.new
s << Point.new(3, 4)
p s.include?(Point.new(3, 4))
p s.add?(Point.new(3, 4))
p s.size
s.delete(Point.new(3, 4))
p s.include?(Point.new(3, 4))

n = Set.new
n << 1
p n.include?(1.0)
p n.include?(1)

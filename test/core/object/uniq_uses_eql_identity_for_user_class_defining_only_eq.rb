# uniq (and Array#eql?) dedup by eql?/hash, not ==. A class that defines
# only == keeps every instance distinct (Object#eql? is identity), so
# two equal-by-== Points both survive uniq -- unlike include?/index,
# which do use ==.

class Point
  attr_reader :x
  def initialize(x) = @x = x
  def ==(o) = o.is_a?(Point) && @x == o.x
end
pts = [Point.new(1), Point.new(2), Point.new(3)]
p pts.include?(Point.new(2))
p pts.index(Point.new(3))
p [Point.new(1), Point.new(1), Point.new(2)].uniq.map(&:x)
p [1.0, 1, 1, 2].uniq
p [1].eql?([1])
p [1].eql?([1.0])
__END__
true
2
[1, 1, 2]
[1.0, 1, 2]
true
false

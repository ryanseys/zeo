class Point
  attr_reader :x

  def initialize(x)
    @x = x
  end

  def ==(other)
    other.is_a?(Point) && x == other.x
  end
end

puts Point.new(1) == Point.new(1)
puts Point.new(1) == Point.new(2)
puts Point.new(1) == 5
puts Point.new(1) != Point.new(2)
puts [Point.new(1), Point.new(2)] == [Point.new(1), Point.new(2)]
__END__
true
false
false
true
true

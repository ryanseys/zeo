class Point
  attr_accessor :x, :y

  def initialize(x, y)
    @x = x
    @y = y
  end
end

p1 = Point.new(1, 2)
p2 = p1.dup
p2.x = 99
puts p1.x
puts p2.x
puts p2.y
p1.freeze
puts p1.clone.frozen?
puts p1.dup.frozen?
__END__
1
99
2
true
false

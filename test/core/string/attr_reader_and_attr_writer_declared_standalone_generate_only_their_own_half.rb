class Point
  attr_reader :x
  attr_writer :y
  def initialize(x, y)
    @x = x
    @y = y
  end
  def show_y
    @y
  end
end
p1 = Point.new(1, 2)
puts p1.x
p1.y = 99
puts p1.show_y
__END__
1
99

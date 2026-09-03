class Point
  def initialize(x, y)
    @x = x
    @y = y
  end
  def deconstruct
    [@x, @y]
  end
end
p1 = Point.new(1, 2)
case p1
in [px, py]
  puts "array: #{px}, #{py}"
end
__END__
array: 1, 2

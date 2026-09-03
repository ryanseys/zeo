class Point
  def initialize(x, y)
    @x = x
    @y = y
  end
  def deconstruct_keys(keys)
    { x: @x, y: @y }
  end
end
p1 = Point.new(1, 2)
case p1
in Point(x:, y:)
  puts "constant: #{x}, #{y}"
end
case p1
in { x:, y: }
  puts "plain: #{x}, #{y}"
end
__END__
constant: 1, 2
plain: 1, 2

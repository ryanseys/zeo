# Struct: `Point = Struct.new`
# becomes an ordinary `class Point < Struct` (accessors, initialize,
# members/to_a/to_h/==/[]/each_pair, Enumerable via the real chain), with
# keyword_init and block-with-methods forms.

Point = Struct.new(:x, :y)

pt = Point.new(3, 4)
p pt
puts pt.x
pt.y = 9
p pt.to_a
p pt.to_h
p pt.members
p pt == Point.new(3, 9)
p pt[0]
p pt[:y]
pt[1] = 12
p pt.y
p pt.map { |v| v * 2 }
pt.each_pair { |k, v| puts "#{k}=#{v}" }
p Point.ancestors.include?(Struct)

Label = Struct.new(:text, :size, keyword_init: true)
l = Label.new(text: "hi", size: 12)
p l
puts l.text

Vec2 = Struct.new(:dx, :dy) do
  def norm
    Math.sqrt(dx * dx + dy * dy)
  end
end
puts Vec2.new(3, 4).norm
p Point.new(1).y
__END__
#<struct Point x=3, y=4>
3
[3, 9]
{x: 3, y: 9}
[:x, :y]
true
3
9
12
[6, 24]
x=3
y=12
true
#<struct Label text="hi", size=12>
hi
5.0
nil

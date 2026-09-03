# `Point = Struct.new(:x, :y)` mints a native struct class at runtime (Batch
# E) and the constant write names it: accessors, positional init (nil-filled),
# members/to_a/to_h/==/[]/[]=/each_pair/inspect, and Enumerable through the
# real ancestor chain. Plus keyword_init and the block-with-methods form. The
# constant form now takes the exact same runtime path as the anonymous one --
# no compile-time synthesis.

Point = Struct.new(:x, :y)
pt = Point.new(1, 2)
p pt
puts pt.x
pt.y = 9
p pt.to_a
p pt.to_h
p pt.members
p pt == Point.new(1, 9)
p pt == Point.new(1, 2)
p pt[0]
p pt[:y]
p pt["x"]
p pt[-1]
pt[1] = 20
p pt.y
p pt.length
p pt.map { |v| v }
p pt.select { |v| v.is_a?(Integer) }
acc = []
pt.each_pair { |k, v| acc << [k, v] }
p acc
p Point.ancestors.include?(Struct)
p Point.ancestors.include?(Enumerable)
Label = Struct.new(:text, keyword_init: true)
l = Label.new(text: "hi")
p l
Pair = Struct.new(:a, :b) do
  def total
    a + b
  end
end
p Pair.new(3, 4).total
p Point.new(5).y
__END__
#<struct Point x=1, y=2>
1
[1, 9]
{x: 1, y: 9}
[:x, :y]
true
false
1
9
1
9
20
2
[1, 20]
[1, 20]
[[:x, 1], [:y, 20]]
true
true
#<struct Label text="hi">
7
nil

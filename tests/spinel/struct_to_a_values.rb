# Struct#to_a and #values gather the member values into an array,
# preserving declaration order. Members may be heterogeneous.
Person = Struct.new(:name, :age)
pp = Person.new("Alice", 30)
p pp.values
p pp.to_a
puts pp.values.inspect
Point = Struct.new(:x, :y)
p Point.new(3, 4).to_a
Mixed = Struct.new(:a, :b, :c)
p Mixed.new(1.5, [1, 2, 3], "hi").to_a
p Mixed.new(1.5, [1, 2, 3], "hi").values

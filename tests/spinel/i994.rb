Person = Struct.new(:name, :age)
p = Person.new("Alice", 30)
puts p.name
puts p.age
puts p[:name]
puts p[:age]
puts p[0]
puts p[1]
puts p[-1]

Point = Struct.new(:x, :y) do
  def to_s
    "(#{x}, #{y})"
  end
end
pt = Point.new(3, 4)
puts pt.to_s
puts pt

class Shape
  def initialize(name)
    @name = name
  end

  def name
    @name
  end
end

class Circle < Shape
end

factory = Circle
c = factory.new("round")
puts c.name
puts c.class
puts c.class == Circle
puts c.is_a?(Shape)
puts c.instance_of?(Shape)
puts Circle.ancestors.include?(Shape)

[1, "one", :one, nil, factory].each do |v|
  puts v.class
end

case c
when Circle
  puts "circle branch"
end

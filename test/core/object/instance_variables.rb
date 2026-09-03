# Any object can reflect over its instance variables by name.
class Point
  def initialize(x, y)
    @x = x
    @y = y
  end
end

p1 = Point.new(3, 4)
p p1.instance_variables               # [:@x, :@y]
p p1.instance_variable_get(:@x)       # 3

# Setting works too, and the string form of the name is accepted.
p1.instance_variable_set(:@x, 30)
p p1.instance_variable_get("@x")      # 30

# A never-declared name reads as nil; an object with no ivars lists none.
p p1.instance_variable_get(:@z)       # nil
class Empty; end
p Empty.new.instance_variables        # []

# The same reflection works when the receiver's class isn't statically known
# -- e.g. a method parameter that receives objects of different classes.
class Circle
  def initialize(r)
    @r = r
  end
end

def radius_or_x(shape)
  shape.instance_variable_get(:@r) || shape.instance_variable_get(:@x)
end

p radius_or_x(Circle.new(5))          # 5
p radius_or_x(Point.new(1, 2))        # 1

# A malformed ivar name raises NameError.
begin
  p1.instance_variable_get(:x)
rescue NameError => e
  puts e.message                      # 'x' is not allowed as an instance variable name
end
__END__
[:@x, :@y]
3
30
nil
[]
5
1
'x' is not allowed as an instance variable name

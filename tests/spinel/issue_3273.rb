class MyClass
  def initialize
    @foo, @bar = nil, nil
    @x, @y = 1, 2
  end
  def vals = [@foo, @bar, @x, @y]
end
p MyClass.new.vals

class C2
  def initialize
    @a, @b, @c = "p", "q", "r"
  end
  def read = [@a, @b, @c]
end
p C2.new.read

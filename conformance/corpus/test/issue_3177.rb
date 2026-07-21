module Factory
  def create(a, b)
    build(a, b)
  end
  def build(a, b)
    new(a, b)
  end
end
class Product
  extend Factory
  def initialize(a, b); @a = a; @b = b; end
  def a; @a; end
  def sum; @a + @b; end
end
p Product.create(1, 2).a
p Product.create(3, 4).sum
p Product.build(5, 6).sum

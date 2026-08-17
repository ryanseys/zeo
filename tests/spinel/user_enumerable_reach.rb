# Three ways an object of a class that includes Enumerable failed to reach the
# mixin: called with an implicit self from another method of the class, read
# out of a container (where it is boxed and every poly op answered for an empty
# collection), and handed to zip as the argument rather than the receiver.
class Nums
  include Enumerable

  def initialize(*xs)
    @xs = xs
  end

  def each
    @xs.each { |x| yield x }
  end

  # implicit self: the redirect keyed on a receiver node, and this call has none
  def total; sum; end
  def size_via_count; count; end
  def biggest; max; end
end

n = Nums.new(1, 2, 3)
p n.total
p n.size_via_count
p n.biggest

# boxed in a container: the poly ops fell to a default that answered 0/nil/false
[Nums.new(1, 2)].each { |o| p o.sum }
[Nums.new(1, 2)].each { |o| p o.count }
[Nums.new(4, 5)].each { |o| p o.max }
[Nums.new(4, 5)].each { |o| p o.min }
[Nums.new(4, 5)].each { |o| p o.to_a }
[Nums.new(4, 5)].each { |o| p o.include?(5) }

# as zip's argument, not its receiver
p Nums.new(1, 2).zip(Nums.new(3, 4))
p [1, 2].zip(Nums.new(5, 6))

# the forms that already worked keep working
p n.sum
p n.to_a
p n.sort
def via_param(o); o.sum; end
p via_param(Nums.new(7, 8))

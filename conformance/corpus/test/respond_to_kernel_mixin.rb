class C; end
c = C.new
puts c.respond_to?(:display)
puts c.respond_to?(:yield_self)
puts c.respond_to?(:no_such_method)
class Nums
  include Enumerable
  def initialize(*xs)
    @xs = xs
  end
  def each
    @xs.each { |x| yield x }
  end
end
p(Nums.new(1, 2).respond_to?(:map))
p(Nums.new(1, 2).respond_to?(:sort_by))
p(Nums.new(1, 2).respond_to?(:no_such))
p(:abc.respond_to?(:to_proc))
pr = ->(x) { x }
p pr.respond_to?(:call)

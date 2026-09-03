# The `.new` binding fix: one `initialize(items = nil)` serving both
# `Bag.new` and `Bag.new([1])` (oracle: 0, 1).

class Bag
  def initialize(items = nil)
    @n = items.nil? ? 0 : 1
  end
  def n
    @n
  end
end
puts Bag.new.n
puts Bag.new([1]).n
__END__
0
1

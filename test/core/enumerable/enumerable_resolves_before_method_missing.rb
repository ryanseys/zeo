# The send-ladder fidelity fix: a REAL module method (Enumerable's `map`,
# via `include Enumerable` + `each`) resolves BEFORE `method_missing` --
# previously method_missing fired first, the opposite of real Ruby.

class Sack
  include Enumerable
  def initialize(items)
    @items = items
  end
  def each(&b)
    @items.each(&b)
    self
  end
  def method_missing(name, *a)
    "mm:#{name}"
  end
end

s = Sack.new([1, 2, 3])
puts s.map { |x| x * 10 }.inspect
puts s.nope
__END__
[10, 20, 30]
mm:nope

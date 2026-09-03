# The old "isn't supported yet for non-Int/Float operands" panic is now
# dynamic dispatch: String#+ via send_value's table, and a user class's
# own operator method chained through a Poly intermediate (`a << 1`
# returns self as Poly; the second `<<` dispatches dynamically).

module Cat
  def self.concat2(a, b)
    a + b
  end
end
puts Cat.concat2("foo", "bar")
class Acc
  def initialize
    @items = []
  end
  def <<(item)
    @items << item
    self
  end
  def size
    @items.length
  end
end
a = Acc.new
a << 1 << 2 << 3
puts a.size
__END__
foobar
3

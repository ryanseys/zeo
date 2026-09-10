# Unary operator methods `def -@` / `def +@` must mangle to valid C
# identifiers and dispatch on `-obj` and `+obj`. A money or duration class
# negating through `-@` is the usual reason to define one.
class Money
  attr_reader :cents
  def initialize(c)
    @cents = c
  end
  def -@
    Money.new(-@cents)
  end
  def +@
    Money.new(@cents)
  end
end

puts((-Money.new(5)).cents)
puts((+Money.new(7)).cents)
puts((-(-Money.new(9))).cents)
__END__
-5
7
9

# Unary operator methods `def -@` / `def +@` must mangle to valid C
# identifiers and dispatch on `-obj` / `+obj`. Found via the spinelgems
# harness (latinum, as-duration negate via `-@`).
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
